//! TCP plumbing: address selection (spec 4.4), port binding (4.5), socket tuning (7.2),
//! and happy-eyeballs connect (4.3). Implements the `Link` abstraction (7.6).

use crate::consts::*;
use crate::error::{FliqError, Result};
use socket2::{Domain, Protocol, SockRef, Socket, TcpKeepalive, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// Transport abstraction so USB tethering / Wi-Fi Direct can be added later without
/// touching the protocol or engine. v1 has one implementation: plain TCP over IPv4.
pub trait Link: Send + Sync {
    fn listen(&self) -> Result<TcpListener>;
    fn connect(&self, ips: &[Ipv4Addr], port: u16) -> Result<TcpStream>;
    fn local_addresses(&self) -> Vec<Ipv4Addr>;
}

#[derive(Clone, Debug, Default)]
pub struct TcpLink {
    /// Address of an interface we created (hotspot/AP) to list first, if any.
    pub preferred: Option<Ipv4Addr>,
    /// Include 127.0.0.1 (tests and localhost benchmarks only).
    pub include_loopback: bool,
}

impl Link for TcpLink {
    fn listen(&self) -> Result<TcpListener> {
        bind_random_port()
    }
    fn connect(&self, ips: &[Ipv4Addr], port: u16) -> Result<TcpStream> {
        connect_any(ips, port)
    }
    fn local_addresses(&self) -> Vec<Ipv4Addr> {
        local_ipv4s(self.preferred, self.include_loopback)
    }
}

const VIRTUAL_HINTS: &[&str] = &[
    "vmware",
    "virtualbox",
    "vbox",
    "hyper-v",
    "vethernet",
    "wsl",
    "docker",
    "br-",
    "veth",
    "tun",
    "tap",
    "utun",
    "tailscale",
    "zerotier",
    "wg",
    "vpn",
    "virbr",
    "lxc",
];

fn rank(ip: Ipv4Addr, name: &str, preferred: Option<Ipv4Addr>) -> u8 {
    let lname = name.to_ascii_lowercase();
    if Some(ip) == preferred {
        0
    } else if ip.is_link_local() {
        4
    } else if VIRTUAL_HINTS.iter().any(|h| lname.contains(h)) {
        3
    } else if ip.is_private() {
        1
    } else {
        2
    }
}

/// Enumerate usable local IPv4 addresses in QR priority order.
pub fn local_ipv4s(preferred: Option<Ipv4Addr>, include_loopback: bool) -> Vec<Ipv4Addr> {
    let mut v: Vec<(u8, Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => {
                Some((rank(ip, &i.name, preferred), ip))
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|(r, _)| *r);
    v.dedup_by_key(|(_, ip)| *ip);
    let mut out: Vec<Ipv4Addr> = v.into_iter().map(|(_, ip)| ip).take(15).collect();
    if include_loopback || out.is_empty() {
        out.push(Ipv4Addr::LOCALHOST);
    }
    out
}

pub fn tune(s: &TcpStream) {
    let _ = s.set_nodelay(true);
    let r = SockRef::from(s);
    let _ = r.set_send_buffer_size(SOCKET_BUF_BYTES);
    let _ = r.set_recv_buffer_size(SOCKET_BUF_BYTES);
    let ka = TcpKeepalive::new().with_time(Duration::from_secs(10));
    let _ = r.set_tcp_keepalive(&ka);
    let _ = s.set_write_timeout(Some(Duration::from_millis(IO_TIMEOUT_MS)));
}

/// Bind 0.0.0.0 on a random free port in 49152-65535.
pub fn bind_random_port() -> Result<TcpListener> {
    for _ in 0..64 {
        let mut b = [0u8; 2];
        getrandom::getrandom(&mut b).map_err(|e| FliqError::Io(e.into()))?;
        let port = 49152 + (u16::from_be_bytes(b) % (65535 - 49152 + 1));
        let sock = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        // Set the receive buffer before listen so accepted sockets get a large window.
        let _ = sock.set_recv_buffer_size(SOCKET_BUF_BYTES);
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port));
        if sock.bind(&addr.into()).is_ok() && sock.listen(64).is_ok() {
            return Ok(sock.into());
        }
    }
    Err(FliqError::Io(std::io::Error::other("no free port")))
}

/// Try every address in parallel; keep the first that connects (3 s each).
pub fn connect_any(ips: &[Ipv4Addr], port: u16) -> Result<TcpStream> {
    let (tx, rx) = mpsc::channel();
    for &ip in ips {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let addr = SocketAddr::V4(SocketAddrV4::new(ip, port));
            let r = TcpStream::connect_timeout(&addr, Duration::from_millis(CONNECT_TIMEOUT_MS));
            let _ = tx.send(r);
        });
    }
    drop(tx);
    if let Some(s) = rx.into_iter().flatten().next() {
        tune(&s);
        return Ok(s);
    }
    Err(FliqError::Unreachable)
}

pub fn connect_one(addr: SocketAddr) -> Result<TcpStream> {
    let s = TcpStream::connect_timeout(&addr, Duration::from_millis(CONNECT_TIMEOUT_MS))
        .map_err(|_| FliqError::Unreachable)?;
    tune(&s);
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranks_virtual_and_link_local_last() {
        assert_eq!(rank("192.168.1.2".parse().unwrap(), "wlan0", None), 1);
        assert_eq!(rank("172.20.0.2".parse().unwrap(), "docker0", None), 3);
        assert_eq!(rank("169.254.3.3".parse().unwrap(), "eth1", None), 4);
        let p: Ipv4Addr = "192.168.43.1".parse().unwrap();
        assert_eq!(rank(p, "ap0", Some(p)), 0);
    }
    #[test]
    fn port_range() {
        let l = bind_random_port().unwrap();
        assert!(l.local_addr().unwrap().port() >= 49152);
    }
    #[test]
    fn happy_eyeballs_skips_dead_address() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // 127.0.0.2 is also loopback on Linux but nothing listens on that port there.
        let s = connect_any(&["10.255.255.1".parse().unwrap(), Ipv4Addr::LOCALHOST], port).unwrap();
        assert_eq!(s.peer_addr().unwrap().port(), port);
    }
}
