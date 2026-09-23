//! The receiving engine, shared by the `cardmic` command line and the
//! Cardmic menu bar app.
//!
//! [`Engine::start`] opens the loopback device and the UDP socket on a
//! background thread and returns once both are ready. The thread keeps the
//! Cardputer streaming (discovery keepalive), decrypts and orders packets,
//! conceals losses and plays the audio. Callers watch it through
//! [`Engine::status`] and an event callback, and stop it by dropping it.

pub mod net;
pub mod output;
pub mod pairing_store;
pub mod spectrum;

use cardmic_audio::sink::{OutputSink, SampleQueue};
use cardmic_audio::Candidate;
use cardmic_core::dsp::{i16_to_f32, Resampler};
use cardmic_core::pairing::{Keys, OpenError, DISCOVERY_V2_PREFIX};
use cardmic_core::protocol::{
    DecodeError, Packet, Version, DISCOVERY_INTERVAL, DISCOVERY_MESSAGE, RECEIVER_TIMEOUT,
    SAMPLES_PER_PACKET, SAMPLE_RATE,
};
use cardmic_core::sequencer::{Action, Sequencer};
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// What a Cardputer with pairing on answers a discovery it cannot accept.
pub const PAIRING_REQUIRED: &[u8] = b"CARDMIC_PAIRING_REQUIRED";

/// How long to wait with no Cardputer before [`Event::StillSearching`].
const SEARCH_HINT_AFTER: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Searching,
    Connected { addr: SocketAddr, encrypted: bool },
}

/// A snapshot of the engine, cheap to take several times a second.
#[derive(Debug, Clone)]
pub struct Status {
    pub link: Link,
    /// Device Cardmic plays into.
    pub output: String,
    /// Device the user picks as the microphone in their app.
    pub mic: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// This computer has a pairing code.
    pub paired: bool,
    pub packets: u64,
    pub concealed: u64,
    pub resyncs: u64,
    pub dropped: u64,
    /// Encrypted packets that did not authenticate: wrong or old pairing code.
    pub auth_failures: u64,
    /// The Cardputer said it requires pairing and this computer has no code,
    /// or not its current one. Cleared once audio flows.
    pub pairing_refused: bool,
    pub underruns: u64,
    pub buffered_ms: f64,
    pub target_ms: f64,
    /// Peak level of the last ~100 ms of audio, 0..1.
    pub level: f32,
}

/// Things worth telling the user about, as they happen.
#[derive(Debug, Clone)]
pub enum Event {
    Connected { addr: SocketAddr, encrypted: bool },
    Lost { addr: SocketAddr },
    /// This computer is paired but the Cardputer is not requiring pairing.
    UnencryptedWhilePaired,
    /// Nothing found for a few seconds.
    StillSearching { paired: bool, auth_failures: u64 },
    /// A Cardputer refused this computer: pairing is on and the code is
    /// missing or out of date.
    PairingRequired { addr: SocketAddr },
    OutputReopened { sample_rate: u32, channels: u16 },
    OutputReopenFailed(String),
    ReceiveError(String),
}

pub struct Options {
    pub output: Candidate,
    /// Pairing keys; `None` for an unpaired computer.
    pub keys: Option<Keys>,
    /// Extra unicast discovery targets, for networks that block broadcast.
    pub devices: Vec<SocketAddr>,
}

/// Why the engine could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// UDP port 41234 is taken, almost always by another Cardmic.
    PortInUse,
    Other(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::PortInUse => f.write_str(
                "another Cardmic is already receiving on this computer (UDP port 41234 is in use)",
            ),
            StartError::Other(e) => f.write_str(e),
        }
    }
}

