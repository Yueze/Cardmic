//! The menu bar app: state, menu, icon.

use crate::devices::{self, Devices};
use crate::platform::{self, MicAccess};
use crate::settings::Settings;
use cardmic_audio::Candidate;
use cardmic_core::pairing::normalize_code;
use cardmic_engine::{output, pairing_store, Engine, Link, Options, StartError, Status};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES: &str = "https://github.com/Yueze/Cardmic/releases";
#[cfg(target_os = "macos")]
const HELP: &str = "https://github.com/Yueze/Cardmic/blob/main/docs/macos.md";
#[cfg(target_os = "windows")]
const HELP: &str = "https://github.com/Yueze/Cardmic/blob/main/docs/windows.md";

/// Held for the app's lifetime so a second copy can tell one is running,
/// and ask it to show its menu instead of starting.
const INSTANCE_PORT: u16 = 41239;
const TICK: Duration = Duration::from_millis(200);

enum UserEvent {
    Menu(MenuEvent),
    Devices(Devices),
    Started(u64, Result<Engine, Failure>),
    /// The engine reported something; refresh now rather than at the next tick.
    Engine,
    Code(Option<String>),
    /// The answer to the microphone permission prompt.
    MicAnswered(bool),
    /// Cardmic was opened again while running: show the menu.
    Show,
}

enum Failure {
    NoLoopback,
    NeedsMicAccess,
    MicDenied,
    PortInUse,
    Other(String),
}

enum Wifi {
    Off,
    Starting,
    Running(Engine),
    NoLoopback,
    /// Waiting for the answer to the microphone permission prompt.
    AskingMic,
    MicDenied,
    /// UDP port 41234 is taken: `cardmic run` in a terminal, most likely.
    Busy,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Idle,
    Live,
    Off,
    Warn,
}

/// What decides the menu's structure (as opposed to its texts).
#[derive(Clone, PartialEq, Eq)]
struct Shape {
    connected: bool,
    usb: Option<String>,
    needs_loopback: bool,
    mic_denied: bool,
    paired: bool,
    loopbacks: Vec<Candidate>,
    output: Option<String>,
    manual: bool,
}

struct Items {
    status: MenuItem,
    hint: MenuItem,
    level: MenuItem,
    wifi: CheckMenuItem,
    automatic: CheckMenuItem,
    outputs: Vec<(MenuId, String)>,
    pair: MenuItem,
    unpair: MenuItem,
    install: MenuItem,
    privacy: MenuItem,
    retry: MenuItem,
    sound: MenuItem,
    login: CheckMenuItem,
    help: MenuItem,
    updates: MenuItem,
    quit: MenuItem,
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    tray: TrayIcon,
    items: Items,
    shape: Option<Shape>,
    texts: (String, String, String),
    glyph: Option<Glyph>,
    settings: Settings,
    devices: Devices,
    wifi: Wifi,
    generation: u64,
    retry_at: Option<Instant>,
    asking_code: bool,
    first_launch: bool,
    /// The menu currently attached to the icon.
    menu: Menu,
}

/// Append a line to the log: ~/Library/Logs/Cardmic.log on macOS,
/// %LOCALAPPDATA%\Cardmic\cardmic.log on Windows. For support; never
/// contains the pairing code.
pub fn log(line: &str) {
    use std::io::Write as _;
    let path = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Logs/Cardmic.log"))
    } else {
        std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Cardmic").join("cardmic.log"))
    };
    let Some(path) = path else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Start over rather than grow without bound.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{secs} {line}");
    }
}

/// `--diagnose`: what the app would see, for support and testing.
pub fn diagnose() {
    let d = devices::scan();
    println!("cardmic-app {VERSION}");
    println!("login item: {}", if platform::login_item_enabled() { "on" } else { "off" });
    println!("microphone access: {:?}", platform::mic_access());
    println!("paired: {}", pairing_store::load().is_some());
    println!("usb mic: {}", d.usb_mic.as_deref().unwrap_or("none"));
    for c in &d.loopbacks {
        println!("loopback candidate: {}", c.label());
    }
    let host = cpal::default_host();
    let settings = Settings::load();
    match output::known(&host, settings.output.as_deref()) {
        Some(c) => println!("would play into (no probe needed): {}", c.label()),
        None => println!("would play into: needs the probe"),
    }
}

