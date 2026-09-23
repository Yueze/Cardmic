//! The app's few preferences, as `key=value` lines next to the pairing code.

use cardmic_engine::pairing_store::config_dir;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Receive over Wi-Fi.
    pub wifi: bool,
    /// The loopback device last played into.
    pub output: Option<String>,
    /// `output` was picked by the user, not detected.
    pub output_manual: bool,
    /// The login item has been offered once (on first launch).
    pub login_item_offered: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { wifi: true, output: None, output_manual: false, login_item_offered: false }
    }
}

fn path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings"))
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "wifi" => s.wifi = v.trim() != "off",
                "output" if !v.trim().is_empty() => s.output = Some(v.trim().to_string()),
                "output_manual" => s.output_manual = v.trim() == "1",
                "login_item_offered" => s.login_item_offered = v.trim() == "1",
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = format!(
            "wifi={}\noutput={}\noutput_manual={}\nlogin_item_offered={}\n",
            if self.wifi { "on" } else { "off" },
            self.output.as_deref().unwrap_or(""),
            u8::from(self.output_manual),
            u8::from(self.login_item_offered),
        );
        let _ = std::fs::write(p, text);
    }
}