pub struct Engine {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<Status>>,
    spectrum: Arc<Mutex<spectrum::Spectrum>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(opts: Options, on_event: impl FnMut(Event) + Send + 'static) -> Result<Engine, StartError> {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(Status {
            link: Link::Searching,
            output: opts.output.output.clone(),
            mic: opts.output.input.clone(),
            sample_rate: 0,
            channels: 0,
            paired: opts.keys.is_some(),
            packets: 0,
            concealed: 0,
            resyncs: 0,
            dropped: 0,
            auth_failures: 0,
            pairing_refused: false,
            underruns: 0,
            buffered_ms: 0.0,
            target_ms: 0.0,
            level: 0.0,
        }));
        let spectrum = Arc::new(Mutex::new(spectrum::Spectrum::new()));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = {
            let stop = stop.clone();
            let status = status.clone();
            let spectrum = spectrum.clone();
            std::thread::Builder::new()
                .name("cardmic-engine".into())
                .spawn(move || {
                    // The audio stream is created on this thread: cpal streams
                    // are not Send on every platform.
                    let host = cpal::default_host();
                    let receiver = match Receiver::open(&host, opts, status, spectrum) {
                        Ok(r) => {
                            let _ = ready_tx.send(Ok(()));
                            r
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    receiver.run(&host, &stop, on_event);
                })
                .map_err(|e| StartError::Other(format!("cannot start the receive thread: {e}")))?
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Engine { stop, status, spectrum, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(StartError::Other("the receive thread stopped unexpectedly".into()))
            }
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Spectrogram columns (see [`spectrum`]) since the last call.
    pub fn take_spectrum(&self) -> Vec<spectrum::Column> {
        self.spectrum.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Output side: resampler and queue in front of the loopback device.
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

struct Receiver {
    socket: UdpSocket,
    stream: Stream,
    output: String,
    keys: Option<Keys>,
    devices: Vec<SocketAddr>,
    targets: Vec<SocketAddr>,
    status: Arc<Mutex<Status>>,
    spectrum: Arc<Mutex<spectrum::Spectrum>>,
}

impl Receiver {
    fn open(
        host: &cpal::Host,
        opts: Options,
        status: Arc<Mutex<Status>>,
        spectrum: Arc<Mutex<spectrum::Spectrum>>,
    ) -> Result<Receiver, StartError> {
        // Socket first: if another Cardmic is running, say so rather than
        // grabbing the audio device it is using.
        let socket = net::bind().map_err(|e| match e.kind() {
            io::ErrorKind::AddrInUse => StartError::PortInUse,
            _ => StartError::Other(format!("cannot bind UDP port 41234: {e}")),
        })?;
        let stream = Stream::open(host, &opts.output.output).map_err(StartError::Other)?;
        {
            let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
            s.sample_rate = stream.sink.sample_rate();
            s.channels = stream.sink.channels();
        }
        Ok(Receiver {
            socket,
            stream,
            output: opts.output.output,
            keys: opts.keys,
            targets: net::discovery_targets(&opts.devices),
            devices: opts.devices,
            status,
            spectrum,
        })
    }

    fn run(mut self, host: &cpal::Host, stop: &AtomicBool, mut on_event: impl FnMut(Event)) {
        let mut sequencer = Sequencer::new();
        let mut device: Option<SocketAddr> = None;
        let mut last_packet: Option<Instant> = None;
        let mut last_discovery: Option<Instant> = None;
        let mut last_status = Instant::now();
        let mut searching_since = Instant::now();
        let mut hinted = false;
        let mut warned_plain = false;
        let mut nonce_counter = 0u64;
        let mut peak = 0.0f32;
        let mut buf = [0u8; 2048];

        while !stop.load(Ordering::Relaxed) {
            // Keepalive: the device drops us 2.5 s after the last discovery.
            // Sent from the receive socket -- see net.rs for why that matters.
            if last_discovery.is_none_or(|t| t.elapsed() >= DISCOVERY_INTERVAL) {
                let v2 = self.keys.as_ref().map(|k| k.discovery(discovery_nonce(&mut nonce_counter)));
                let message: &[u8] = match &v2 {
                    Some(d) => d,
                    None => DISCOVERY_MESSAGE,
                };
                for target in &self.targets {
                    let _ = self.socket.send_to(message, target);
                }
                last_discovery = Some(Instant::now());
                // Interfaces come and go (laptop joins another network).
                self.targets = net::discovery_targets(&self.devices);
            }

            // Another app changed the device's sample rate, or it went away.
            if self.stream.sink.needs_rebuild() {
                std::thread::sleep(Duration::from_millis(200));
                match Stream::open(host, &self.output) {
                    Ok(s) => {
                        self.stream = s;
                        sequencer.reset();
                        let (sample_rate, channels) = (self.stream.sink.sample_rate(), self.stream.sink.channels());
                        {
                            let mut st = self.lock();
                            st.sample_rate = sample_rate;
                            st.channels = channels;
                        }
                        on_event(Event::OutputReopened { sample_rate, channels });
                    }
                    Err(e) => on_event(Event::OutputReopenFailed(e)),
                }
            }

            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => {
                    let data = &buf[..n];
                    // Our own broadcasts come back to us; ignore them.
                    if data == DISCOVERY_MESSAGE || data.starts_with(DISCOVERY_V2_PREFIX) {
                        continue;
                    }
                    if data == PAIRING_REQUIRED {
                        let first = !std::mem::replace(&mut self.lock().pairing_refused, true);
                        if first {
                            on_event(Event::PairingRequired { addr: from });
                        }
                        continue;
                    }
                    let packet = match Packet::decode(data) {
                        Ok(p) => {
                            if self.keys.is_some() && !warned_plain {
                                warned_plain = true;
                                on_event(Event::UnencryptedWhilePaired);
                            }
                            p
                        }
                        Err(DecodeError::Encrypted) => match self.keys.as_ref().map(|k| k.open(data)) {
                            Some(Ok(p)) => p,
                            Some(Err(OpenError::Authentication)) | None => {
                                self.lock().auth_failures += 1;
                                continue;
                            }
                            Some(Err(OpenError::NotCpm2)) => continue,
                        },
                        Err(_) => continue, // not ours: a LAN carries plenty of other traffic
                    };

                    if device != Some(from) {
                        let encrypted = packet.version == Version::Cpm2;
                        self.lock().pairing_refused = false;
                        device = Some(from);
                        sequencer.reset();
                        self.stream.resync();
                        self.lock().link = Link::Connected { addr: from, encrypted };
                        on_event(Event::Connected { addr: from, encrypted });
                    }
                    last_packet = Some(Instant::now());

                    let samples = i16_to_f32(&packet.samples);
                    peak = samples.iter().fold(peak, |m, s| m.max(s.abs()));
                    let mut st_packets = 1u64;
                    match sequencer.accept(packet.sequence) {
                        Action::Play => {
                            self.stream.push(&samples);
                            self.analyse(&samples);
                        }
                        Action::ConcealThenPlay { silent_packets } => {
                            self.lock().concealed += silent_packets as u64;
                            self.stream.conceal(silent_packets);
                            self.stream.push(&samples);
                            self.analyse(&vec![0.0; silent_packets as usize * SAMPLES_PER_PACKET]);
                            self.analyse(&samples);
                        }
                        Action::ResyncThenPlay => {
                            self.lock().resyncs += 1;
                            self.stream.resync();
                            self.stream.push(&samples);
                            self.analyse(&samples);
                        }
                        Action::Drop => {
                            self.lock().dropped += 1;
                            st_packets = 0;
                        }
                    }
                    self.lock().packets += st_packets;
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(e) => {
                    on_event(Event::ReceiveError(e.to_string()));
                    std::thread::sleep(Duration::from_millis(100));
                }
            }

            if let (Some(addr), Some(t)) = (device, last_packet) {
                if t.elapsed() > RECEIVER_TIMEOUT {
                    device = None;
                    last_packet = None;
                    sequencer.reset();
                    self.stream.resync();
                    searching_since = Instant::now();
                    hinted = false;
                    self.lock().link = Link::Searching;
                    on_event(Event::Lost { addr });
                }
            }

            if device.is_none() && !hinted && searching_since.elapsed() > SEARCH_HINT_AFTER {
                hinted = true;
                let auth_failures = self.lock().auth_failures;
                on_event(Event::StillSearching { paired: self.keys.is_some(), auth_failures });
            }

            if last_status.elapsed() >= Duration::from_millis(100) {
                last_status = Instant::now();
                let rate = self.stream.sink.sample_rate().max(1) as f64;
                let buffered_ms = self.stream.queue.buffered() as f64 / rate * 1000.0;
                let target_ms = self.stream.queue.target() as f64 / rate * 1000.0;
                let underruns = self.stream.queue.underruns();
                let receiving = last_packet.is_some_and(|t| t.elapsed() < Duration::from_millis(300));
                let mut st = self.lock();
                st.buffered_ms = buffered_ms;
                st.target_ms = target_ms;
                st.underruns = underruns;
                st.level = if receiving { peak } else { 0.0 };
                peak = 0.0;
            }
        }
    }

    fn analyse(&self, samples: &[f32]) {
        self.spectrum.lock().unwrap_or_else(|e| e.into_inner()).push(samples);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }
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