pub fn run() {
    let instance = match TcpListener::bind(("127.0.0.1", INSTANCE_PORT)) {
        Ok(l) => l,
        Err(_) => {
            // Already running: have that copy show its menu, where the user is.
            if let Ok(mut s) = TcpStream::connect_timeout(&(std::net::Ipv4Addr::LOCALHOST, INSTANCE_PORT).into(), Duration::from_secs(1)) {
                let _ = s.write_all(b"show\n");
            }
            return;
        }
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_dock_visibility(false);
    }

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |e| {
            let _ = proxy.send_event(UserEvent::Menu(e));
        }
    }));

    let mut instance = Some(instance);
    let mut app: Option<App> = None;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        match event {
            // On macOS the tray icon must be created once the loop is running.
            Event::NewEvents(StartCause::Init) => {
                if let Some(l) = instance.take() {
                    listen_for_reopen(l, proxy.clone());
                    app = Some(App::new(proxy.clone()));
                }
            }
            // Opened again from Finder, Launchpad or Spotlight while running.
            Event::Reopen { .. } => {
                if let Some(a) = app.as_mut() {
                    a.show_menu();
                }
            }
            Event::UserEvent(e) => {
                if let Some(a) = app.as_mut() {
                    if a.handle(e) {
                        app = None; // drops the engine before exiting
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                    a.refresh();
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if let Some(a) = app.as_mut() {
                    a.tick();
                }
            }
            _ => {}
        }
    });
}

/// A second copy of the app connects here to ask this one to show its menu.
fn listen_for_reopen(listener: TcpListener, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut buf = [0u8; 8];
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            if matches!(stream.read(&mut buf), Ok(n) if buf[..n].starts_with(b"show")) {
                let _ = proxy.send_event(UserEvent::Show);
            }
        }
    });
}

impl App {
    fn new(proxy: EventLoopProxy<UserEvent>) -> App {
        log(&format!("Cardmic {VERSION} started"));
        platform::migrate_login_item();
        let mut settings = Settings::load();
        let first_launch = !settings.login_item_offered;
        if first_launch {
            // A microphone you have to remember to start is one you do not
            // use; the menu has the switch to turn this off.
            if platform::login_item_blocker().is_none() {
                let _ = platform::set_login_item(true);
            }
            settings.login_item_offered = true;
            settings.save();
        }

        let items = Items::new();
        let menu = Menu::new();
        let tray = TrayIconBuilder::new()
            .with_tooltip("Cardmic")
            .with_icon(icon(Glyph::Idle))
            .with_icon_as_template(cfg!(target_os = "macos"))
            .with_menu(Box::new(menu.clone()))
            .build()
            .expect("create the menu bar icon");

        devices::watch({
            let proxy = proxy.clone();
            move |d| {
                let _ = proxy.send_event(UserEvent::Devices(d));
            }
        });

        let mut app = App {
            proxy,
            tray,
            items,
            shape: None,
            texts: Default::default(),
            glyph: None,
            devices: Devices::default(),
            wifi: Wifi::Off,
            settings,
            generation: 0,
            retry_at: None,
            asking_code: false,
            first_launch,
            menu,
        };
        if app.settings.wifi {
            app.start();
        }
        app.refresh();
        app
    }

    // ------------------------------------------------------------ engine

