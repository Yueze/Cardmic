// Renders the app's window for the README, from the repository root:
//
//   swiftc -O -o /tmp/render tools/screenshots/render.swift
//   /tmp/render STATE.json COLUMNS.txt END_INDEX OUT.png
//
// The window is styled like the app's (titled, transparent title bar, full-size
// content, 520 x 520, not resizable) and shows the app's own page
// (client/tray/ui) in a WKWebView. It is fed STATE.json, the state the app
// sends, plus spectrum columns in real time (see spectrum_columns in
// client/engine/examples), and captured with its system shadow at END_INDEX.
// Capturing a window of this process needs no screen-recording permission.
import Cocoa
import WebKit

let args = CommandLine.arguments
guard args.count >= 5 else {
    print("usage: render STATE.json COLUMNS.txt END_INDEX OUT.png")
    exit(1)
}
let ui = "client/tray/ui"
let page = try! String(contentsOfFile: ui + "/index.html", encoding: .utf8)
let fonts = try! String(contentsOfFile: ui + "/fonts.js", encoding: .utf8)
// @VERSION@ in the state is the client's version, from client/Cargo.toml.
let cargo = try! String(contentsOfFile: "client/Cargo.toml", encoding: .utf8)
let version = cargo.split(separator: "\n").first { $0.hasPrefix("version = ") }!
    .split(separator: "\"")[1]
let stateJSON = try! String(contentsOfFile: args[1], encoding: .utf8)
    .trimmingCharacters(in: .whitespacesAndNewlines)
    .replacingOccurrences(of: "@VERSION@", with: String(version))
let cols = try! String(contentsOfFile: args[2], encoding: .utf8).split(separator: "\n").map(String.init)
let endIndex = Int(args[3])!
let out = args[4]

typealias CreateImage = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?

final class App: NSObject, NSApplicationDelegate, WKNavigationDelegate {
    var win: NSWindow!
    var web: WKWebView!
    var index = 0
    var timer: Timer?

    func applicationDidFinishLaunching(_ n: Notification) {
        let cfg = WKWebViewConfiguration()
        let uc = cfg.userContentController
        // A stand-in for the app's IPC, so the page does not start its demo mode.
        uc.addUserScript(WKUserScript(source: "window.ipc = { postMessage() {} };", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        uc.addUserScript(WKUserScript(source: fonts, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 520),
                       styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
                       backing: .buffered, defer: false)
        win.titlebarAppearsTransparent = true
        win.titleVisibility = .hidden
        win.title = "Cardmic"
        win.backgroundColor = .black
        web = WKWebView(frame: win.contentView!.bounds, configuration: cfg)
        web.autoresizingMask = [.width, .height]
        web.navigationDelegate = self
        win.contentView!.addSubview(web)
        web.loadHTMLString(page, baseURL: nil)
        win.center()
        win.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func webView(_ w: WKWebView, didFinish n: WKNavigation!) {
        // 3 columns every 30 ms, as the app sends them; start 4 s early so the
        // spectrogram is full, on the same phase as END_INDEX.
        index = max(0, endIndex - 400) / 3 * 3 + endIndex % 3
        timer = Timer.scheduledTimer(withTimeInterval: 0.03, repeats: true) { [weak self] _ in self?.tick() }
    }

    func tick() {
        // Keep streaming through the capture, so the newest columns are drawn.
        if index == endIndex { capture() }
        guard index + 3 <= cols.count else { return }
        let chunk = cols[index..<index + 3]
        index += 3
        let level = Double(Int(chunk.last!.suffix(2), radix: 16)!) / 255.0
        let state = String(stateJSON.dropLast()) + ",\"level\":\(level),\"spec\":\"\(chunk.joined())\"}"
        web.evaluateJavaScript("window.cardmic && cardmic.update(\(state))")
    }

    func capture() {
        let cg = dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", RTLD_NOW)
        guard let sym = dlsym(cg, "CGWindowListCreateImage") else { fatalError("no CGWindowListCreateImage") }
        let create = unsafeBitCast(sym, to: CreateImage.self)
        // kCGWindowListOptionIncludingWindow, kCGWindowImageBestResolution; the shadow is included.
        guard let img = create(.null, 1 << 3, UInt32(win.windowNumber), 1 << 3)?.takeRetainedValue() else {
            fatalError("window capture failed")
        }
        let png = NSBitmapImageRep(cgImage: img).representation(using: .png, properties: [:])!
        try! png.write(to: URL(fileURLWithPath: out))
        print("\(img.width)x\(img.height) -> \(out)")
        NSApp.terminate(nil)
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = App()
app.delegate = delegate
app.run()
