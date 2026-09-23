//! Network side: discovery keepalive and packet receipt.
//!
//! One socket does both jobs, and that is not a style choice. The device
//! streams to the source address *and port* of the discovery packet, so
//! discovery must leave from the same socket that receives audio. Sending it
//! from a second socket makes the device stream to a port nothing is reading,
//! while its screen reports a connected receiver.

use cardmic_core::protocol::PORT;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

/// Bind the receive socket.
///
/// Binds [`PORT`] so that, when discovery is sent from it, the device's reply
/// lands on the port the prototype and firmware expect.
pub fn bind() -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PORT))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(10)))?;
    Ok(socket)
}

/// Where to send discovery.
///
/// One subnet-directed broadcast address (`ip | !netmask`) per usable IPv4
/// interface, plus the limited broadcast address. Per-interface addresses are
/// what survive multi-homed machines and macOS routing; the limited broadcast
/// alone is not a substitute. `explicit` adds a unicast target, for networks
/// that block broadcast or for pointing at a synthetic device.
pub fn discovery_targets(explicit: &[SocketAddr]) -> Vec<SocketAddr> {
    let mut targets: Vec<SocketAddr> = Vec::new();
    for &addr in explicit {
        push_unique(&mut targets, addr);
    }

    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            if !iface.is_oper_up() || iface.is_p2p() {
                continue;
            }
            let if_addrs::IfAddr::V4(v4) = iface.addr else { continue };
            if v4.ip.is_loopback() || v4.ip.is_link_local() {
                continue;
            }
            let broadcast = v4
                .broadcast
                .unwrap_or_else(|| Ipv4Addr::from(u32::from(v4.ip) | !u32::from(v4.netmask)));
            push_unique(&mut targets, SocketAddrV4::new(broadcast, PORT).into());
        }
    }

    push_unique(&mut targets, SocketAddrV4::new(Ipv4Addr::BROADCAST, PORT).into());
    targets
}

fn push_unique(targets: &mut Vec<SocketAddr>, addr: SocketAddr) {
    if !targets.contains(&addr) {
        targets.push(addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_targets_come_first_and_are_not_duplicated() {
        let synth: SocketAddr = "127.0.0.1:41235".parse().unwrap();
        let targets = discovery_targets(&[synth, synth]);
        assert_eq!(targets[0], synth);
        assert_eq!(targets.iter().filter(|&&t| t == synth).count(), 1);
    }

    #[test]
    fn always_includes_the_limited_broadcast() {
        let limited: SocketAddr = SocketAddrV4::new(Ipv4Addr::BROADCAST, PORT).into();
        assert!(discovery_targets(&[]).contains(&limited));
    }

    #[test]
    fn subnet_broadcast_is_ip_or_inverted_mask() {
        // The formula the prototype uses per adapter.
        let ip = Ipv4Addr::new(192, 168, 165, 122);
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        let b = Ipv4Addr::from(u32::from(ip) | !u32::from(mask));
        assert_eq!(b, Ipv4Addr::new(192, 168, 165, 255));
    }
}
