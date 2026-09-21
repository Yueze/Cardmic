//! Cardmic desktop client.
//!
//! Receives the Cardputer's wireless microphone stream and plays it into a
//! loopback audio device, where every other app sees it as a microphone.

mod net;
mod pairing_store;
mod png;
mod screenshot;

use cardmic_audio::probe::{self, Verdict};
use cardmic_audio::sink::{OutputSink, SampleQueue};
use cardmic_audio::Candidate;
use cardmic_core::dsp::{i16_to_f32, Resampler};
use cardmic_core::pairing::{format_code, normalize_code, Keys, OpenError};
use cardmic_core::protocol::{
    Packet, DISCOVERY_INTERVAL, DISCOVERY_MESSAGE, RECEIVER_TIMEOUT, SAMPLES_PER_PACKET,
    SAMPLE_RATE,
};
use cardmic_core::sequencer::{Action, Sequencer};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

const USAGE: &str = "\
cardmic — use an M5Stack Cardputer ADV as a wireless microphone

USAGE:
    cardmic run [--output NAME] [--device ADDR:PORT]...
    cardmic doctor
    cardmic devices
    cardmic screenshot DEVICE_IP [--page NAME] [-o FILE] [--scale N]
    cardmic pair CODE
    cardmic unpair

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
    match preferred(&working) {
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
    if cfg!(target_os = "macos") {
        println!("Install BlackHole (free): brew install blackhole-2ch");
        println!("  or download it from https://existential.audio/blackhole/");
    } else if cfg!(target_os = "windows") {
        println!("Install VB-CABLE (donationware): https://vb-audio.com/Cable/");
    }
    println!("Both require a reboot to finish installing. USB mode needs none of this.");
}

/// Pick a device among those that verifiably loop back. Purpose-built
/// loopback drivers are preferred over ones bundled with conferencing apps,
/// which can disappear when that app is uninstalled or updated.
fn preferred(working: &[Candidate]) -> Option<&Candidate> {
    working
        .iter()
        .find(|c| c.output.contains("BlackHole"))
        .or_else(|| working.iter().find(|c| c.output.contains("CABLE")))
        .or_else(|| working.first())
}

