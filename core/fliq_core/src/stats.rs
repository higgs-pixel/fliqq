//! Per-stage timing counters for the benchmark and the bottleneck hint.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

#[derive(Default, Debug)]
pub struct StageStats {
    pub read_ns: AtomicU64,
    pub hash_ns: AtomicU64,
    pub encrypt_ns: AtomicU64,
    pub net_send_ns: AtomicU64,
    pub net_recv_ns: AtomicU64,
    pub decrypt_ns: AtomicU64,
    pub verify_ns: AtomicU64,
    pub write_ns: AtomicU64,
    pub payload_bytes: AtomicU64,
    pub chunks_resent: AtomicU64,
    pub streams_opened: AtomicU64,
}

pub struct Timer(Instant);

impl Timer {
    pub fn start() -> Self {
        Timer(Instant::now())
    }
    pub fn stop(self, into: &AtomicU64) {
        into.fetch_add(self.0.elapsed().as_nanos() as u64, Relaxed);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bottleneck {
    Link,
    Storage,
    Cpu,
    Unknown,
}

impl Bottleneck {
    pub fn as_str(self) -> &'static str {
        match self {
            Bottleneck::Link => "link",
            Bottleneck::Storage => "storage",
            Bottleneck::Cpu => "cpu",
            Bottleneck::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StageSnapshot {
    pub read_s: f64,
    pub hash_s: f64,
    pub encrypt_s: f64,
    pub net_send_s: f64,
    pub net_recv_s: f64,
    pub decrypt_s: f64,
    pub verify_s: f64,
    pub write_s: f64,
    pub payload_bytes: u64,
    pub chunks_resent: u64,
    pub streams_opened: u64,
}

impl StageStats {
    pub fn snapshot(&self) -> StageSnapshot {
        let s = |a: &AtomicU64| a.load(Relaxed) as f64 / 1e9;
        StageSnapshot {
            read_s: s(&self.read_ns),
            hash_s: s(&self.hash_ns),
            encrypt_s: s(&self.encrypt_ns),
            net_send_s: s(&self.net_send_ns),
            net_recv_s: s(&self.net_recv_ns),
            decrypt_s: s(&self.decrypt_ns),
            verify_s: s(&self.verify_ns),
            write_s: s(&self.write_ns),
            payload_bytes: self.payload_bytes.load(Relaxed),
            chunks_resent: self.chunks_resent.load(Relaxed),
            streams_opened: self.streams_opened.load(Relaxed),
        }
    }
}

impl StageSnapshot {
    /// Heuristic for one side of the transfer: the stage group with the most busy time.
    /// Network time includes waiting on the socket, so a dominant network share means the
    /// link (or the far side) is the limit.
    pub fn bottleneck(&self) -> Bottleneck {
        let cpu = self.hash_s + self.encrypt_s + self.decrypt_s + self.verify_s;
        let disk = self.read_s + self.write_s;
        let net = self.net_send_s + self.net_recv_s;
        let max = cpu.max(disk).max(net);
        if max <= 0.0 {
            Bottleneck::Unknown
        } else if max == net {
            Bottleneck::Link
        } else if max == disk {
            Bottleneck::Storage
        } else {
            Bottleneck::Cpu
        }
    }
}
