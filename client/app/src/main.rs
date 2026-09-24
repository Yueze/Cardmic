//! Cardmic desktop client.
//!
//! Receives the Cardputer's wireless microphone stream and plays it into a
//! loopback audio device, where every other app sees it as a microphone.

mod png;
mod screenshot;

use cardmic_audio::probe::{self, Verdict};
use cardmic_audio::Candidate;
use cardmic_core::pairing::{format_code, normalize_code};
use cardmic_engine::output::{self, install_hint};
use cardmic_engine::{pairing_store, Engine, Event, Link, Options};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
cardmic — use an M5Stack Cardputer (ADV or original) as a wireless microphone

USAGE:
    cardmic run [--output NAME] [--device ADDR:PORT]...
    cardmic doctor
    cardmic devices
    cardmic screenshot DEVICE_IP [--page NAME] [-o FILE] [--scale N]
    cardmic pair CODE
    cardmic unpair
    cardmic --version

COMMANDS:
    run       Receive the Cardputer's audio and play it into a loopback device.
              Then choose that device as the microphone in your app.
    doctor    Find which audio devices on this machine genuinely loop back.
    devices   List audio devices.
    pair      Pair with a Cardputer that requires pairing, using the code
              shown in Cardmic > Settings > Pairing. Audio is then encrypted.
    unpair    Forget the stored pairing code.
    screenshot
              Save a pixel-exact PNG of the device screen (Cardmic must be
              open and on Wi-Fi). --page: main, settings, info or about.