fn auto_select(host: &cpal::Host) -> Result<Candidate, String> {
    let working: Vec<Candidate> = cardmic_audio::loopback_candidates(host)
        .into_iter()
        .filter(|c| probe::loopback_rms(host, c).is_loopback())
        .collect();
    preferred(&working).cloned().ok_or_else(|| {
        print_install_hint();
        "no working loopback device found; run `cardmic doctor`".into()
    })
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

/// Live receive state. Everything here is owned by the main thread; the only
/// thing shared with the audio callback is the [`SampleQueue`].
struct Stream {
    sink: OutputSink,
    queue: Arc<SampleQueue>,
    resampler: Resampler,
    scratch: Vec<f32>,
}

impl Stream {
    fn open(host: &cpal::Host, name: &str) -> Result<Self, String> {
        let sink = OutputSink::open(host, name)?;
        let queue = sink.queue();
        let resampler = Resampler::new(SAMPLE_RATE, sink.sample_rate());
        Ok(Stream { sink, queue, resampler, scratch: Vec::with_capacity(4096) })
    }

    /// Resample 16 kHz samples to the device rate and queue them.
    fn push(&mut self, samples: &[f32]) {
        self.scratch.clear();
        self.resampler.process(samples, &mut self.scratch);
        self.queue.push(&self.scratch);
    }

    /// Conceal `packets` lost packets. The silence goes through the
    /// resampler rather than straight into the queue, so the resampler's phase
    /// stays continuous across the gap and there is no click at either edge.
    fn conceal(&mut self, packets: u32) {
        let zeros = vec![0.0f32; packets as usize * SAMPLES_PER_PACKET];
        self.push(&zeros);
    }

    fn resync(&mut self) {
        self.queue.clear();
        self.resampler.reset();
    }
}

#[derive(Default)]
struct Stats {
    packets: u64,
    concealed: u64,
    resyncs: u64,
    dropped: u64,
    auth_failures: u64,
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

/// Varies between discovery packets; uniqueness is all it needs.
fn discovery_nonce(counter: &mut u64) -> [u8; 8] {
    *counter = counter.wrapping_add(1);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    (t ^ counter.rotate_left(32)).to_le_bytes()
}

fn run(args: &[String]) -> Result<(), String> {
    let opts = parse_run_args(args)?;
    let host = cpal::default_host();

    // Which device to play into, and what the user should pick as the mic.
    let (output, mic) = match opts.output {
        Some(name) => {
            let mic = cardmic_audio::loopback_candidates(&host)
                .into_iter()
                .find(|c| c.output == name)
                .map(|c| c.input)
                .unwrap_or_else(|| name.clone());
            (name, mic)
        }
        None => {
            println!("Looking for a loopback device...");
            let c = auto_select(&host)?;
            (c.output, c.input)
        }
    };

    let mut stream = Stream::open(&host, &output)?;
    println!(
        "Playing into \"{}\" ({} Hz, {} ch). In your app, choose \"{mic}\" as the microphone.",
        stream.sink.name(),
        stream.sink.sample_rate(),
        stream.sink.channels()
    );
    let keys: Option<Keys> = pairing_store::load_keys();
    match &keys {
        Some(_) => println!("Paired: audio from a Cardputer with pairing on is encrypted."),
        None => println!(
            "Not paired: audio travels unencrypted. For privacy, turn on Settings > Pairing on the Cardputer and run `cardmic pair CODE`."
        ),
    }
    let mut nonce_counter = 0u64;
    let mut warned_plain = false;

    let socket = net::bind().map_err(|e| format!("cannot bind UDP port 41234: {e}"))?;
    let targets = net::discovery_targets(&opts.devices);
    println!("Searching for a Cardputer ({} discovery target(s))...", targets.len());

    let mut sequencer = Sequencer::new();
    let mut stats = Stats::default();
    let mut device: Option<SocketAddr> = None;
    let mut last_packet: Option<Instant> = None;
    let mut last_discovery: Option<Instant> = None;
    let mut last_status = Instant::now();
    let search_started = Instant::now();
    let mut hinted = false;
    let mut buf = [0u8; 2048];

    loop {
        // Keepalive: the device drops us 2.5 s after the last discovery.
        // Sent from the receive socket -- see net.rs for why that matters.
        if last_discovery.is_none_or(|t| t.elapsed() >= DISCOVERY_INTERVAL) {
            let v2 = keys.as_ref().map(|k| k.discovery(discovery_nonce(&mut nonce_counter)));
            let message: &[u8] = match &v2 {
                Some(d) => d,
                None => DISCOVERY_MESSAGE,
            };
            for target in &targets {
                let _ = socket.send_to(message, target);
            }
            last_discovery = Some(Instant::now());
        }

        // Another app changed the device's sample rate, or it went away.
        if stream.sink.needs_rebuild() {
            eprintln!("\noutput stream invalidated; reopening \"{output}\"");
            std::thread::sleep(Duration::from_millis(200));
            match Stream::open(&host, &output) {
                Ok(s) => {
                    stream = s;
                    sequencer.reset();
                    eprintln!("reopened at {} Hz, {} ch", stream.sink.sample_rate(), stream.sink.channels());
                }
                Err(e) => eprintln!("reopen failed, will retry: {e}"),
            }
        }

        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let data = &buf[..n];
                // Our own broadcasts come back to us; ignore them.
                if data == DISCOVERY_MESSAGE || data.starts_with(cardmic_core::pairing::DISCOVERY_V2_PREFIX) {
                    continue;
                }
                let packet = match Packet::decode(data) {
                    Ok(p) => {
                        if keys.is_some() && !warned_plain {
                            warned_plain = true;
                            println!(
                                "\nNote: this Cardputer is not requiring pairing, so audio is unencrypted. Turn on Settings > Pairing."
                            );
                        }
                        p
                    }
                    Err(cardmic_core::protocol::DecodeError::Encrypted) => match keys.as_ref().map(|k| k.open(data)) {
                        Some(Ok(p)) => p,
                        Some(Err(OpenError::Authentication)) | None => {
                            stats.auth_failures += 1;
                            continue;
                        }
                        Some(Err(OpenError::NotCpm2)) => continue,
                    },
                    Err(_) => continue, // not ours: a LAN carries plenty of other traffic
                };

                if device != Some(from) {
                    let how = match packet.version {
                        cardmic_core::protocol::Version::Cpm2 => "encrypted",
                        cardmic_core::protocol::Version::Cpm1 => "unencrypted",
                    };
                    println!("\nConnected to Cardputer at {from} ({how})");
                    device = Some(from);
                    sequencer.reset();
                    stream.resync();
                }
                last_packet = Some(Instant::now());

                let samples = i16_to_f32(&packet.samples);
                match sequencer.accept(packet.sequence) {
                    Action::Play => stream.push(&samples),
                    Action::ConcealThenPlay { silent_packets } => {
                        stats.concealed += silent_packets as u64;
                        stream.conceal(silent_packets);
                        stream.push(&samples);
                    }
                    Action::ResyncThenPlay => {
                        stats.resyncs += 1;
                        stream.resync();
                        stream.push(&samples);
                    }
                    Action::Drop => {
                        stats.dropped += 1;
                        continue;
                    }
                }
                stats.packets += 1;
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => eprintln!("receive error: {e}"),
        }

        if let (Some(addr), Some(t)) = (device, last_packet) {
            if t.elapsed() > RECEIVER_TIMEOUT {
                println!("\nCardputer at {addr} stopped sending; searching again...");
                device = None;
                last_packet = None;
                sequencer.reset();
                stream.resync();
            }
        }

        if device.is_none() && !hinted && search_started.elapsed() > Duration::from_secs(6) {
            hinted = true;
            if keys.is_some() {
                println!("Still searching. Is Cardmic open and on the same Wi-Fi? If the pairing code changed, run `cardmic pair` again.");
            } else {
                println!("Still searching. Is Cardmic open and on the same Wi-Fi? If Settings > Pairing is on, run `cardmic pair CODE` first.");
            }
        }

        if last_status.elapsed() >= Duration::from_secs(1) {
            last_status = Instant::now();
            if device.is_some() {
                let rate = stream.sink.sample_rate() as f64;
                let buffered_ms = stream.queue.buffered() as f64 / rate * 1000.0;
                let auth = if stats.auth_failures > 0 {
                    format!(" · auth failures {}", stats.auth_failures)
                } else {
                    String::new()
                };
                print!(
                    "\r  {} pkts · buffer {:>4.0}/{:.0} ms · concealed {} · resyncs {} · underruns {}{auth}   ",
                    stats.packets,
                    buffered_ms,
                    stream.queue.target() as f64 / rate * 1000.0,
                    stats.concealed,
                    stats.resyncs,
                    stream.queue.underruns()
                );
                let _ = io::stdout().flush();
            }
        }
    }
}