    /// (Re)start receiving with the current settings, on a worker thread:
    /// choosing a device can take a second or two.
    fn start(&mut self) {
        self.generation += 1;
        self.wifi = Wifi::Starting; // drops a running engine, freeing its port
        self.retry_at = None;
        let generation = self.generation;
        let settings = self.settings.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let host = cpal::default_host();
            let target = if settings.output_manual {
                settings.output.as_deref().map(|n| output::named(&host, n)).ok_or(Failure::NoLoopback)
            } else if let Some(c) = output::known(&host, settings.output.as_deref()) {
                Ok(c)
            } else {
                // Telling loopbacks from dead ends means recording from them.
                match platform::mic_access() {
                    MicAccess::Granted => output::choose(&host, None).ok_or(Failure::NoLoopback),
                    MicAccess::NotAsked => Err(Failure::NeedsMicAccess),
                    MicAccess::Denied => Err(Failure::MicDenied),
                }
            };
            let result = match target {
                Err(f) => Err(f),
                Ok(output) => {
                    let events = proxy.clone();
                    Engine::start(
                        Options { output, keys: pairing_store::load_keys(), devices: Vec::new() },
                        move |_| {
                            let _ = events.send_event(UserEvent::Engine);
                        },
                    )
                    .map_err(|e| match e {
                        StartError::PortInUse => Failure::PortInUse,
                        StartError::Other(e) => Failure::Other(e),
                    })
                }
            };
            let _ = proxy.send_event(UserEvent::Started(generation, result));
        });
    }

    fn stop(&mut self) {
        self.generation += 1;
        self.wifi = Wifi::Off;
        self.retry_at = None;
    }

    fn status(&self) -> Option<Status> {
        match &self.wifi {
            Wifi::Running(e) => Some(e.status()),
            _ => None,
        }
    }

    // ------------------------------------------------------------ events

    /// Returns true to quit.
    fn handle(&mut self, event: UserEvent) -> bool {
        match event {
            UserEvent::Started(generation, result) => {
                if generation != self.generation {
                    return false; // superseded; dropping the engine stops it
                }
                log(&match &result {
                    Ok(e) => format!("receiving; playing into {:?}", e.status().output),
                    Err(Failure::NoLoopback) => "no working loopback device".into(),
                    Err(Failure::NeedsMicAccess) => "asking for microphone access".into(),
                    Err(Failure::MicDenied) => "microphone access is off".into(),
                    Err(Failure::PortInUse) => "UDP 41234 in use".into(),
                    Err(Failure::Other(e)) => format!("start failed: {e}"),
                });
                self.wifi = match result {
                    Ok(engine) => {
                        if !self.settings.output_manual {
                            let chosen = engine.status().output;
                            if self.settings.output.as_deref() != Some(chosen.as_str()) {
                                self.settings.output = Some(chosen);
                                self.settings.save();
                            }
                        }
                        Wifi::Running(engine)
                    }
                    Err(Failure::NoLoopback) => Wifi::NoLoopback,
                    Err(Failure::NeedsMicAccess) => {
                        let proxy = self.proxy.clone();
                        platform::request_mic_access(move |granted| {
                            let _ = proxy.send_event(UserEvent::MicAnswered(granted));
                        });
                        Wifi::AskingMic
                    }
                    Err(Failure::MicDenied) => {
                        self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                        Wifi::MicDenied
                    }
                    Err(Failure::PortInUse) => {
                        self.retry_at = Some(Instant::now() + Duration::from_secs(5));
                        Wifi::Busy
                    }
                    Err(Failure::Other(e)) => {
                        self.retry_at = Some(Instant::now() + Duration::from_secs(10));
                        Wifi::Failed(e)
                    }
                };
            }
            UserEvent::Devices(d) => {
                let loopbacks_changed = d.loopbacks != self.devices.loopbacks;
                self.devices = d;
                // A loopback device was just installed (or removed): try again.
                if loopbacks_changed && matches!(self.wifi, Wifi::NoLoopback) {
                    self.start();
                }
            }
            UserEvent::Engine => {}
            UserEvent::Show => self.show_menu(),
            UserEvent::MicAnswered(granted) => {
                log(&format!("microphone access answered: {granted}"));
                if matches!(self.wifi, Wifi::AskingMic) {
                    if granted {
                        self.start();
                    } else {
                        self.retry_at = Some(Instant::now() + Duration::from_secs(3));
                        self.wifi = Wifi::MicDenied;
                    }
                }
            }
            UserEvent::Code(code) => {
                self.asking_code = false;
                if let Some(text) = code {
                    match normalize_code(&text) {
                        Ok(code) => match pairing_store::save(&code) {
                            Ok(_) => {
                                if self.settings.wifi {
                                    self.start();
                                }
                            }
                            Err(e) => platform::alert("Could not save the pairing code", &e),
                        },
                        Err(e) => {
                            platform::alert(
                                "That is not a pairing code",
                                &format!("{e}. The code is on the Cardputer in Cardmic > Settings > Pairing, e.g. 7K2M-9QXB-4TPA."),
                            );
                            self.ask_code();
                        }
                    }
                }
            }
            UserEvent::Menu(e) => return self.menu(e.id),
        }
        false
    }

    fn menu(&mut self, id: MenuId) -> bool {
        let it = &self.items;
        if id == *it.quit.id() {
            self.stop();
            return true;
        } else if id == *it.wifi.id() {
            self.settings.wifi = it.wifi.is_checked();
            self.settings.save();
            if self.settings.wifi {
                self.start();
            } else {
                self.stop();
            }
        } else if id == *it.automatic.id() {
            self.settings.output_manual = false;
            self.settings.save();
            self.shape = None; // re-sync the check marks
            if self.settings.wifi {
                self.start();
            }
        } else if let Some((_, name)) = it.outputs.iter().find(|(oid, _)| *oid == id) {
            self.settings.output = Some(name.clone());
            self.settings.output_manual = true;
            self.settings.save();
            self.shape = None;
            if self.settings.wifi {
                self.start();
            }
        } else if id == *it.pair.id() {
            self.ask_code();
        } else if id == *it.unpair.id() {
            match pairing_store::remove() {
                Ok(_) => {
                    if self.settings.wifi {
                        self.start();
                    }
                }
                Err(e) => platform::alert("Could not forget the pairing code", &e),
            }
        } else if id == *it.install.id() {
            platform::open_url(output::install_hint().1);
        } else if id == *it.privacy.id() {
            platform::open_url(platform::PRIVACY_URL);
        } else if id == *it.retry.id() {
            self.settings.wifi = true;
            self.settings.save();
            self.start();
        } else if id == *it.sound.id() {
            platform::open_sound_settings();
        } else if id == *it.login.id() {
            let want = it.login.is_checked();
            if let Err(e) = platform::set_login_item(want) {
                it.login.set_checked(!want);
                platform::alert("Could not change Open at Login", &e);
            }
        } else if id == *it.help.id() {
            platform::open_url(HELP);
        } else if id == *it.updates.id() {
            platform::open_url(RELEASES);
        }
        false
    }

    fn ask_code(&mut self) {
        if self.asking_code {
            return;
        }
        self.asking_code = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let code = platform::ask_code(
                "Type the code shown on the Cardputer in Cardmic > Settings > Pairing (turn Pairing on first).",
            );
            let _ = proxy.send_event(UserEvent::Code(code));
        });
    }

    fn tick(&mut self) {
        if self.retry_at.is_some_and(|t| Instant::now() >= t) && self.settings.wifi {
            if matches!(self.wifi, Wifi::MicDenied) && platform::mic_access() != MicAccess::Granted {
                // Still off: look again in a moment, quietly.
                self.retry_at = Some(Instant::now() + Duration::from_secs(3));
            } else {
                self.start();
            }
        }
        if self.first_launch && self.shape.is_some() {
            // Show where the app lives, once.
            self.first_launch = false;
            #[cfg(target_os = "macos")]
            {
                if platform::icon_visible(&self.tray) {
                    self.tray.show_menu();
                } else {
                    std::thread::spawn(|| {
                        platform::alert(
                            "Cardmic is running",
                            "Your menu bar is too full to show Cardmic's icon (three dots), so macOS hides it. \
                             Open Cardmic again from Applications or Spotlight at any time to show its menu.",
                        )
                    });
                }
            }
            #[cfg(target_os = "windows")]
            std::thread::spawn(|| {
                platform::alert(
                    "Cardmic is running",
                    "Find its icon, three dots, in the notification area next to the clock (click ^ if it is hidden). \
                     Its menu shows what to pick as the microphone in your apps.",
                )
            });
        }
        self.refresh();
    }

    // ------------------------------------------------------------ view

    /// Open the menu: from the icon if it can be seen, else at the pointer.
    fn show_menu(&mut self) {
        self.refresh();
        #[cfg(target_os = "macos")]
        if !platform::icon_visible(&self.tray) {
            platform::pop_up_menu(&self.menu);
            return;
        }
        self.tray.show_menu();
    }

    fn refresh(&mut self) {
        let status = self.status();
        let connected = matches!(status.as_ref().map(|s| s.link), Some(Link::Connected { .. }));
        let shape = Shape {
            connected,
            usb: self.devices.usb_mic.clone(),
            needs_loopback: matches!(self.wifi, Wifi::NoLoopback),
            mic_denied: matches!(self.wifi, Wifi::MicDenied),
            paired: pairing_store::load().is_some(),
            loopbacks: self.devices.loopbacks.clone(),
            output: status.as_ref().map(|s| s.output.clone()).or_else(|| self.settings.output.clone()),
            manual: self.settings.output_manual,
        };
        if self.shape.as_ref() != Some(&shape) {
            self.rebuild(&shape);
            self.shape = Some(shape);
            self.texts = Default::default();
        }

        let texts = self.describe(status.as_ref());
        if texts.0 != self.texts.0 {
            self.items.status.set_text(&texts.0);
            let _ = self.tray.set_tooltip(Some(format!("Cardmic — {}", texts.0)));
        }
        if texts.1 != self.texts.1 {
            self.items.hint.set_text(&texts.1);
        }
        if texts.2 != self.texts.2 {
            self.items.level.set_text(&texts.2);
        }
        self.texts = texts;

        let glyph = if connected || self.devices.usb_mic.is_some() {
            Glyph::Live
        } else if matches!(self.wifi, Wifi::Off) {
            Glyph::Off
        } else if matches!(self.wifi, Wifi::NoLoopback | Wifi::AskingMic | Wifi::MicDenied | Wifi::Busy | Wifi::Failed(_))
            || status.as_ref().is_some_and(|s| s.auth_failures > 0)
        {
            Glyph::Warn
        } else {
            Glyph::Idle
        };
        if self.glyph != Some(glyph) {
            #[cfg(target_os = "macos")]
            let _ = self.tray.set_icon_with_as_template(Some(icon(glyph)), true);
            #[cfg(not(target_os = "macos"))]
            let _ = self.tray.set_icon(Some(icon(glyph)));
            self.glyph = Some(glyph);
        }
    }

    /// Status line, hint line, level line.
    fn describe(&self, status: Option<&Status>) -> (String, String, String) {
        let (install_name, _) = output::install_hint();
        match (&self.wifi, status) {
            (Wifi::Off, _) => (
                "Wi-Fi receiving is off".into(),
                "USB works without this app".into(),
                String::new(),
            ),
            (Wifi::Starting, _) => ("Getting ready…".into(), String::new(), String::new()),
            (Wifi::NoLoopback, _) => (
                "Wi-Fi needs a virtual audio device".into(),
                format!("Install {install_name}, then come back here"),
                String::new(),
            ),
            (Wifi::AskingMic, _) => (
                "Allow microphone access to finish setup".into(),
                "Cardmic tests your virtual audio devices once".into(),
                String::new(),
            ),
            (Wifi::MicDenied, _) => (
                "Microphone access is off for Cardmic".into(),
                "Turn it on to test virtual audio devices".into(),
                String::new(),
            ),
            (Wifi::Busy, _) => (
                "Another Cardmic is receiving".into(),
                "Close `cardmic run` in the terminal; this retries".into(),
                String::new(),
            ),
            (Wifi::Failed(e), _) => ("Wi-Fi problem".into(), clip(e, 60), String::new()),
            (Wifi::Running(_), Some(s)) => match s.link {
                Link::Connected { encrypted, .. } => (
                    format!("Connected over Wi-Fi{}", if encrypted { " · encrypted" } else { "" }),
                    format!("In your app, choose “{}” as the microphone", s.mic),
                    format!("Level  {}", meter(s.level)),
                ),
                Link::Searching if s.auth_failures > 0 => (
                    "Found a Cardputer, but the code is wrong".into(),
                    "Pairing code changed: choose Pair with Cardputer…".into(),
                    String::new(),
                ),
                Link::Searching => (
                    "Looking for your Cardputer…".into(),
                    if s.paired {
                        "Open Cardmic on it, on this computer's Wi-Fi".into()
                    } else {
                        "Open Cardmic on it. Pairing on? Pair first".into()
                    },
                    String::new(),
                ),
            },
            (Wifi::Running(_), None) => unreachable!("a running engine always has a status"),
        }
    }

    fn rebuild(&mut self, shape: &Shape) {
        let it = &mut self.items;
        let menu = Menu::new();
        let sep = PredefinedMenuItem::separator;

        let _ = menu.append(&it.status);
        let _ = menu.append(&it.hint);
        if shape.connected {
            let _ = menu.append(&it.level);
        }
        if let Some(usb) = &shape.usb {
            let _ = menu.append(&MenuItem::new(format!("USB connected: choose “{usb}”"), false, None));
        }
        let _ = menu.append(&sep());

        it.wifi.set_checked(self.settings.wifi);
        let _ = menu.append(&it.wifi);
        if shape.mic_denied {
            let _ = menu.append(&it.privacy);
        }
        if shape.needs_loopback {
            it.install.set_text(format!("Install {}…", output::install_hint().0));
            let _ = menu.append(&it.install);
            let _ = menu.append(&it.retry);
        } else if matches!(self.wifi, Wifi::Busy | Wifi::Failed(_)) {
            let _ = menu.append(&it.retry);
        }

        let play_into = Submenu::new("Play Into", !shape.loopbacks.is_empty());
        it.automatic.set_checked(!shape.manual);
        let _ = play_into.append(&it.automatic);
        let _ = play_into.append(&sep());
        it.outputs.clear();
        for c in &shape.loopbacks {
            let item = CheckMenuItem::new(c.label(), true, shape.output.as_deref() == Some(c.output.as_str()), None);
            it.outputs.push((item.id().clone(), c.output.clone()));
            let _ = play_into.append(&item);
        }
        let _ = menu.append(&play_into);

        if shape.paired {
            it.pair.set_text("Change Pairing Code…");
            let _ = menu.append(&it.pair);
            let _ = menu.append(&it.unpair);
        } else {
            it.pair.set_text("Pair with Cardputer…");
            let _ = menu.append(&it.pair);
        }
        let _ = menu.append(&sep());

        let _ = menu.append(&it.sound);
        it.login.set_checked(platform::login_item_enabled());
        let _ = menu.append(&it.login);
        let _ = menu.append(&sep());
        let _ = menu.append(&it.help);
        let _ = menu.append(&it.updates);
        let _ = menu.append(&it.quit);

        self.tray.set_menu(Some(Box::new(menu.clone())));
        self.menu = menu;
    }
}