OPTIONS:
    --output NAME        Loopback device to play into. Default: auto-detect.
    --device ADDR:PORT   Also send discovery straight to this address, for
                         networks that block broadcast. Repeatable.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("doctor") => doctor(),
        Some("devices") => devices(),
        Some("screenshot") => screenshot::run(&args[1..]),
        Some("pair") => pair(&args[1..]),
        Some("unpair") => unpair(),
        Some("-V" | "--version") => {
            println!("cardmic {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("-h" | "--help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------- devices

fn devices() -> Result<(), String> {
    let host = cpal::default_host();
    let candidates = cardmic_audio::loopback_candidates(&host);
    println!("Devices on both the output and input side (loopback candidates):");
    if candidates.is_empty() {
        println!("  (none)");
    }
    for c in &candidates {
        println!("  {}", c.label());
    }
    println!("\nRun `cardmic doctor` to find which of these actually loop back.");
    Ok(())
}

// ---------------------------------------------------------------- doctor

fn doctor() -> Result<(), String> {
    let host = cpal::default_host();
    let candidates = cardmic_audio::loopback_candidates(&host);
    if candidates.is_empty() {
        println!("No loopback candidates found.\n");
        print_install_hint();
        return Err("no loopback device available".into());
    }

    println!("Probing {} candidate device(s) with a 997 Hz tone...\n", candidates.len());
    println!("  {:<36} {:>9}  VERDICT", "DEVICE", "RMS");
    let mut working = Vec::new();
    for c in &candidates {
        let verdict = probe::loopback_rms(&host, c);
        let (rms, label) = match &verdict {
            Verdict::Loops(r) => (format!("{r:.4}"), "loops back — usable".to_string()),
            Verdict::Silent(r) => (format!("{r:.4}"), "silent — not a loopback".to_string()),
            Verdict::Error(e) => ("-".into(), format!("error: {e}")),
        };
        println!("  {:<36} {rms:>9}  {label}", c.label());
        if verdict.is_loopback() {
            working.push(c.clone());
        }
    }

    println!();
    match output::preferred(&working) {
        Some(best) => {
            println!("OK. Cardmic will play into: {}", best.output);
            println!("In your app, choose \"{}\" as the microphone.", best.input);
            Ok(())
        }
        None => {
            print_install_hint();
            Err("no working loopback device".into())
        }
    }
}

fn print_install_hint() {
    let (name, url) = install_hint();
    println!("Install {name} (free): {url}");
    if cfg!(target_os = "macos") {
        println!("  or: brew install blackhole-2ch");
    }
    println!("It may need a restart to finish installing. USB mode needs none of this.");
}

// ---------------------------------------------------------------- run

struct RunArgs {
    output: Option<String>,
    devices: Vec<SocketAddr>,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut out = RunArgs { output: None, devices: Vec::new() };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--output" => out.output = Some(it.next().ok_or("--output needs a device name")?.clone()),
            "--device" => {
                let v = it.next().ok_or("--device needs ADDR:PORT")?;
                out.devices.push(v.parse().map_err(|e| format!("--device {v:?}: {e}"))?);
            }
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(out)
}

fn pair(args: &[String]) -> Result<(), String> {
    let raw = args.join(" ");
    if raw.trim().is_empty() {
        return Err("usage: cardmic pair XXXX-XXXX-XXXX  (the code in Cardmic > Settings > Pairing)".into());
    }
    let code = normalize_code(&raw).map_err(|e| e.to_string())?;
    let path = pairing_store::save(&code)?;
    println!("Paired with code {}. Saved to {}.", format_code(&code), path.display());
    println!("Turn on pairing on the Cardputer (Settings > Pairing), then run: cardmic run");
    Ok(())
}

fn unpair() -> Result<(), String> {
    if pairing_store::remove()? {
        println!("Forgot the pairing code.");
    } else {
        println!("This computer was not paired.");
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let opts = parse_run_args(args)?;
    let host = cpal::default_host();

    // Which device to play into, and what the user should pick as the mic.
    let target: Candidate = match &opts.output {
        Some(name) => output::named(&host, name),
        None => {
            println!("Looking for a loopback device...");
            output::choose(&host, None).ok_or_else(|| {
                print_install_hint();
                "no working loopback device found; run `cardmic doctor`".to_string()
            })?
        }
    };
    let keys = pairing_store::load_keys();
    let paired = keys.is_some();

    let engine = Engine::start(
        Options { output: target.clone(), keys, devices: opts.devices.clone() },
        |event| match event {
            Event::Connected { addr, encrypted } => {
                let how = if encrypted { "encrypted" } else { "unencrypted" };
                println!("\nConnected to Cardputer at {addr} ({how})");
            }
            Event::Lost { addr } => println!("\nCardputer at {addr} stopped sending; searching again..."),
            Event::Named { addr, name } => println!("The Cardputer at {addr} is {name}"),
            Event::UnencryptedWhilePaired => println!(
                "\nNote: this Cardputer is not requiring pairing, so audio is unencrypted. Turn on Settings > Pairing."
            ),
            Event::StillSearching { paired, auth_failures } => {
                if auth_failures > 0 {
                    println!("Still searching. A Cardputer is sending audio this computer cannot decrypt: the pairing code changed. Run `cardmic pair` with the new code.");
                } else if paired {
                    println!("Still searching. Is Cardmic open and on the same Wi-Fi? If the pairing code changed, run `cardmic pair` again.");
                } else {
                    println!("Still searching. Is Cardmic open and on the same Wi-Fi? If Settings > Pairing is on, run `cardmic pair CODE` first.");
                }
            }
            Event::PairingRequired { addr } => println!(
                "\nThe Cardputer at {addr} requires pairing. Run `cardmic pair CODE` with the code in Cardmic > Settings > Pairing."
            ),
            Event::OutputReopened { sample_rate, channels } => {
                eprintln!("\noutput stream reopened at {sample_rate} Hz, {channels} ch")
            }
            Event::OutputReopenFailed(e) => eprintln!("\nreopening the output failed, will retry: {e}"),
            Event::ReceiveError(e) => eprintln!("receive error: {e}"),
        },
    )
    .map_err(|e| match e {
        cardmic_engine::StartError::PortInUse => {
            "another `cardmic run` or the Cardmic menu bar app is already receiving on this computer \
             (UDP port 41234 is in use). Close it, or keep using that one."
                .to_string()
        }
        other => other.to_string(),
    })?;

    let st = engine.status();
    println!(
        "Playing into \"{}\" ({} Hz, {} ch). In your app, choose \"{}\" as the microphone.",
        st.output, st.sample_rate, st.channels, st.mic
    );
    if paired {
        println!("Paired: audio from a Cardputer with pairing on is encrypted.");
    } else {
        println!(
            "Not paired: audio travels unencrypted. For privacy, turn on Settings > Pairing on the Cardputer and run `cardmic pair CODE`."
        );
    }
    println!("Searching for a Cardputer...");

    loop {
        std::thread::sleep(Duration::from_secs(1));
        let st = engine.status();
        if matches!(st.link, Link::Connected { .. }) {
            let auth = if st.auth_failures > 0 { format!(" · auth failures {}", st.auth_failures) } else { String::new() };
            print!(
                "\r  {} pkts · buffer {:>4.0}/{:.0} ms · concealed {} · resyncs {} · underruns {}{auth}   ",
                st.packets, st.buffered_ms, st.target_ms, st.concealed, st.resyncs, st.underruns
            );
            let _ = io::stdout().flush();
        }
    }
}
