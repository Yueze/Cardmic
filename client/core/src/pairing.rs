//! Pairing and encryption.
//!
//! The device shows a 12-character code; the user gives it to the client once
//! (`cardmic pair XXXX-XXXX-XXXX`). Both sides derive the same two keys:
//!
//! ```text
//! PBKDF2-HMAC-SHA256(code, "cardmic/pair/v1", 20000 rounds) -> 32 bytes
//!   [0..16]  AES-128-GCM key for audio
//!   [16..32] HMAC-SHA256 key for discovery
//! ```
//!
//! With pairing on, the device only answers discovery that proves knowledge
//! of the code, and sends audio as `CPM2`:
//!
//! ```text
//! discovery v2 (45 bytes): "CPADV_MIC_DISCOVER_V2" | nonce[8] | HMAC(mac_key, first 29 bytes)[..16]
//!
//! CPM2 (672 bytes)
//!  0.. 3  magic "CPM2"      4 version = 2     5 flags
//!  6.. 7  sample_count      8..11 sequence
//! 12..15  session  (random per receiver session; replaces sample_rate)
//! 16..31  AES-GCM tag
//! 32..    ciphertext of i16[320]
//! nonce = session (LE) | sequence (LE) | 0u32,  associated data = bytes 0..16
//! ```
//!
//! The code carries 60 bits and the key derivation is deliberately slow, so a
//! recording of the traffic cannot practically be brute-forced back to it.

use crate::protocol::{Packet, Version, CPM2_LEN, SAMPLES_PER_PACKET, SAMPLE_RATE};
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::Aes128Gcm;
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Characters in a pairing code, without dashes.
pub const CODE_LEN: usize = 12;
/// Crockford base32: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SALT: &[u8] = b"cardmic/pair/v1";
/// PBKDF2 rounds. Must match the firmware.
pub const ROUNDS: u32 = 20_000;

/// Discovery for paired clients.
pub const DISCOVERY_V2_PREFIX: &[u8] = b"CPADV_MIC_DISCOVER_V2";
/// Wire size of a v2 discovery packet.
pub const DISCOVERY_V2_LEN: usize = 45;

const HEADER_LEN: usize = 16;
const TAG_LEN: usize = 16;

/// Why a pairing code was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeError {
    /// Not 12 characters once dashes and spaces are removed.
    Length(usize),
    /// A character that cannot appear in a code.
    Character(char),
}

impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeError::Length(n) => write!(f, "a pairing code has {CODE_LEN} characters, this has {n}"),
            CodeError::Character(c) => write!(f, "{c:?} cannot appear in a pairing code"),
        }
    }
}

impl std::error::Error for CodeError {}

/// Canonical form of a code as typed: upper case, no dashes or spaces, and
/// the look-alikes O, I and L read as 0, 1 and 1.
pub fn normalize_code(input: &str) -> Result<String, CodeError> {
    let mut out = String::with_capacity(CODE_LEN);
    for c in input.chars() {
        if matches!(c, '-' | ' ' | '\t') {
            continue;
        }
        let u = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        if !u.is_ascii() || !ALPHABET.contains(&(u as u8)) {
            return Err(CodeError::Character(c));
        }
        out.push(u);
    }
    if out.len() != CODE_LEN {
        return Err(CodeError::Length(out.len()));
    }
    Ok(out)
}

/// "ABCD-EFGH-JKMN", as the device shows it.
pub fn format_code(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("-")
}

/// Keys derived from a pairing code.
#[derive(Clone)]
pub struct Keys {
    enc: [u8; 16],
    mac: [u8; 16],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)") // never print key material
    }
}

/// Why a CPM2 datagram could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Not a CPM2 packet at all.
    NotCpm2,
    /// Well-formed CPM2 that failed authentication: wrong code, or tampered.
    Authentication,
}

impl Keys {
    /// Derive the keys from a normalized code. Slow on purpose (tens of ms).
    pub fn derive(code: &str) -> Keys {
        let mut out = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(code.as_bytes(), SALT, ROUNDS, &mut out);
        let mut keys = Keys { enc: [0; 16], mac: [0; 16] };
        keys.enc.copy_from_slice(&out[..16]);
        keys.mac.copy_from_slice(&out[16..]);
        keys
    }

