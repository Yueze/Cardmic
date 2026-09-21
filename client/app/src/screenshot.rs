//! `cardmic screenshot`: pull a pixel-exact picture of the device screen.
//!
//! Sends `CARDMIC_SCREENSHOT [page]` to UDP 41234. The device answers from an
//! ephemeral port with CMSS packets: a 16-byte little-endian header
//! (`"CMSS"`, width, height, first row, row count, reserved) followed by
//! big-endian RGB565 pixels. Missing rows trigger a retry.

use crate::png;
use cardmic_core::protocol::PORT;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

const HEADER: usize = 16;

pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

pub fn capture(device: Ipv4Addr, page: Option<&str>) -> Result<Shot, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| e.to_string())?;
    let request = match page {
        Some(p) => format!("CARDMIC_SCREENSHOT {p}"),
        None => "CARDMIC_SCREENSHOT".to_string(),
    };
    let target = SocketAddr::from((device, PORT));

    for _attempt in 0..5 {
        socket.send_to(request.as_bytes(), target).map_err(|e| e.to_string())?;
        let mut size: Option<(u32, u32)> = None;
        let mut rgb = Vec::new();
        let mut have: Vec<bool> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buf = [0u8; 2048];
        while Instant::now() < deadline {
            let (n, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(_) => {
                    if !have.is_empty() {
                        break; // stream went quiet
                    }
                    continue;
                }
            };
            let SocketAddr::V4(from) = from else { continue };
            if *from.ip() != device || n < HEADER || &buf[..4] != b"CMSS" {
                continue;
            }
            let u16_at = |i: usize| u16::from_le_bytes([buf[i], buf[i + 1]]) as u32;
            let (w, h, y, rows) = (u16_at(4), u16_at(6), u16_at(8), u16_at(10));
            if size.is_none() {
                size = Some((w, h));
                rgb = vec![0u8; (w * h * 3) as usize];
                have = vec![false; h as usize];
            }
            if size != Some((w, h)) || y + rows > h || n < HEADER + (w * rows * 2) as usize {
                continue;
            }
            for r in 0..rows {
                for x in 0..w {
                    let i = HEADER + ((r * w + x) * 2) as usize;
                    let c = u16::from_be_bytes([buf[i], buf[i + 1]]);
                    let o = (((y + r) * w + x) * 3) as usize;
                    rgb[o] = (((c >> 11) & 0x1F) as u32 * 255 / 31) as u8;
                    rgb[o + 1] = (((c >> 5) & 0x3F) as u32 * 255 / 63) as u8;
                    rgb[o + 2] = ((c & 0x1F) as u32 * 255 / 31) as u8;
                }
                have[(y + r) as usize] = true;
            }
            if have.iter().all(|&b| b) {
                let (width, height) = size.unwrap();
                return Ok(Shot { width, height, rgb });
            }
        }
    }
    Err(format!(
        "no complete screenshot from {device}. Is Cardmic open on the device and on Wi-Fi?"
    ))
}

/// Nearest-neighbour upscale, so pixel art stays crisp.
pub fn scale(shot: &Shot, k: u32) -> Shot {
    let (w, h) = (shot.width * k, shot.height * k);
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let s = (((y / k) * shot.width + x / k) * 3) as usize;
            let d = ((y * w + x) * 3) as usize;
            rgb[d..d + 3].copy_from_slice(&shot.rgb[s..s + 3]);
        }
    }
    Shot { width: w, height: h, rgb }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut device: Option<Ipv4Addr> = None;
    let mut page: Option<String> = None;
    let mut out = String::from("cardmic.png");
    let mut k = 3u32;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--page" => page = Some(it.next().ok_or("--page needs a name")?.clone()),
            "-o" | "--out" => out = it.next().ok_or("--out needs a file")?.clone(),
            "--scale" => {
                let v = it.next().ok_or("--scale needs a number")?;
                k = v.parse().ok().filter(|&k| (1..=8).contains(&k)).ok_or("--scale is 1..8")?;
            }
            other if device.is_none() => {
                device = Some(other.parse().map_err(|_| format!("{other:?} is not an IPv4 address"))?)
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let device = device.ok_or("usage: cardmic screenshot DEVICE_IP [--page main|settings|info|about] [-o FILE] [--scale N]")?;
    let shot = scale(&capture(device, page.as_deref())?, k);
    std::fs::write(&out, png::encode_rgb(shot.width, shot.height, &shot.rgb))
        .map_err(|e| format!("{out}: {e}"))?;
    println!("{out}  {}x{}", shot.width, shot.height);
    Ok(())
}
