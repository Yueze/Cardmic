//! Where `cardmic pair` keeps the pairing code.
//!
//! macOS: ~/Library/Application Support/cardmic/pairing
//! Windows: %APPDATA%\cardmic\pairing
//! Linux and others: $XDG_CONFIG_HOME/cardmic/pairing or ~/.config/cardmic/pairing

use cardmic_core::pairing::{normalize_code, Keys};
use std::path::PathBuf;

fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support/cardmic"))
    } else if cfg!(target_os = "windows") {
        env("APPDATA").map(|a| a.join("cardmic"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|c| c.join("cardmic"))
    }
}

pub fn path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("pairing"))
}

/// The stored code, if any. A corrupt file counts as none.
pub fn load() -> Option<String> {
    let text = std::fs::read_to_string(path()?).ok()?;
    normalize_code(text.trim()).ok()
}

pub fn load_keys() -> Option<Keys> {
    load().map(|c| Keys::derive(&c))
}

pub fn save(code: &str) -> Result<PathBuf, String> {
    let p = path().ok_or("cannot find a config directory (HOME/APPDATA unset)")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&p, format!("{code}\n")).map_err(|e| format!("{}: {e}", p.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(p)
}

pub fn remove() -> Result<bool, String> {
    let Some(p) = path() else { return Ok(false) };
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}
