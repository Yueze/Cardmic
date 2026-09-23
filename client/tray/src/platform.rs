//! The few things that differ between macOS and Windows: dialogs, opening
//! links, and starting at login.

pub use imp::*;

/// Permission to record audio input. Only macOS asks; the probe that tells a
/// loopback device from a dead end records from it for a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum MicAccess {
    Granted,
    NotAsked,
    Denied,
}

#[cfg(target_os = "macos")]
mod imp {
    use dispatch2::{DispatchQueue, MainThreadBound};
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSMenu, NSWindowOcclusionState};
    use objc2_service_management::{SMAppService, SMAppServiceStatus};
    use std::path::PathBuf;
    use std::process::Command;
    use tray_icon::menu::ContextMenu;

    pub fn alert(title: &str, message: &str) {
        let script = format!(
            "display alert \"{}\" message \"{}\" as informational buttons {{\"OK\"}} default button \"OK\"",
            escape(title),
            escape(message)
        );
        let _ = Command::new("osascript").args(["-e", &script]).status();
    }

    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    pub fn open_url(url: &str) {
        let _ = Command::new("open").arg(url).spawn();
    }

    pub fn copy_text(text: &str) {
        use std::io::Write;
        if let Ok(mut child) = Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }

    /// Started by the login item rather than by the user. macOS does not
    /// say, so: within a few minutes of the system starting.
    pub fn launched_at_login() -> bool {
        objc2_foundation::NSProcessInfo::processInfo().systemUptime() < 180.0
    }

    pub fn open_sound_settings() {
        open_url("x-apple.systempreferences:com.apple.Sound-Settings.extension?input");
    }

    /// Why a login item would not survive: the app is running from the disk
    /// image or from a quarantine copy, not from where it is installed.
    pub fn login_item_blocker() -> Option<&'static str> {
        let exe = std::env::current_exe().ok()?;
        let p = exe.to_string_lossy();
        if p.contains("/AppTranslocation/") || p.starts_with("/Volumes/") {
            Some("Move Cardmic to the Applications folder first, then open it from there.")
        } else {
            None
        }
    }

    // The app itself is the login item (System Settings > General > Login
    // Items > Open at Login), with its name and icon, like any other app.
    pub fn login_item_enabled() -> bool {
        unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
    }

    pub fn set_login_item(enable: bool) -> Result<(), String> {
        remove_legacy_agent();
        if enable {
            if let Some(why) = login_item_blocker() {
                return Err(why.into());
            }
        }
        let service = unsafe { SMAppService::mainAppService() };
        let result = unsafe {
            if enable {
                service.registerAndReturnError()
            } else {
                service.unregisterAndReturnError()
            }
        };
        result.map_err(|e| e.localizedDescription().to_string())
    }

    /// Early test builds registered a LaunchAgent, which macOS lists as a
    /// nameless "exec" background item. Replace it with the real login item.
    pub fn migrate_login_item() {
        let Some(path) = legacy_agent() else { return };
        if path.exists() {
            let _ = set_login_item(true);
        }
    }

    fn legacy_agent() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/LaunchAgents/io.github.yueze.cardmic.plist"))
    }

    fn remove_legacy_agent() {
        if let Some(p) = legacy_agent() {
            let _ = std::fs::remove_file(p);
        }
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    pub const PRIVACY_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";

    pub fn mic_access() -> super::MicAccess {
        use objc2::runtime::AnyClass;
        use objc2_foundation::NSString;
        let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return super::MicAccess::Granted };
        let audio = NSString::from_str("soun"); // AVMediaTypeAudio
        let status: isize = unsafe { objc2::msg_send![cls, authorizationStatusForMediaType: &*audio] };
        match status {
            0 => super::MicAccess::NotAsked,
            3 => super::MicAccess::Granted,
            _ => super::MicAccess::Denied, // restricted or denied
        }
    }

    /// Show the system prompt; `answered` runs on an arbitrary thread.
    pub fn request_mic_access(answered: impl Fn(bool) + Send + Sync + 'static) {
        use objc2::runtime::{AnyClass, Bool};
        use objc2_foundation::NSString;
        let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return answered(true) };
        let audio = NSString::from_str("soun");
        let block = block2::RcBlock::new(move |granted: Bool| answered(granted.as_bool()));
        unsafe {
            let _: () = objc2::msg_send![cls, requestAccessForMediaType: &*audio, completionHandler: &*block];
        }
    }

    /// Where the menu bar icon first appears. macOS keeps each app's icon
    /// position as a distance from the right edge of the screen, and puts new
    /// icons at the far left, which on a full menu bar is behind the camera
    /// notch. Start at the right end of the app icons instead. Only when no
    /// position is stored: once the user drags the icon (with Command), that
    /// place is kept.
    pub fn place_icon_near_the_right() {
        use objc2_foundation::{NSString, NSUserDefaults};
        let defaults = NSUserDefaults::standardUserDefaults();
        let key = NSString::from_str("NSStatusItem Preferred Position Item-0");
        if defaults.objectForKey(&key).is_none() {
            defaults.setDouble_forKey(200.0, &key);
        }
    }

    /// False when macOS has no room for the icon (a full menu bar, or the
    /// camera notch): it is then created but never drawn.
    pub fn icon_visible(tray: &tray_icon::TrayIcon) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return true };
        let Some(item) = tray.ns_status_item() else { return false };
        let Some(button) = item.button(mtm) else { return false };
        let Some(window) = button.window() else { return false };
        window.occlusionState().contains(NSWindowOcclusionState::Visible)
    }

    /// Open the menu at the pointer (after a click on the Dock icon), or from
    /// the menu bar icon when `at_pointer` is false and macOS draws it.
    ///
    /// Deferred to the main queue. A menu runs its own event loop, and
    /// starting one from inside tao's event handler deadlocks: tao locks its
    /// handler again from the nested loop.
    pub fn show_menu(tray: &tray_icon::TrayIcon, menu: &tray_icon::menu::Menu, at_pointer: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let ptr = menu.ns_menu() as *mut NSMenu;
        // SAFETY: a live NSMenu owned by `menu`; retained for the deferred call.
        let Some(ns_menu) = (unsafe { Retained::retain(ptr) }) else { return };
        let item = if !at_pointer && icon_visible(tray) { tray.ns_status_item() } else { None };
        let bound = MainThreadBound::new((ns_menu, item), mtm);
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let (menu, item) = bound.get(mtm);
            let app = NSApplication::sharedApplication(mtm);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            match item.as_ref().and_then(|i| i.button(mtm).map(|b| (i, b))) {
                Some((item, button)) => {
                    // What a click on the icon does.
                    item.setMenu(Some(menu));
                    // SAFETY: a nil sender is what AppKit itself passes.
                    unsafe { button.performClick(None) };
                    item.setMenu(None);
                }
                None => {
                    menu.popUpMenuPositioningItem_atLocation_inView(None, NSEvent::mouseLocation(), None);
                }
            }
        });
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE: &str = "Cardmic";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn open_url(url: &str) {
        let op = wide("open");
        let url = wide(url);
        unsafe {
            ShellExecuteW(null_mut(), op.as_ptr(), url.as_ptr(), null(), null(), SW_SHOWNORMAL);
        }
    }

    pub fn open_sound_settings() {
        open_url("ms-settings:sound");
    }

    pub fn alert(title: &str, message: &str) {
        let t = wide(title);
        let m = wide(message);
        unsafe {
            MessageBoxW(null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND);
        }
    }

    pub fn login_item_blocker() -> Option<&'static str> {
        None
    }

    pub fn migrate_login_item() {}

    pub fn launched_at_login() -> bool {
        std::env::args().any(|a| a == "--background")
    }

    pub fn copy_text(text: &str) {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
        const CF_UNICODETEXT: u32 = 13;
        let data = wide(text);
        unsafe {
            if OpenClipboard(null_mut()) == 0 {
                return;
            }
            EmptyClipboard();
            let mem = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
            if !mem.is_null() {
                let dst = GlobalLock(mem) as *mut u16;
                if !dst.is_null() {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
                    GlobalUnlock(mem);
                    SetClipboardData(CF_UNICODETEXT, mem); // the clipboard owns it now
                }
            }
            CloseClipboard();
        }
    }

    pub fn place_icon_near_the_right() {}

    pub const PRIVACY_URL: &str = "ms-settings:privacy-microphone";

    pub fn mic_access() -> super::MicAccess {
        super::MicAccess::Granted
    }

    pub fn request_mic_access(answered: impl Fn(bool) + Send + Sync + 'static) {
        answered(true)
    }

    pub fn login_item_enabled() -> bool {
        let key = wide(RUN_KEY);
        let name = wide(VALUE);
        unsafe {
            RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), null_mut())
                == ERROR_SUCCESS
        }
    }

    pub fn set_login_item(enable: bool) -> Result<(), String> {
        let key = wide(RUN_KEY);
        let name = wide(VALUE);
        let rc = unsafe {
            if enable {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                // --background: started at sign-in, so do not open the window.
                let data = wide(&format!("\"{}\" --background", exe.display()));
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    data.as_ptr() as *const c_void,
                    (data.len() * 2) as u32,
                )
            } else {
                match RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) {
                    ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                    rc => rc,
                }
            }
        };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("registry error {rc}"))
        }
    }
}