    fn discovery_tag(&self, first29: &[u8]) -> [u8; 16] {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.mac).expect("HMAC takes any key length");
        mac.update(first29);
        let full = mac.finalize().into_bytes();
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&full[..16]);
        tag
    }

    /// A v2 discovery packet. `nonce` only needs to vary between packets.
    pub fn discovery(&self, nonce: [u8; 8]) -> [u8; DISCOVERY_V2_LEN] {
        let mut out = [0u8; DISCOVERY_V2_LEN];
        out[..21].copy_from_slice(DISCOVERY_V2_PREFIX);
        out[21..29].copy_from_slice(&nonce);
        let tag = self.discovery_tag(&out[..29]);
        out[29..].copy_from_slice(&tag);
        out
    }

    /// Device side: does this discovery prove knowledge of the code?
    pub fn verify_discovery(&self, buf: &[u8]) -> bool {
        if buf.len() != DISCOVERY_V2_LEN || &buf[..21] != DISCOVERY_V2_PREFIX {
            return false;
        }
        let tag = self.discovery_tag(&buf[..29]);
        // Constant-time comparison.
        tag.iter().zip(&buf[29..]).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
    }

    fn cipher(&self) -> Aes128Gcm {
        Aes128Gcm::new(GenericArray::from_slice(&self.enc))
    }

    fn nonce(session: u32, sequence: u32) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[..4].copy_from_slice(&session.to_le_bytes());
        n[4..8].copy_from_slice(&sequence.to_le_bytes());
        n
    }

    /// Device side: encrypt one packet of samples as CPM2.
    pub fn seal(&self, session: u32, sequence: u32, samples: &[i16]) -> Vec<u8> {
        assert_eq!(samples.len(), SAMPLES_PER_PACKET);
        let mut out = vec![0u8; CPM2_LEN];
        out[0..4].copy_from_slice(b"CPM2");
        out[4] = 2;
        out[6..8].copy_from_slice(&(SAMPLES_PER_PACKET as u16).to_le_bytes());
        out[8..12].copy_from_slice(&sequence.to_le_bytes());
        out[12..16].copy_from_slice(&session.to_le_bytes());
        let mut body: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let (header, rest) = out.split_at_mut(HEADER_LEN);
        let tag = self
            .cipher()
            .encrypt_in_place_detached(GenericArray::from_slice(&Self::nonce(session, sequence)), header, &mut body)
            .expect("AES-GCM encryption of a fixed-size buffer cannot fail");
        rest[..TAG_LEN].copy_from_slice(&tag);
        rest[TAG_LEN..].copy_from_slice(&body);
        out
    }

    /// Decrypt and authenticate a CPM2 datagram.
    pub fn open(&self, buf: &[u8]) -> Result<Packet, OpenError> {
        if buf.len() != CPM2_LEN || &buf[0..4] != b"CPM2" || buf[4] != 2 {
            return Err(OpenError::NotCpm2);
        }
        if u16::from_le_bytes([buf[6], buf[7]]) as usize != SAMPLES_PER_PACKET {
            return Err(OpenError::NotCpm2);
        }
        let sequence = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let session = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let mut body = buf[HEADER_LEN + TAG_LEN..].to_vec();
        self.cipher()
            .decrypt_in_place_detached(
                GenericArray::from_slice(&Self::nonce(session, sequence)),
                &buf[..HEADER_LEN],
                &mut body,
                GenericArray::from_slice(&buf[HEADER_LEN..HEADER_LEN + TAG_LEN]),
            )
            .map_err(|_| OpenError::Authentication)?;
        let samples = body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        Ok(Packet { version: Version::Cpm2, flags: buf[5], sequence, sample_rate: SAMPLE_RATE, samples })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors computed independently with Python (hashlib, hmac,
    // cryptography.AESGCM) for code 7K2M9QXB4TPA.
    const CODE: &str = "7K2M9QXB4TPA";

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn ramp() -> Vec<i16> {
        (0..SAMPLES_PER_PACKET).map(|i| (i as i16).wrapping_mul(97)).collect()
    }

    #[test]
    fn key_derivation_matches_reference() {
        let k = Keys::derive(CODE);
        assert_eq!(hex(&k.enc), "982206814472972ee1f71fb4f4912b00");
        assert_eq!(hex(&k.mac), "26d8b7637fda9c8fb224c94046cf49dd");
    }

    #[test]
    fn discovery_tag_matches_reference() {
        let d = Keys::derive(CODE).discovery([0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(&d[..21], DISCOVERY_V2_PREFIX);
        assert_eq!(hex(&d[29..]), "5e61da59a626d79b27d16ff93799269d");
    }

    #[test]
    fn seal_matches_reference() {
        let p = Keys::derive(CODE).seal(0x1122_3344, 5, &ramp());
        assert_eq!(hex(&p[16..32]), "5bc90188762e89d95219d8db6a4c9fe2", "GCM tag");
        assert_eq!(hex(&p[32..48]), "1fd651da1e844c9acb19be2a3ab9eb2c", "ciphertext");
    }

    #[test]
    fn open_round_trips_and_rejects_tampering() {
        let k = Keys::derive(CODE);
        let mut p = k.seal(7, 42, &ramp());
        let opened = k.open(&p).expect("opens");
        assert_eq!((opened.sequence, opened.samples), (42, ramp()));
        p[40] ^= 1; // flip one ciphertext bit
        assert_eq!(k.open(&p), Err(OpenError::Authentication));
        let mut q = k.seal(7, 42, &ramp());
        q[8] ^= 1; // the header is authenticated too
        assert_eq!(k.open(&q), Err(OpenError::Authentication));
    }

    #[test]
    fn wrong_code_cannot_open_or_discover() {
        let right = Keys::derive(CODE);
        let wrong = Keys::derive("7K2M9QXB4TPB");
        assert_eq!(wrong.open(&right.seal(1, 1, &ramp())), Err(OpenError::Authentication));
        assert!(!right.verify_discovery(&wrong.discovery([9; 8])));
        assert!(right.verify_discovery(&right.discovery([9; 8])));
    }

    #[test]
    fn codes_normalize_like_they_are_read_aloud() {
        assert_eq!(normalize_code("7k2m-9qxb-4tpa").unwrap(), CODE);
        assert_eq!(normalize_code(" 7K2M 9QXB 4TPA ").unwrap(), CODE);
        assert_eq!(normalize_code("O0Il-0000-0000").unwrap(), "001100000000");
        assert_eq!(normalize_code("ABCD-EFGH"), Err(CodeError::Length(8)));
        assert_eq!(normalize_code("ABCD-EFGH-JKMU"), Err(CodeError::Character('U')));
        assert_eq!(format_code(CODE), "7K2M-9QXB-4TPA");
    }
}
