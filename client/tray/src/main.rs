//! Cardmic menu bar app (macOS) and tray app (Windows).
//!
//! A microphone icon that finds the Cardputer by itself and plays its Wi-Fi
//! audio into a loopback device, so any app can use it as a microphone. The
//! menu says what to pick in that app, and handles pairing. It runs the same
//! engine as `cardmic run`.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod app;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod devices;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod settings;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod ui;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod update;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod usb;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    if std::env::args().any(|a| a == "--diagnose") {
        app::diagnose();
        return;
    }
    app::run();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("The Cardmic menu bar app is for macOS and Windows. On Linux, use `cardmic run`.");
    std::process::exit(1);
}
