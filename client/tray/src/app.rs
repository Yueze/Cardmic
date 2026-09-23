//! The menu bar app: state, menu, icon.

use crate::devices::{self, Devices};
use crate::platform::{self, MicAccess};
use crate::settings::Settings;
use crate::ui::{Command, Json, Ui};
use cardmic_audio::Candidate;
use cardmic_core::pairing::normalize_code;
use cardmic_engine::{output, pairing_store, Engine, Link, Options, StartError, Status};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
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
/// Also the level meter's frame rate while the window is open.
const TICK: Duration = Duration::from_millis(100);

enum UserEvent {
    Menu(MenuEvent),
    Devices(Devices),
    Started(u64, Result<Engine, Failure>),
    /// The engine reported something; refresh now rather than at the next tick.
    Engine,
    /// A command from the window's page.
    Ui(String),
    /// A click on the tray icon (Windows: left click opens the window).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Tray(tray_icon::TrayIconEvent),
    /// The answer to the microphone permission prompt.
    MicAnswered(bool),
    /// Cardmic was opened again while running: show the window.
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
    open: MenuItem,
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
    first_launch: bool,
    /// This computer has a pairing code (cached; read on start and change).
    paired: bool,
    ui: Option<Ui>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    logged_icon: bool,
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

    // A regular app on macOS: a Dock icon (three dots) that opens the menu
    // when clicked, besides the menu bar icon, which a full menu bar or the
    // camera notch can hide.
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |e| {
            let _ = proxy.send_event(UserEvent::Menu(e));
        }
    }));

    let mut instance = Some(instance);
    let mut app: Option<App> = None;
    #[cfg(target_os = "windows")]
    tray_icon::TrayIconEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |e| {
            let _ = proxy.send_event(UserEvent::Tray(e));
        }
    }));

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        match event {
            // On macOS the tray icon must be created once the loop is running.
            Event::NewEvents(StartCause::Init) => {
                if let Some(l) = instance.take() {
                    listen_for_reopen(l, proxy.clone());
                    app = Some(App::new(proxy.clone(), target));
                }
            }
            // The Dock icon was clicked, or Cardmic opened again from Finder,
            // Launchpad or Spotlight.
            Event::Reopen { .. } => {
                if let Some(a) = app.as_mut() {
                    a.show_window();
                }
            }
            // Closing the window keeps Cardmic running.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if let Some(a) = app.as_mut() {
                    a.hide_window();
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

/// The menu bar a regular macOS app shows while it is in front.
#[cfg(target_os = "macos")]
fn app_menu_bar() {
    use tray_icon::menu::AboutMetadata;
    let bar = Menu::new();
    let about = AboutMetadata {
        name: Some("Cardmic".into()),
        version: Some(VERSION.into()),
        website: Some("https://github.com/Yueze/Cardmic".into()),
        license: Some("MIT".into()),
        ..Default::default()
    };
    let app = Submenu::with_items(
        "Cardmic",
        true,
        &[
            &PredefinedMenuItem::about(None, Some(about)),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    );
    if let Ok(app) = app {
        let _ = bar.append(&app);
    }
    bar.init_for_nsapp();
    std::mem::forget(bar); // lives as long as the app
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
    fn new(proxy: EventLoopProxy<UserEvent>, target: &EventLoopWindowTarget<UserEvent>) -> App {
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

        #[cfg(target_os = "macos")]
        app_menu_bar();

        let items = Items::new();
        let menu = Menu::new();
        platform::place_icon_near_the_right();
        let tray = TrayIconBuilder::new()
            .with_tooltip("Cardmic")
            .with_icon(icon(Glyph::Idle))
            .with_icon_as_template(cfg!(target_os = "macos"))
            .with_menu(Box::new(menu.clone()))
            // Windows: left click opens the window, right click the menu.
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()
            .expect("create the menu bar icon");
        let ui = match Ui::new(target, proxy.clone(), UserEvent::Ui) {
            Ok(ui) => Some(ui),
            Err(e) => {
                log(&format!("window unavailable: {e}"));
                None
            }
        };

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
            first_launch,
            paired: pairing_store::load().is_some(),
            ui,
            logged_icon: false,
            menu,
        };
        if app.settings.wifi {
            app.start();
        }
        app.refresh();
        // Opened by the user (not at login): show the window.
        if !first_launch && !platform::launched_at_login() {
            app.show_window();
        }
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
            UserEvent::Show => self.show_window(),
            UserEvent::Ui(msg) => {
                if let Some(cmd) = Command::parse(&msg) {
                    self.command(cmd);
                }
            }
            UserEvent::Tray(e) => {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = e
                {
                    if self.ui.as_ref().is_some_and(|u| u.visible()) {
                        self.hide_window();
                    } else {
                        self.show_window();
                    }
                }
            }
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
            let on = it.wifi.is_checked();
            self.set_wifi(on);
        } else if id == *it.automatic.id() {
            self.set_output(None);
        } else if let Some((_, name)) = it.outputs.iter().find(|(oid, _)| *oid == id) {
            let name = name.clone();
            self.set_output(Some(name));
        } else if id == *it.open.id() {
            self.show_window();
        } else if id == *it.pair.id() {
            self.show_window();
            if let Some(ui) = &self.ui {
                ui.focus_pairing();
            }
        } else if id == *it.unpair.id() {
            self.unpair();
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
            self.set_login(want);
        } else if id == *it.help.id() {
            platform::open_url(HELP);
        } else if id == *it.updates.id() {
            platform::open_url(RELEASES);
        }
        false
    }

    // ------------------------------------------------------------ actions
    // Shared by the menu and the window.

    fn set_wifi(&mut self, on: bool) {
        self.settings.wifi = on;
        self.settings.save();
        self.shape = None; // re-sync the check mark
        if on {
            self.start();
        } else {
            self.stop();
        }
    }

    /// `None` = pick automatically.
    fn set_output(&mut self, name: Option<String>) {
        match name {
            Some(name) => {
                self.settings.output = Some(name);
                self.settings.output_manual = true;
            }
            None => self.settings.output_manual = false,
        }
        self.settings.save();
        self.shape = None;
        if self.settings.wifi {
            self.start();
        }
    }

    fn set_login(&mut self, on: bool) {
        if let Err(e) = platform::set_login_item(on) {
            log(&format!("login item: {e}"));
            platform::alert("Could not change Open at Login", &e);
        }
        self.shape = None;
    }

    fn pair(&mut self, text: &str) -> Result<(), String> {
        let code = normalize_code(text).map_err(|e| format!("{e}. The code looks like 7K2M-9QXB-4TPA."))?;
        pairing_store::save(&code)?;
        self.paired = true;
        if self.settings.wifi {
            self.start();
        }
        Ok(())
    }

    fn unpair(&mut self) {
        match pairing_store::remove() {
            Ok(_) => {
                self.paired = false;
                if self.settings.wifi {
                    self.start();
                }
            }
            Err(e) => platform::alert("Could not forget the pairing code", &e),
        }
    }

    fn command(&mut self, cmd: Command) {
        match cmd {
            Command::Ready => {}
            Command::Wifi(on) => self.set_wifi(on),
            Command::Output(name) => self.set_output(name),
            Command::Pair(text) => {
                let result = self.pair(&text);
                if let Some(ui) = &self.ui {
                    ui.pair_result(result.is_ok(), result.as_ref().err().map(String::as_str).unwrap_or(""));
                }
            }
            Command::Unpair => self.unpair(),
            Command::Login(on) => self.set_login(on),
            Command::Open(what) => match what.as_str() {
                "help" => platform::open_url(HELP),
                "updates" => platform::open_url(RELEASES),
                "install" => platform::open_url(output::install_hint().1),
                "privacy" => platform::open_url(platform::PRIVACY_URL),
                "sound" => platform::open_sound_settings(),
                _ => {}
            },
            Command::Retry => self.set_wifi(true),
            Command::Copy(text) => platform::copy_text(&text),
            Command::Log(msg) => log(&format!("window: {msg}")),
            Command::Drag => {
                if let Some(ui) = &self.ui {
                    let _ = ui.window.drag_window();
                }
            }
        }
        self.push_state();
    }

    fn show_window(&mut self) {
        match &self.ui {
            Some(ui) => {
                ui.show();
                self.push_state();
            }
            // No web view (very old Windows without WebView2): the menu will do.
            None => self.show_menu(true),
        }
    }

    fn hide_window(&mut self) {
        if let Some(ui) = &self.ui {
            ui.hide();
        }
    }

    /// Send the state, and new spectrogram columns, to the window if it is
    /// open. Columns that pile up while it is closed are dropped.
    fn push_state(&self) {
        let columns = match &self.wifi {
            Wifi::Running(engine) => engine.take_spectrum(),
            _ => Vec::new(),
        };
        let Some(ui) = self.ui.as_ref().filter(|u| u.visible()) else { return };
        let mut json = self.state_json();
        if !columns.is_empty() {
            let mut hex = String::with_capacity(columns.len() * columns[0].len() * 2);
            for col in &columns {
                for b in col {
                    use std::fmt::Write as _;
                    let _ = write!(hex, "{b:02x}");
                }
            }
            json.pop(); // the closing brace
            json.push_str(&format!(",\"spec\":\"{hex}\"}}"));
        }
        ui.push(&json);
    }

    fn state_json(&self) -> String {
        let status = self.status();
        let s = status.as_ref();
        let connected = s.and_then(|s| match s.link {
            Link::Connected { addr, encrypted } => Some((addr, encrypted)),
            Link::Searching => None,
        });
        let phase = match (&self.wifi, s) {
            (Wifi::Off, _) => "off",
            (Wifi::Starting, _) => "starting",
            (Wifi::NoLoopback, _) => "no_loopback",
            (Wifi::AskingMic, _) => "asking_mic",
            (Wifi::MicDenied, _) => "mic_denied",
            (Wifi::Busy, _) => "busy",
            (Wifi::Failed(_), _) => "failed",
            (Wifi::Running(_), _) if connected.is_some() => "connected",
            (Wifi::Running(_), Some(s)) if s.auth_failures > 0 => "wrong_code",
            (Wifi::Running(_), _) => "searching",
        };
        let (mut title, mut detail, _) = self.describe(s);
        if let Some((addr, _)) = connected {
            title = "Connected".into();
            detail = format!("Cardputer at {}", addr.ip());
        }
        if phase == "off" && self.devices.usb_mic.is_some() {
            title = "Connected over USB".into();
            detail = "Wi-Fi receiving is off".into();
        }
        let loss = s.map_or(0.0, |s| {
            let total = s.packets + s.concealed;
            if total == 0 { 0.0 } else { s.concealed as f64 * 100.0 / total as f64 }
        });
        let outputs: Vec<(String, String)> =
            self.devices.loopbacks.iter().map(|c| (c.output.clone(), c.label())).collect();
        let output = s.map(|s| s.output.clone()).or_else(|| self.settings.output.clone());
        Json::default()
            .str("platform", if cfg!(target_os = "macos") { "mac" } else { "win" })
            .str("version", VERSION)
            .bool("wifi", self.settings.wifi)
            .bool("login", platform::login_item_enabled())
            .str("phase", phase)
            .str("title", &title)
            .str("detail", &detail)
            .opt("device", connected.map(|(a, _)| a.ip().to_string()).as_deref())
            .bool("encrypted", connected.is_some_and(|(_, e)| e))
            .opt("mic", s.map(|s| s.mic.as_str()))
            .opt("output", output.as_deref())
            .bool("manual", self.settings.output_manual)
            .pairs("outputs", &outputs)
            .num("level", s.map_or(0.0, |s| s.level as f64))
            .num("buffer_ms", s.map_or(0.0, |s| s.target_ms))
            .num("loss_pct", loss)
            .opt("usb", self.devices.usb_mic.as_deref())
            .bool("paired", self.paired)
            .str("install_name", output::install_hint().0)
            .build()
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
        #[cfg(target_os = "macos")]
        if !self.logged_icon && self.shape.is_some() {
            self.logged_icon = true;
            log(&format!("menu bar icon visible: {}", platform::icon_visible(&self.tray)));
        }
        if self.first_launch && self.shape.is_some() {
            // Show what Cardmic is, once.
            self.first_launch = false;
            self.show_window();
        }
        self.refresh();
        self.push_state();
    }

    // ------------------------------------------------------------ view

    /// Open the menu: from the icon if it can be seen, else at the pointer.
    fn show_menu(&mut self, at_pointer: bool) {
        self.refresh();
        #[cfg(target_os = "macos")]
        platform::show_menu(&self.tray, &self.menu, at_pointer);
        #[cfg(target_os = "windows")]
        {
            let _ = at_pointer; // the tray menu always opens at the pointer
            self.tray.show_menu();
        }
    }

    fn refresh(&mut self) {
        let status = self.status();
        let connected = matches!(status.as_ref().map(|s| s.link), Some(Link::Connected { .. }));
        let shape = Shape {
            connected,
            usb: self.devices.usb_mic.clone(),
            needs_loopback: matches!(self.wifi, Wifi::NoLoopback),
            mic_denied: matches!(self.wifi, Wifi::MicDenied),
            paired: self.paired,
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

        let _ = menu.append(&it.open);
        let _ = menu.append(&sep());
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
            open: MenuItem::new("Open Cardmic", true, None),
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
