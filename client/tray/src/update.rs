//! Keeping the app up to date, the way the firmware keeps itself up to date:
//! the latest GitHub release says what the newest version is.
//!
//! The release carries `cardmic-app-update.json`:
//!
//! ```json
//! {"version": "0.6.1",
//!  "macos":   {"url": ".../Cardmic-macOS.zip", "sha256": "...", "size": 4000000},
//!  "windows": {"url": ".../Cardmic-Windows-Setup.exe", "sha256": "...", "size": 3000000}}
//! ```
//!
//! A newer version is downloaded in the background and checked against the
//! SHA-256 (and, on macOS, its code signature), then installed: on macOS by
//! swapping the app bundle in place, on Windows by running the installer
//! silently. Either way the app then starts again by itself.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MANIFEST_URL: &str = "https://github.com/Yueze/Cardmic/releases/latest/download/cardmic-app-update.json";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Downloading { version: String, percent: u8 },
    /// Downloaded and verified; installs on the next [`Updater::install`].
    Ready { version: String },
    Failed(String),
}

struct Staged {
    version: String,
    path: PathBuf,
}

#[derive(Clone)]
pub struct Updater {
    state: Arc<Mutex<State>>,
    staged: Arc<Mutex<Option<Staged>>>,
    busy: Arc<AtomicBool>,
}

impl Updater {
    pub fn new() -> Updater {
        cleanup_old_bundles();
        Updater {
            state: Arc::new(Mutex::new(State::Idle)),
            staged: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    /// Check, and download a newer version if there is one. Runs in the
    /// background; `changed` is called on each change of state.
    pub fn check(&self, changed: impl Fn() + Send + 'static) {
        if matches!(self.state(), State::Ready { .. }) || self.busy.swap(true, Ordering::SeqCst) {
            return; // already downloaded, or a check is running
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("cardmic-update".into())
            .spawn(move || {
                me.set(State::Checking, &changed);
                let result = me.check_and_fetch(&changed);
                me.set(
                    match result {
                        Ok(Some(version)) => State::Ready { version },
                        Ok(None) => State::UpToDate,
                        Err(e) => State::Failed(e),
                    },
                    &changed,
                );
                me.busy.store(false, Ordering::SeqCst);
            })
            .ok();
    }

    fn set(&self, s: State, changed: &impl Fn()) {
        if let Ok(mut st) = self.state.lock() {
            *st = s;
        }
        changed();
    }

    fn check_and_fetch(&self, changed: &impl Fn()) -> Result<Option<String>, String> {
        let agent = agent();
        let url = std::env::var("CARDMIC_UPDATE_MANIFEST").unwrap_or_else(|_| MANIFEST_URL.to_string());
        let mut resp = agent.get(&url).call().map_err(|e| format!("no connection to the update server ({e})"))?;
        if resp.status() == 404 {
            return Ok(None); // nothing published yet
        }
        if !resp.status().is_success() {
            return Err(format!("update check failed (HTTP {})", resp.status().as_u16()));
        }
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let manifest: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("bad update manifest: {e}"))?;
        let version = manifest["version"].as_str().ok_or("update manifest has no version")?.to_string();
        if version_key(&version) <= version_key(VERSION) {
            return Ok(None);
        }
        let platform = if cfg!(target_os = "macos") { "macos" } else { "windows" };
        let asset = &manifest[platform];
        let asset_url = asset["url"].as_str().ok_or("no download for this system")?;
        let sha256 = asset["sha256"].as_str().ok_or("no checksum in the manifest")?.to_ascii_lowercase();

        let dir = cache_dir().ok_or("no cache directory")?.join(&version);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(asset_url.rsplit('/').next().unwrap_or("update"));
        self.download(&agent, asset_url, &file, &version, changed)?;
        if sha256_of(&file)? != sha256 {
            let _ = std::fs::remove_dir_all(&dir);
            return Err("download did not match its checksum".into());
        }
        let path = prepare(&file, &dir, &version)?;
        if let Ok(mut s) = self.staged.lock() {
            *s = Some(Staged { version: version.clone(), path });
        }
        Ok(Some(version))
    }

    fn download(&self, agent: &ureq::Agent, url: &str, to: &Path, version: &str, changed: &impl Fn()) -> Result<(), String> {
        let resp = agent.get(url).call().map_err(|e| format!("download failed ({e})"))?;
        if !resp.status().is_success() {
            return Err(format!("download failed (HTTP {})", resp.status().as_u16()));
        }
        let total = resp.body().content_length().unwrap_or(0);
        let mut reader = resp.into_body().into_reader();
        let mut out = std::fs::File::create(to).map_err(|e| e.to_string())?;
        let (mut done, mut shown) = (0u64, 0u8);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download interrupted ({e})"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            let percent = (done * 100).checked_div(total).map_or(0, |p| p.min(100) as u8);
            if percent != shown {
                shown = percent;
                self.set(State::Downloading { version: version.to_string(), percent }, changed);
            }
        }
        Ok(())
    }

    /// Install the downloaded version and start it. On success the caller
    /// must quit at once: the new version is waiting for this one to exit.
    /// `show`: open the window again once the new version is running.
    pub fn install(&self, show: bool) -> Result<(), String> {
        let staged = self.staged.lock().map_err(|e| e.to_string())?;
        let s = staged.as_ref().ok_or("no update downloaded")?;
        install(&s.path, &s.version, show)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .http_status_as_error(false)
            .user_agent(format!("Cardmic/{VERSION}"))
            .build(),
    )
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// "0.6.1" > "0.6.1-beta.2" > "0.6.0". Like the firmware's version check.
pub fn version_key(v: &str) -> (u64, u64, u64, u8) {
    let v = v.trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once('-') {
        Some((c, _)) => (c, true),
        None => (v, false),
    };
    let mut n = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (n.next().unwrap_or(0), n.next().unwrap_or(0), n.next().unwrap_or(0), if pre { 0 } else { 1 })
}

fn cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches/io.github.yueze.cardmic/update"))
    } else {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Cardmic").join("update"))
    }
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn prepare(zip: &Path, dir: &Path, version: &str) -> Result<PathBuf, String> {
    use std::process::Command;
    // ditto keeps the bundle's signature and attributes intact.
    let ok = Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(dir).status().map_err(|e| e.to_string())?;
    if !ok.success() {
        return Err("could not unpack the update".into());
    }
    let app = dir.join("Cardmic.app");
    let verified = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(&app)
        .status()
        .map_err(|e| e.to_string())?;
    if !verified.success() {
        return Err("the update's code signature is broken".into());
    }
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).map_err(|e| e.to_string())?;
    if !plist.contains("<string>io.github.yueze.cardmic</string>") || !plist.contains(&format!("<string>{version}</string>")) {
        return Err("the update is not the Cardmic version it claims to be".into());
    }
    Ok(app)
}

