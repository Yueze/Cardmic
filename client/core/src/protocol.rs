//! Cardmic wire protocol.
//!
//! Two packet versions are understood. `CPM1` is what the firmware speaks
//! today. `CPM2` is reserved for pairing: it keeps CPM1's 16-byte header
//! byte-for-byte and inserts a 16-byte authentication tag before the payload,
//! so that adding authentication later does not change the framing again.
//!
//! ```text
//! CPM1 (656 bytes)                 CPM2 (672 bytes)
//!  0.. 3  magic  "CPM1"             0.. 3  magic  "CPM2"
//!  4      version = 1               4      version = 2
//!  5      flags                     5      flags
//!  6.. 7  sample_count = 320        6.. 7  sample_count = 320
//!  8..11  sequence                  8..11  sequence
//! 12..15  sample_rate = 16000      12..15  sample_rate = 16000
//! 16..    samples i16[320]         16..31  auth_tag [16]  (zero in v1)
//!                                  32..    samples i16[320]
//! ```
//!
//! All integers are little-endian. The sample payload is raw PCM.
//!
//! # The part that is not in the byte layout
//!
//! Two contract rules are as load-bearing as the framing, and a client that
//! gets them wrong fails in ways that look like firmware bugs:
//!
//! 1. **The device streams to the discovery packet's source address _and
//!    source port_**, not to [`PORT`]. Discovery must therefore be sent from
//!    the same socket that receives audio. See [`DISCOVERY_MESSAGE`].
//! 2. **Discovery is a keepalive, not a handshake.** The device drops the
//!    receiver [`RECEIVER_TIMEOUT`] after the last discovery packet, so the
//!    client must rebroadcast at [`DISCOVERY_INTERVAL`].

use std::time::Duration;

/// UDP port the device listens on for discovery, and the port a client should
/// bind so that the device's reply lands where it is listening.
pub const PORT: u16 = 41234;

/// Discovery payload, sent verbatim with no trailing NUL. The firmware
/// compares it by exact length, so 21 bytes and not 22.
pub const DISCOVERY_MESSAGE: &[u8] = b"CPADV_MIC_DISCOVER_V1";

/// The device stops streaming this long after the last discovery packet.
pub const RECEIVER_TIMEOUT: Duration = Duration::from_millis(2500);

/// How often a client should rebroadcast discovery to stay connected. Chosen
/// well under [`RECEIVER_TIMEOUT`] so that losing a datagram does not drop the
/// session; matches the prototype client.
pub const DISCOVERY_INTERVAL: Duration = Duration::from_millis(700);

/// Samples carried by one packet: 20 ms at 16 kHz.
pub const SAMPLES_PER_PACKET: usize = 320;

/// The only sample rate the device produces.
pub const SAMPLE_RATE: u32 = 16_000;

const HEADER_LEN: usize = 16;
const AUTH_TAG_LEN: usize = 16;
const PAYLOAD_LEN: usize = SAMPLES_PER_PACKET * 2;

/// Wire size of a `CPM1` packet.
pub const CPM1_LEN: usize = HEADER_LEN + PAYLOAD_LEN; // 656
/// Wire size of a `CPM2` packet.
pub const CPM2_LEN: usize = HEADER_LEN + AUTH_TAG_LEN + PAYLOAD_LEN; // 672

/// Which framing a packet uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    /// Current firmware, no authentication field.
    Cpm1,
    /// Reserved for pairing; carries a (currently zero) authentication tag.
    Cpm2,
}

impl Version {
    const fn magic(self) -> &'static [u8; 4] {
        match self {
            Version::Cpm1 => b"CPM1",
            Version::Cpm2 => b"CPM2",
        }
    }

    const fn version_byte(self) -> u8 {
        match self {
            Version::Cpm1 => 1,
            Version::Cpm2 => 2,
        }
    }

    /// Wire size of a packet in this version.
    pub const fn packet_len(self) -> usize {
        match self {
            Version::Cpm1 => CPM1_LEN,
            Version::Cpm2 => CPM2_LEN,
        }
    }

    const fn payload_offset(self) -> usize {
        match self {
            Version::Cpm1 => HEADER_LEN,
            Version::Cpm2 => HEADER_LEN + AUTH_TAG_LEN,
        }
    }
}

