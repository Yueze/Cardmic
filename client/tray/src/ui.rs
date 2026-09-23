//! The Cardmic window: a small page (ui/index.html) in the system web view.
//!
//! Rust pushes the state as JSON a few times a second while the window is
//! open; the page sends back one-line commands, `name` or `name<TAB>arg`.

use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use wry::{WebView, WebViewBuilder};

const PAGE: &str = include_str!("../ui/index.html");
/// The device's bitmap fonts (see ui/make_fonts.py), loaded before the page.
const FONTS: &str = include_str!("../ui/fonts.js");

pub struct Ui {
    pub window: Window,
    webview: WebView,
}

/// A command from the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Ready,
    Wifi(bool),
    /// `None` = pick automatically.
    Output(Option<String>),
    Pair(String),
    Unpair,
    Login(bool),
    Open(String),
    Retry,
    Copy(String),
    Drag,
    /// A script error on the page, for the log.
    Log(String),
}

impl Command {
    pub fn parse(msg: &str) -> Option<Command> {
        let (name, arg) = msg.split_once('\t').unwrap_or((msg, ""));
        Some(match name {
            "ready" => Command::Ready,
            "wifi" => Command::Wifi(arg == "1"),
            "output" => Command::Output(Some(arg.to_string()).filter(|a| !a.is_empty())),
            "pair" => Command::Pair(arg.to_string()),
            "unpair" => Command::Unpair,
            "login" => Command::Login(arg == "1"),
            "open" => Command::Open(arg.to_string()),
            "retry" => Command::Retry,
            "copy" => Command::Copy(arg.to_string()),
            "drag" => Command::Drag,
            "log" => Command::Log(arg.to_string()),
            _ => return None,
        })
    }
}

impl Ui {
    pub fn new<T: 'static + Send>(
        target: &EventLoopWindowTarget<T>,
        proxy: EventLoopProxy<T>,
        wrap: fn(String) -> T,
    ) -> Result<Ui, String> {
        #[allow(unused_mut)]
        let mut builder = WindowBuilder::new()
            .with_title("Cardmic")
            .with_inner_size(LogicalSize::new(520.0, 520.0))
            .with_resizable(false)
            .with_visible(false);
        #[cfg(target_os = "macos")]
        {
            use tao::platform::macos::WindowBuilderExtMacOS;
            // The page's header is the title bar, traffic lights included.
            builder = builder.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true);
        }
        let window = builder.build(target).map_err(|e| e.to_string())?;
        let webview = WebViewBuilder::new()
            .with_initialization_script(FONTS)
            .with_background_color((0, 0, 0, 255))
            .with_html(PAGE)
            .with_accept_first_mouse(true)
            .with_ipc_handler(move |req: wry::http::Request<String>| {
                let _ = proxy.send_event(wrap(req.body().clone()));
            })
            .build(&window)
            .map_err(|e| e.to_string())?;
        Ok(Ui { window, webview })
    }

    pub fn show(&self) {
        self.window.set_visible(true);
        self.window.set_focus();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn visible(&self) -> bool {
        self.window.is_visible()
    }

    pub fn push(&self, state_json: &str) {
        let _ = self.webview.evaluate_script(&format!("window.cardmic && cardmic.update({state_json})"));
    }

    pub fn pair_result(&self, ok: bool, message: &str) {
        let _ = self.webview.evaluate_script(&format!(
            "window.cardmic && cardmic.pairResult({ok}, {})",
            json_str(message)
        ));
    }

    pub fn focus_pairing(&self) {
        let _ = self.webview.evaluate_script("window.cardmic && cardmic.focusPair()");
    }
}

/// A JSON string literal, safe to embed in a script.
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"), // never close a script tag
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Builds a flat JSON object.
#[derive(Default)]
pub struct Json(Vec<String>);

impl Json {
    pub fn str(mut self, k: &str, v: &str) -> Self {
        self.0.push(format!("{}:{}", json_str(k), json_str(v)));
        self
    }
    pub fn opt(mut self, k: &str, v: Option<&str>) -> Self {
        self.0.push(format!("{}:{}", json_str(k), v.map(json_str).unwrap_or_else(|| "null".into())));
        self
    }
    pub fn bool(mut self, k: &str, v: bool) -> Self {
        self.0.push(format!("{}:{v}", json_str(k)));
        self
    }
    pub fn num(mut self, k: &str, v: f64) -> Self {
        let v = if v.is_finite() { v } else { 0.0 };
        self.0.push(format!("{}:{v}", json_str(k)));
        self
    }
    /// `[{"name":..,"label":..}, ...]`
    pub fn pairs(mut self, k: &str, items: &[(String, String)]) -> Self {
        let list: Vec<String> = items
            .iter()
            .map(|(n, l)| format!("{{\"name\":{},\"label\":{}}}", json_str(n), json_str(l)))
            .collect();
        self.0.push(format!("{}:[{}]", json_str(k), list.join(",")));
        self
    }
    pub fn build(self) -> String {
        format!("{{{}}}", self.0.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse() {
        assert_eq!(Command::parse("ready"), Some(Command::Ready));
        assert_eq!(Command::parse("wifi\t1"), Some(Command::Wifi(true)));
        assert_eq!(Command::parse("output\t"), Some(Command::Output(None)));
        assert_eq!(Command::parse("output\tBlackHole 2ch"), Some(Command::Output(Some("BlackHole 2ch".into()))));
        assert_eq!(Command::parse("pair\t7K2M-9QXB-4TPA"), Some(Command::Pair("7K2M-9QXB-4TPA".into())));
        assert_eq!(Command::parse("nonsense"), None);
    }

    #[test]
    fn strings_are_escaped_for_scripts() {
        assert_eq!(json_str("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(json_str("</script>"), "\"\\u003c/script>\"");
        assert_eq!(json_str("扬声器\n"), "\"扬声器\\u000a\"");
    }

    #[test]
    fn json_objects() {
        let j = Json::default().str("a", "x").bool("b", true).num("c", 1.5).opt("d", None).build();
        assert_eq!(j, r#"{"a":"x","b":true,"c":1.5,"d":null}"#);
    }
}