impl Items {
    fn new() -> Items {
        Items {
            status: MenuItem::new("Getting ready…", false, None),
            hint: MenuItem::new("", false, None),
            level: MenuItem::new("", false, None),
            wifi: CheckMenuItem::new("Receive over Wi-Fi", true, true, None),
            automatic: CheckMenuItem::new("Automatic", true, true, None),
            outputs: Vec::new(),
            pair: MenuItem::new("Pair with Cardputer…", true, None),
            unpair: MenuItem::new("Forget Pairing Code", true, None),
            install: MenuItem::new("Install…", true, None),
            privacy: MenuItem::new("Open Privacy Settings…", true, None),
            retry: MenuItem::new("Try Again", true, None),
            sound: MenuItem::new("Open Sound Settings…", true, None),
            login: CheckMenuItem::new(
                if cfg!(target_os = "macos") { "Open at Login" } else { "Start with Windows" },
                true,
                false,
                None,
            ),
            help: MenuItem::new("Cardmic Help", true, None),
            updates: MenuItem::new(format!("Version {VERSION} · Check for Updates…"), true, None),
            quit: MenuItem::new("Quit Cardmic", true, None),
        }
    }
}

/// Ten segments over the 50 dB below full scale.
fn meter(peak: f32) -> String {
    let db = 20.0 * peak.max(1e-6).log10();
    let lit = (((db + 50.0) / 50.0).clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "▰".repeat(lit), "▱".repeat(10 - lit))
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

fn icon(glyph: Glyph) -> Icon {
    #[cfg(target_os = "macos")]
    let (rgba, size): (&[u8], u32) = match glyph {
        Glyph::Live => (include_bytes!("../assets/tray-mac-live-36.rgba"), 36),
        Glyph::Off => (include_bytes!("../assets/tray-mac-off-36.rgba"), 36),
        Glyph::Idle | Glyph::Warn => (include_bytes!("../assets/tray-mac-idle-36.rgba"), 36),
    };
    #[cfg(target_os = "windows")]
    let (rgba, size): (&[u8], u32) = match glyph {
        Glyph::Live => (include_bytes!("../assets/tray-win-live-32.rgba"), 32),
        Glyph::Off => (include_bytes!("../assets/tray-win-off-32.rgba"), 32),
        Glyph::Warn => (include_bytes!("../assets/tray-win-warn-32.rgba"), 32),
        Glyph::Idle => (include_bytes!("../assets/tray-win-idle-32.rgba"), 32),
    };
    Icon::from_rgba(rgba.to_vec(), size, size).expect("valid icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_spans_fifty_decibels() {
        assert_eq!(meter(0.0), "▱▱▱▱▱▱▱▱▱▱");
        assert_eq!(meter(1.0), "▰▰▰▰▰▰▰▰▰▰");
        assert_eq!(meter(0.0316), "▰▰▰▰▱▱▱▱▱▱"); // -30 dB
    }

    #[test]
    fn clip_keeps_short_text() {
        assert_eq!(clip("abc", 5), "abc");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