/// One decoded audio packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub version: Version,
    pub flags: u8,
    pub sequence: u32,
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

/// Why a datagram was not a usable Cardmic packet.
///
/// These are all expected during normal operation — a LAN carries plenty of
/// traffic that is not ours — so callers should drop and continue rather than
/// treat any of them as fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// Datagram is shorter than any valid header.
    TooShort { len: usize },
    /// First four bytes are not a magic we know.
    UnknownMagic([u8; 4]),
    /// Magic and version byte disagree.
    VersionMismatch { magic: Version, version_byte: u8 },
    /// Length does not match what the version requires.
    WrongLength { expected: usize, actual: usize },
    /// `sample_count` is not [`SAMPLES_PER_PACKET`].
    UnexpectedSampleCount(u16),
    /// `sample_rate` is not [`SAMPLE_RATE`].
    UnexpectedSampleRate(u32),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::TooShort { len } => write!(f, "datagram too short: {len} bytes"),
            DecodeError::UnknownMagic(m) => write!(f, "unknown magic {m:?}"),
            DecodeError::VersionMismatch { magic, version_byte } => {
                write!(f, "magic says {magic:?} but version byte is {version_byte}")
            }
            DecodeError::WrongLength { expected, actual } => {
                write!(f, "expected {expected} bytes, got {actual}")
            }
            DecodeError::UnexpectedSampleCount(n) => write!(f, "unexpected sample_count {n}"),
            DecodeError::UnexpectedSampleRate(r) => write!(f, "unexpected sample_rate {r}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

impl Packet {
    /// Build a packet carrying exactly [`SAMPLES_PER_PACKET`] samples.
    ///
    /// # Panics
    /// If `samples.len() != SAMPLES_PER_PACKET`.
    pub fn new(version: Version, sequence: u32, samples: Vec<i16>) -> Self {
        assert_eq!(
            samples.len(),
            SAMPLES_PER_PACKET,
            "a packet carries exactly {SAMPLES_PER_PACKET} samples"
        );
        Packet { version, flags: 0, sequence, sample_rate: SAMPLE_RATE, samples }
    }

    /// Serialise to the wire.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.version.packet_len()];
        out[0..4].copy_from_slice(self.version.magic());
        out[4] = self.version.version_byte();
        out[5] = self.flags;
        out[6..8].copy_from_slice(&(self.samples.len() as u16).to_le_bytes());
        out[8..12].copy_from_slice(&self.sequence.to_le_bytes());
        out[12..16].copy_from_slice(&self.sample_rate.to_le_bytes());
        // auth_tag (CPM2 only) stays zero in v1.
        let base = self.version.payload_offset();
        for (i, s) in self.samples.iter().enumerate() {
            out[base + i * 2..base + i * 2 + 2].copy_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// Parse a datagram. Accepts both `CPM1` and `CPM2`.
    pub fn decode(buf: &[u8]) -> Result<Packet, DecodeError> {
        if buf.len() < HEADER_LEN {
            return Err(DecodeError::TooShort { len: buf.len() });
        }

        let magic: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
        let version = match &magic {
            b"CPM1" => Version::Cpm1,
            b"CPM2" => Version::Cpm2,
            _ => return Err(DecodeError::UnknownMagic(magic)),
        };

        if buf[4] != version.version_byte() {
            return Err(DecodeError::VersionMismatch { magic: version, version_byte: buf[4] });
        }
        if buf.len() != version.packet_len() {
            return Err(DecodeError::WrongLength {
                expected: version.packet_len(),
                actual: buf.len(),
            });
        }

        let sample_count = u16_le(&buf[6..8]);
        if sample_count as usize != SAMPLES_PER_PACKET {
            return Err(DecodeError::UnexpectedSampleCount(sample_count));
        }
        let sample_rate = u32_le(&buf[12..16]);
        if sample_rate != SAMPLE_RATE {
            return Err(DecodeError::UnexpectedSampleRate(sample_rate));
        }

        let base = version.payload_offset();
        let samples = buf[base..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        Ok(Packet {
            version,
            flags: buf[5],
            sequence: u32_le(&buf[8..12]),
            sample_rate,
            samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> Vec<i16> {
        (0..SAMPLES_PER_PACKET).map(|i| (i as i16).wrapping_mul(97)).collect()
    }

    #[test]
    fn wire_sizes_match_the_firmware() {
        // packet_t in the firmware's cardmic_net.c: 16-byte header + 320 * i16.
        assert_eq!(CPM1_LEN, 656);
        assert_eq!(CPM2_LEN, 672);
    }

    #[test]
    fn discovery_message_is_21_bytes() {
        // The firmware compares by exact length; 22 (with a NUL) would never match.
        assert_eq!(DISCOVERY_MESSAGE.len(), 21);
    }

    #[test]
    fn round_trips_both_versions() {
        for version in [Version::Cpm1, Version::Cpm2] {
            let original = Packet::new(version, 0xDEAD_BEEF, ramp());
            let decoded = Packet::decode(&original.encode()).expect("decodes");
            assert_eq!(decoded, original, "{version:?} did not survive a round trip");
        }
    }

    #[test]
    fn cpm2_keeps_cpm1_field_positions() {
        // The CPM2 layout keeps every CPM1 header field at the same offset and
        // inserts the tag after the header. Field *positions* are identical;
        // the magic and version byte legitimately differ in value.
        let a = Packet::new(Version::Cpm1, 42, ramp()).encode();
        let b = Packet::new(Version::Cpm2, 42, ramp()).encode();
        assert_eq!(a[5..16], b[5..16], "flags/sample_count/sequence/sample_rate must match");
        assert_ne!(a[0..4], b[0..4], "magic must differ so CPM1 parsers reject CPM2");
        assert_eq!((a[4], b[4]), (1, 2), "version byte identifies the framing");
    }

    #[test]
    fn cpm2_auth_tag_is_zero_in_v1() {
        let encoded = Packet::new(Version::Cpm2, 1, ramp()).encode();
        assert_eq!(&encoded[16..32], &[0u8; 16]);
    }

    #[test]
    fn sequence_is_read_at_offset_8() {
        // The sequence number is a u32 LE at offset 8.
        let encoded = Packet::new(Version::Cpm1, 0x0102_0304, ramp()).encode();
        assert_eq!(&encoded[8..12], &[0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn rejects_a_cpm2_datagram_claiming_cpm1_length() {
        let mut encoded = Packet::new(Version::Cpm2, 1, ramp()).encode();
        encoded.truncate(CPM1_LEN);
        assert_eq!(
            Packet::decode(&encoded),
            Err(DecodeError::WrongLength { expected: CPM2_LEN, actual: CPM1_LEN })
        );
    }

    #[test]
    fn rejects_foreign_traffic_without_panicking() {
        for junk in [&b""[..], &b"hi"[..], &[0xFFu8; 700][..], DISCOVERY_MESSAGE] {
            assert!(Packet::decode(junk).is_err(), "should reject {} bytes", junk.len());
        }
    }

    #[test]
    fn rejects_mismatched_magic_and_version_byte() {
        let mut encoded = Packet::new(Version::Cpm2, 1, ramp()).encode();
        encoded[4] = 1; // claims CPM1 while carrying CPM2 magic
        assert_eq!(
            Packet::decode(&encoded),
            Err(DecodeError::VersionMismatch { magic: Version::Cpm2, version_byte: 1 })
        );
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let mut encoded = Packet::new(Version::Cpm1, 1, ramp()).encode();
        encoded[12..16].copy_from_slice(&48_000u32.to_le_bytes());
        assert_eq!(Packet::decode(&encoded), Err(DecodeError::UnexpectedSampleRate(48_000)));
    }
}
