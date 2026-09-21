//! Synthetic Cardmic device.
//!
//! Stands in for a Cardputer ADV so a client can be developed and tested with
//! no hardware. It deliberately reproduces the firmware's two contract rules
//! rather than being lenient about them, so a client that gets either wrong
//! fails here exactly as it would against the real device:
//!
//! 1. It streams to the discovery packet's **source address and source port**
//!    (see `cardmic_net.c` in the firmware). A client that sends discovery
//!    from a different socket than it receives on will get nothing.
//! 2. It drops the receiver [`RECEIVER_TIMEOUT`] after the last discovery
//!    packet. A client that broadcasts once will go silent after 2.5 s.
//!
//! Usage:
//!   cardmic-synth [--port N] [--pair CODE] [--tone HZ] [--loss PCT]
//!
//! Without `--pair` it behaves like a device with pairing off (plain CPM1,
//! any discovery accepted). With `--pair` it behaves like a device with
//! pairing on: only authenticated v2 discovery, audio sealed as CPM2.
//!
//! `--port` defaults to 41235, not 41234, so it can run on the same machine as
//! a client that binds 41234. Point the client's discovery at 127.0.0.1:41235.

use cardmic_core::pairing::{normalize_code, Keys};
use cardmic_core::protocol::{
    Packet, Version, DISCOVERY_MESSAGE, RECEIVER_TIMEOUT, SAMPLES_PER_PACKET, SAMPLE_RATE,
};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const PACKET_INTERVAL: Duration = Duration::from_millis(20);

struct Config {
    port: u16,
    keys: Option<Keys>,
    tone_hz: f32,
    loss_pct: u32,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config { port: 41235, keys: None, tone_hz: 440.0, loss_pct: 0 };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--port" => cfg.port = value("--port")?.parse().map_err(|e| format!("--port: {e}"))?,
            "--pair" => {
                let code = normalize_code(&value("--pair")?).map_err(|e| format!("--pair: {e}"))?;
                cfg.keys = Some(Keys::derive(&code));
            }
            "--tone" => cfg.tone_hz = value("--tone")?.parse().map_err(|e| format!("--tone: {e}"))?,
            "--loss" => {
                cfg.loss_pct = value("--loss")?.parse().map_err(|e| format!("--loss: {e}"))?;
                if cfg.loss_pct > 100 {
                    return Err("--loss must be 0..=100".into());
                }
            }
            "-h" | "--help" => {
                println!("cardmic-synth [--port N] [--pair CODE] [--tone HZ] [--loss PCT]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(cfg)
}

/// Deterministic pseudo-random loss so a run is reproducible.
struct Lossy {
    state: u32,
    pct: u32,
}

impl Lossy {
    fn drop_this(&mut self) -> bool {
        // xorshift32
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state % 100 < self.pct
    }
}

fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    let socket = UdpSocket::bind(("0.0.0.0", cfg.port)).unwrap_or_else(|e| {
        eprintln!("error: cannot bind UDP port {}: {e}", cfg.port);
        std::process::exit(1);
    });
    // Short timeout so the loop can both service discovery and pace packets.
    socket.set_read_timeout(Some(Duration::from_millis(5))).expect("set_read_timeout");

    println!(
        "cardmic-synth listening on :{}  (pairing {}, {} Hz tone, {}% loss)",
        cfg.port,
        if cfg.keys.is_some() { "on" } else { "off" },
        cfg.tone_hz,
        cfg.loss_pct
    );

    let mut receiver: Option<SocketAddr> = None;
    let mut last_discovery = Instant::now();
    let mut next_packet = Instant::now();
    let mut sequence: u32 = 0;
    let mut session: u32 = 0;
    let mut phase: f32 = 0.0;
    let phase_step = cfg.tone_hz / SAMPLE_RATE as f32;
    let mut lossy = Lossy { state: 0x9E37_79B9, pct: cfg.loss_pct };
    let mut buf = [0u8; 64];

    loop {
        // Service discovery. The reply goes to the SOURCE address and port of
        // this datagram -- rule 1 of the contract.
        match socket.recv_from(&mut buf) {
            Ok((n, from))
                if match &cfg.keys {
                    Some(k) => k.verify_discovery(&buf[..n]),
                    None => &buf[..n] == DISCOVERY_MESSAGE || buf[..n].starts_with(b"CPADV_MIC_DISCOVER_V2"),
                } =>
            {
                if receiver != Some(from) {
                    println!("receiver: {from}");
                    sequence = 0;
                    session = session.wrapping_mul(1_664_525).wrapping_add(1_013_904_223) ^ from.port() as u32;
                }
                receiver = Some(from);
                last_discovery = Instant::now();
            }
            Ok(_) | Err(_) => {}
        }

        // Rule 2: no keepalive for RECEIVER_TIMEOUT means the receiver is gone.
        if receiver.is_some() && last_discovery.elapsed() > RECEIVER_TIMEOUT {
            println!("receiver timed out after {RECEIVER_TIMEOUT:?} without discovery");
            receiver = None;
        }

        let Some(dest) = receiver else { continue };
        if Instant::now() < next_packet {
            continue;
        }
        next_packet += PACKET_INTERVAL;
        // If we fell far behind (e.g. the process was suspended), do not burst.
        if Instant::now() > next_packet + PACKET_INTERVAL * 4 {
            next_packet = Instant::now() + PACKET_INTERVAL;
        }

        let samples: Vec<i16> = (0..SAMPLES_PER_PACKET)
            .map(|_| {
                let s = (phase * std::f32::consts::TAU).sin() * 0.5;
                phase = (phase + phase_step).fract();
                (s * i16::MAX as f32) as i16
            })
            .collect();

        let wire = match &cfg.keys {
            Some(k) => k.seal(session, sequence, &samples),
            None => Packet::new(Version::Cpm1, sequence, samples).encode(),
        };
        sequence = sequence.wrapping_add(1);

        if lossy.drop_this() {
            continue; // simulated loss: sequence still advanced, as on a real link
        }
        if let Err(e) = socket.send_to(&wire, dest) {
            eprintln!("send to {dest} failed: {e}");
        }
    }
}
