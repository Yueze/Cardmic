//! Runs a 16 kHz mono 16-bit WAV through the engine's spectrum and prints one
//! column per line, as hex: 62 bands and the level, which is what the app's
//! window receives. Used to make the README's screenshots from a recording.
//!
//!     cargo run -p cardmic-engine --example spectrum_columns -- speech.wav > speech.cols
use cardmic_engine::spectrum::Spectrum;

fn main() {
    let path = std::env::args().nth(1).expect("usage: spectrum_columns FILE.wav");
    let data = std::fs::read(&path).expect("read the WAV");
    let at = data.windows(4).position(|w| w == b"data").expect("a WAV data chunk") + 8;
    let samples: Vec<f32> =
        data[at..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    let mut spectrum = Spectrum::new();
    for chunk in samples.chunks(160) {
        spectrum.push(chunk);
        for col in spectrum.take() {
            let line: String = col.bands.iter().chain(std::iter::once(&col.level)).map(|b| format!("{b:02x}")).collect();
            println!("{line}");
        }
    }
}
