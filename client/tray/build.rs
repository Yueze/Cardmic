//! Windows: embed the app icon (resource 1, also used by the pairing
//! prompt) and a manifest for modern controls and per-monitor DPI.

fn main() {
    println!("cargo:rerun-if-changed=assets/Cardmic.ico");
    println!("cargo:rerun-if-changed=windows/Cardmic.manifest");
    // Only when building on Windows for Windows: the resource compiler is
    // part of the Windows SDK.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && cfg!(windows) {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("assets/Cardmic.ico", "1")
            .set_manifest_file("windows/Cardmic.manifest")
            .set("ProductName", "Cardmic")
            .set("FileDescription", "Cardmic")
            .set("CompanyName", "The Cardmic Authors")
            .set("LegalCopyright", "Copyright © 2026 The Cardmic Authors. Apache License 2.0.");
        if let Err(e) = res.compile() {
            panic!("embedding Windows resources failed: {e}");
        }
    }
}