/// The .app this process runs from.
#[cfg(target_os = "macos")]
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?.to_path_buf();
    (app.extension().is_some_and(|e| e == "app")).then_some(app)
}

#[cfg(target_os = "macos")]
fn install(new_app: &Path, version: &str, show: bool) -> Result<(), String> {
    use std::process::Command;
    let app = running_bundle().ok_or("Cardmic is not running from an app bundle")?;
    let path = app.to_string_lossy().to_string();
    if path.contains("/AppTranslocation/") || path.starts_with("/Volumes/") {
        return Err("move Cardmic to the Applications folder to update it".into());
    }
    let parent = app.parent().ok_or("no folder around the app")?;
    let old = parent.join(format!(".Cardmic-{}-old.app", VERSION));
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(&app, &old).map_err(|e| format!("cannot replace {}: {e}", app.display()))?;
    if std::fs::rename(new_app, &app).is_err() {
        // Different volume: copy instead.
        let copied = Command::new("/usr/bin/ditto").arg(new_app).arg(&app).status().map(|s| s.success()).unwrap_or(false);
        if !copied {
            let _ = std::fs::rename(&old, &app); // put the old one back
            return Err("could not install the update".into());
        }
    }
    // Start the new version once this one has quit; opening it while this
    // process still runs would only bring this one to the front.
    let script = format!(
        "while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"{app}\" --args --updated{show}",
        pid = std::process::id(),
        app = path.replace('"', "\\\""),
        show = if show { " --show" } else { "" }
    );
    Command::new("/bin/sh").args(["-c", &script]).spawn().map_err(|e| e.to_string())?;
    crate::app::log(&format!("updated to {version}; restarting"));
    Ok(())
}

/// Leftovers of an earlier update, next to the app.
#[cfg(target_os = "macos")]
fn cleanup_old_bundles() {
    let Some(app) = running_bundle() else { return };
    let Some(parent) = app.parent() else { return };
    if let Ok(entries) = std::fs::read_dir(parent) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(".Cardmic-") && name.ends_with("-old.app") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
fn prepare(setup: &Path, _dir: &Path, _version: &str) -> Result<PathBuf, String> {
    Ok(setup.to_path_buf())
}

#[cfg(target_os = "windows")]
fn install(setup: &Path, version: &str, _show: bool) -> Result<(), String> {
    // The installer closes this app, installs over it and starts it again.
    std::process::Command::new(setup)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;
    crate::app::log(&format!("updating to {version}"));
    Ok(())
}

#[cfg(target_os = "windows")]
fn cleanup_old_bundles() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_like_the_firmware() {
        assert!(version_key("0.6.1") > version_key("0.6.0"));
        assert!(version_key("0.6.0") > version_key("0.6.0-beta.2"));
        assert!(version_key("v0.10.0") > version_key("0.9.9"));
        assert_eq!(version_key("0.6.0"), version_key("v0.6.0"));
    }
}
