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

    /// Ask for the pairing code. `None` if cancelled.
    pub fn ask_code(message: &str) -> Option<String> {
        let script = format!(
            "text returned of (display dialog \"{}\" default answer \"\" with title \"Pair with Cardputer\" \
             buttons {{\"Cancel\", \"Pair\"}} default button \"Pair\" cancel button \"Cancel\" with icon note)",
            escape(message)
        );
        let out = Command::new("osascript").args(["-e", &script]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

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

    /// False when macOS has no room for the icon (a full menu bar, or the
    /// camera notch): it is then created but never drawn.
    pub fn icon_visible(tray: &tray_icon::TrayIcon) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return true };
        let Some(item) = tray.ns_status_item() else { return false };
        let Some(button) = item.button(mtm) else { return false };
        let Some(window) = button.window() else { return false };
        window.occlusionState().contains(NSWindowOcclusionState::Visible)
    }

    /// Open the menu: from the icon if macOS draws it, else at the pointer.
    ///
    /// Deferred to the main queue. A menu runs its own event loop, and
    /// starting one from inside tao's event handler deadlocks: tao locks its
    /// handler again from the nested loop.
    pub fn show_menu(tray: &tray_icon::TrayIcon, menu: &tray_icon::menu::Menu) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let ptr = menu.ns_menu() as *mut NSMenu;
        // SAFETY: a live NSMenu owned by `menu`; retained for the deferred call.
        let Some(ns_menu) = (unsafe { Retained::retain(ptr) }) else { return };
        let item = if icon_visible(tray) { tray.ns_status_item() } else { None };
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
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
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
                let data = wide(&format!("\"{}\"", exe.display()));
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

    // ---- a small modal text prompt; Win32 has no built-in one ----

    const ID_EDIT: i32 = 100;

    struct Prompt {
        edit: HWND,
        result: Option<String>,
        done: bool,
    }

    /// Ask for the pairing code. `None` if cancelled.
    pub fn ask_code(message: &str) -> Option<String> {
        unsafe {
            let hinst = GetModuleHandleW(null());
            let class = wide("CardmicPrompt");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(prompt_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: LoadIconW(hinst, 1 as *const u16),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc); // fails harmlessly if already registered

            let scale = |v: i32| v * GetDpiForSystem() as i32 / 96;
            let (w, h) = (scale(420), scale(170));
            let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 3;
            // Shared with prompt_proc through GWLP_USERDATA; only ever touched
            // through this pointer, and freed after the window is gone.
            let state = Box::into_raw(Box::new(Prompt { edit: null_mut(), result: None, done: false }));
            let title = wide("Pair with Cardputer");
            let hwnd = CreateWindowExW(
                WS_EX_DLGMODALFRAME | WS_EX_TOPMOST,
                class.as_ptr(),
                title.as_ptr(),
                WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                x,
                y,
                w,
                h,
                null_mut(),
                null_mut(),
                hinst,
                state as *const c_void,
            );
            if hwnd.is_null() {
                drop(Box::from_raw(state));
                return None;
            }

            let mut ncm: NONCLIENTMETRICSW = std::mem::zeroed();
            ncm.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
            SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut c_void, 0);
            let font = CreateFontIndirectW(&ncm.lfMessageFont);

            let child = |class: &str, text: &str, style: u32, ex: u32, x: i32, y: i32, w: i32, h: i32, id: i32| {
                let c = wide(class);
                let t = wide(text);
                let hw = CreateWindowExW(
                    ex,
                    c.as_ptr(),
                    t.as_ptr(),
                    WS_CHILD | WS_VISIBLE | style,
                    scale(x),
                    scale(y),
                    scale(w),
                    scale(h),
                    hwnd,
                    id as usize as HMENU,
                    hinst,
                    null(),
                );
                SendMessageW(hw, WM_SETFONT, font as WPARAM, 1);
                hw
            };
            child("STATIC", message, 0, 0, 16, 14, 380, 36, 0);
            (*state).edit = child(
                "EDIT",
                "",
                WS_TABSTOP | ES_UPPERCASE as u32 | ES_AUTOHSCROLL as u32,
                WS_EX_CLIENTEDGE,
                16,
                54,
                372,
                24,
                ID_EDIT,
            );
            child("BUTTON", "Pair", WS_TABSTOP | BS_DEFPUSHBUTTON as u32, 0, 212, 92, 84, 26, IDOK);
            child("BUTTON", "Cancel", WS_TABSTOP | BS_PUSHBUTTON as u32, 0, 304, 92, 84, 26, IDCANCEL);

            SetForegroundWindow(hwnd);
            SetFocus((*state).edit);
            let mut msg: MSG = std::mem::zeroed();
            while !(*state).done && GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if IsDialogMessageW(hwnd, &msg) == 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            DeleteObject(font);
            Box::from_raw(state).result
        }
    }

    unsafe extern "system" fn prompt_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let cs = lparam as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Prompt;
            match msg {
                WM_COMMAND if !state.is_null() => {
                    let id = (wparam & 0xffff) as i32;
                    if id == IDOK {
                        let mut buf = [0u16; 64];
                        let n = GetWindowTextW((*state).edit, buf.as_mut_ptr(), buf.len() as i32);
                        (*state).result = Some(String::from_utf16_lossy(&buf[..n.max(0) as usize]));
                        (*state).done = true;
                        DestroyWindow(hwnd);
                    } else if id == IDCANCEL {
                        (*state).done = true;
                        DestroyWindow(hwnd);
                    }
                    0
                }
                WM_CLOSE => {
                    if !state.is_null() {
                        (*state).done = true;
                    }
                    DestroyWindow(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}
