//! Session establishment (spec 4.1, 5.3, 6.2, 6.3).
//!
//! The device that shows the QR is always the TCP server (Noise responder); the device that
//! scans is the client (Noise initiator). Stream 0 is control; data streams 1..=16 each run
//! their own Noise handshake and must present the data token derived from the control
//! handshake hash.

use crate::consts::*;
use crate::error::{FliqError, Result};
use crate::kdf;
use crate::msg::{Ctrl, recv_ctrl, send_ctrl};
use crate::net::{self, Link, TcpLink};
use crate::noise::{self, SecureReader, SecureWriter};
use crate::qr::{QrPayload, ServerMode, WifiInfo, now_unix};
use crate::stats::StageStats;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

const KIND_CONTROL: u8 = 0;
const PROBE_REPLY: [u8; 4] = *b"FLIQ";
const KIND_DATA: u8 = 1;
/// Re-open the control stream inside a live session (spec 7.7). Carries the data token.
const KIND_RESUME: u8 = 2;

/// Cancels a session from any thread, and tracks the session's sockets for clean-up.
#[derive(Default)]
pub struct CancelToken {
    flag: AtomicBool,
    socks: Mutex<Vec<TcpStream>>,
}

impl CancelToken {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    /// Request cancellation. Running transfers notice within ~100 ms, tell the other
    /// device (so it fails immediately instead of waiting to reconnect), then close
    /// their sockets and delete partial files.
    pub fn cancel(&self) {
        self.flag.store(true, SeqCst);
    }
    /// Close every registered socket without marking the session cancelled
    /// (used once a transfer has finished, so no descriptors linger).
    pub fn close_all(&self) {
        for s in self.socks.lock().unwrap().drain(..) {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(SeqCst)
    }
    pub fn register(&self, s: &TcpStream) {
        if let Ok(c) = s.try_clone() {
            if self.is_cancelled() {
                let _ = c.shutdown(std::net::Shutdown::Both);
            }
            let mut v = self.socks.lock().unwrap();
            v.retain(|x| x.peer_addr().is_ok());
            v.push(c);
        }
    }
}

/// An authenticated, encrypted control stream (stream 0).
pub struct Control {
    pub reader: SecureReader<TcpStream>,
    pub writer: SecureWriter<TcpStream>,
    pub verification_code: String,
    pub peer_name: String,
    pub peer: SocketAddr,
    pub(crate) stream: TcpStream,
}

impl Control {
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// One authenticated data stream.
pub struct DataConn {
    pub reader: SecureReader<TcpStream>,
    pub writer: SecureWriter<TcpStream>,
    pub index: u16,
    pub(crate) stream: TcpStream,
    _slot: Option<SlotGuard>,
}

impl DataConn {
    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// Frees a data-stream slot when the stream ends.
struct SlotGuard {
    slots: Arc<Mutex<[bool; MAX_DATA_STREAMS as usize + 1]>>,
    idx: u16,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.slots.lock().unwrap()[self.idx as usize] = false;
    }
}

fn split(
    stream: TcpStream,
    hs: &noise::Handshaken,
    stats: Option<Arc<StageStats>>,
) -> Result<(SecureReader<TcpStream>, SecureWriter<TcpStream>, TcpStream)> {
    let r = stream.try_clone()?;
    let w = stream.try_clone()?;
    Ok((
        SecureReader::new(r, hs.transport.clone(), stats.clone()),
        SecureWriter::new(w, hs.transport.clone(), stats),
        stream,
    ))
}

fn hello(name: &str) -> Ctrl {
    Ctrl::Hello {
        version: PROTOCOL_VERSION,
        ciphers: vec!["ChaChaPoly".into()],
        device_name: name.chars().take(MAX_DEVICE_NAME_LEN).collect(),
        os: std::env::consts::OS.into(),
    }
}

// ---------------------------------------------------------------- server

pub struct ServerConfig {
    pub mode: ServerMode,
    pub device_name: String,
    pub link: TcpLink,
    pub wifi: Option<WifiInfo>,
    pub ttl_secs: u64,
}

impl ServerConfig {
    pub fn new(mode: ServerMode, device_name: &str) -> Self {
        ServerConfig {
            mode,
            device_name: device_name.into(),
            link: TcpLink::default(),
            wifi: None,
            ttl_secs: QR_TTL_SECS,
        }
    }
}

struct ServerShared {
    sid: Vec<u8>,
    sk: Zeroizing<Vec<u8>>,
    /// Destroyed after 3 failed handshakes or when the session ends.
    psk: Mutex<Option<Zeroizing<Vec<u8>>>>,
    exp: u64,
    failed: Mutex<u32>,
    control_taken: AtomicBool,
    token: Mutex<Option<Zeroizing<[u8; 32]>>>,
    slots: Arc<Mutex<[bool; MAX_DATA_STREAMS as usize + 1]>>,
    closed: AtomicBool,
    mode: ServerMode,
    name: String,
    cancel: Arc<CancelToken>,
    stats: Arc<StageStats>,
    /// (verification code, peer name) of the established session, reused on resume.
    session_info: Mutex<Option<(String, String)>>,
}

impl ServerShared {
    fn fail(&self, control_tx: &Sender<Result<Control>>) {
        let mut f = self.failed.lock().unwrap();
        *f += 1;
        if *f >= MAX_FAILED_HANDSHAKES {
            *self.psk.lock().unwrap() = None;
            if !self.control_taken.load(SeqCst) {
                let _ = control_tx.send(Err(FliqError::Locked));
            }
        }
    }
}

/// A listening session. Show `qr()` to the other device, then call `wait_control`.
pub struct Server {
    qr: QrPayload,
    shared: Arc<ServerShared>,
    control_rx: Receiver<Result<Control>>,
    data_rx: Receiver<DataConn>,
    resume_rx: Receiver<Control>,
}

impl Server {
    pub fn start(cfg: ServerConfig, cancel: Arc<CancelToken>, stats: Arc<StageStats>) -> Result<Server> {
        let listener = cfg.link.listen()?;
        let port = listener.local_addr()?.port();
        let (sk, pk) = noise::generate_static_keypair()?;
        let mut sid = vec![0u8; 16];
        let mut psk = Zeroizing::new(vec![0u8; 16]);
        getrandom::getrandom(&mut sid).map_err(|e| FliqError::Io(e.into()))?;
        getrandom::getrandom(&mut psk).map_err(|e| FliqError::Io(e.into()))?;
        let exp = now_unix() + cfg.ttl_secs;
        let qr = QrPayload {
            v: PROTOCOL_VERSION,
            sid: sid.clone(),
            pk,
            psk: psk.to_vec(),
            ip: cfg.link.local_addresses().iter().map(|i| i.to_string()).collect(),
            port,
            wifi: cfg.wifi,
            mode: cfg.mode.as_str().into(),
            exp,
            name: cfg.device_name.chars().take(MAX_DEVICE_NAME_LEN).collect(),
        };
        let shared = Arc::new(ServerShared {
            sid,
            sk,
            psk: Mutex::new(Some(psk)),
            exp,
            failed: Mutex::new(0),
            control_taken: AtomicBool::new(false),
            token: Mutex::new(None),
            slots: Arc::new(Mutex::new([false; MAX_DATA_STREAMS as usize + 1])),
            closed: AtomicBool::new(false),
            mode: cfg.mode,
            name: qr.name.clone(),
            cancel,
            stats,
            session_info: Mutex::new(None),
        });
        let (ctx, crx) = bounded(4);
        let (dtx, drx) = unbounded();
        let (rtx, rrx) = unbounded();
        let sh = shared.clone();
        let tx = Txs { control: ctx, data: dtx, resume: rtx };
        std::thread::spawn(move || accept_loop(listener, sh, tx));
        Ok(Server { qr, shared, control_rx: crx, data_rx: drx, resume_rx: rrx })
    }

    pub fn qr(&self) -> &QrPayload {
        &self.qr
    }

    pub fn failed_handshakes(&self) -> u32 {
        *self.shared.failed.lock().unwrap()
    }

    /// Wait for the scanning device. Fails on expiry, lock-out or cancel.
    pub fn wait_control(&self) -> Result<Control> {
        loop {
            if self.shared.cancel.is_cancelled() {
                return Err(FliqError::Cancelled);
            }
            match self.control_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(r) => return r,
                Err(_) => {
                    if now_unix() > self.shared.exp {
                        self.close();
                        return Err(FliqError::QrExpired);
                    }
                }
            }
        }
    }

    pub fn data_conns(&self) -> Receiver<DataConn> {
        self.data_rx.clone()
    }

    /// Resumed control streams (after the client re-dialed). Used by the engine.
    pub fn resumed_controls(&self) -> Receiver<Control> {
        self.resume_rx.clone()
    }

    /// End the session: stop listening and destroy the PSK.
    pub fn close(&self) {
        self.shared.closed.store(true, SeqCst);
        *self.shared.psk.lock().unwrap() = None;
        *self.shared.token.lock().unwrap() = None;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Clone)]
struct Txs {
    control: Sender<Result<Control>>,
    data: Sender<DataConn>,
    resume: Sender<Control>,
}

fn accept_loop(l: TcpListener, sh: Arc<ServerShared>, tx: Txs) {
    let _ = l.set_nonblocking(true);
    while !sh.closed.load(SeqCst) && !sh.cancel.is_cancelled() {
        match l.accept() {
            Ok((s, _)) => {
                let _ = s.set_nonblocking(false);
                let (sh, tx) = (sh.clone(), tx.clone());
                std::thread::spawn(move || handle_conn(s, sh, tx));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn handle_conn(s: TcpStream, sh: Arc<ServerShared>, tx: Txs) {
    let (ctx, dtx) = (&tx.control, &tx.data);
    net::tune(&s);
    let _ = s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)));
    // Reachability probe (an empty first frame): echo it and close. Never counts as a
    // failed handshake, so the self-check cannot lock the session.
    let mut first = [0u8; 2];
    if matches!(s.peek(&mut first), Ok(2)) && first == [0, 0] {
        let mut s2 = &s;
        let _ = std::io::Read::read_exact(&mut s2, &mut first);
        let _ = std::io::Write::write_all(&mut s2, &PROBE_REPLY);
        return;
    }
    let Some(psk) = sh.psk.lock().unwrap().clone() else { return };
    if !sh.control_taken.load(SeqCst) && now_unix() > sh.exp {
        return;
    }
    let mut hs_stream = match s.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let hs = match noise::responder(&mut hs_stream, &sh.sid, &sh.sk, &psk) {
        Ok(h) => h,
        Err(_) => return sh.fail(ctx),
    };
    drop(psk);
    let peer = match s.peer_addr() {
        Ok(p) => p,
        Err(_) => return,
    };
    let (mut r, mut w, s) = match split(s, &hs, None) {
        Ok(x) => x,
        Err(_) => return,
    };
    // First encrypted message says which kind of stream this is. A replayed handshake
    // message cannot produce it, so failure here also counts as a failed handshake.
    let mut kind = [0u8; 1];
    if r.read_exact(&mut kind).is_err() {
        return sh.fail(ctx);
    }
    match kind[0] {
        KIND_CONTROL => {
            if sh.control_taken.swap(true, SeqCst) || now_unix() > sh.exp {
                return; // QR is single-use.
            }
            let peer_name = match recv_ctrl(&mut r) {
                Ok(Ctrl::Hello { version, device_name, .. }) if version == PROTOCOL_VERSION => device_name,
                _ => {
                    sh.control_taken.store(false, SeqCst);
                    return sh.fail(ctx);
                }
            };
            let role = match sh.mode {
                ServerMode::Send => "sender",
                ServerMode::Receive => "receiver",
            };
            if send_ctrl(&mut w, &hello(&sh.name)).is_err()
                || send_ctrl(&mut w, &Ctrl::Role { server_role: role.into() }).is_err()
            {
                return;
            }
            *sh.token.lock().unwrap() = Some(kdf::data_token(&hs.hash));
            *sh.session_info.lock().unwrap() = Some((kdf::verification_code(&hs.hash), peer_name.clone()));
            let _ = s.set_read_timeout(None);
            sh.cancel.register(&s);
            let ctrl = Control {
                reader: r,
                writer: w,
                verification_code: kdf::verification_code(&hs.hash),
                peer_name,
                peer,
                stream: s,
            };
            let _ = ctx.send(Ok(ctrl));
        }
        KIND_RESUME => {
            let mut tok = [0u8; 32];
            if r.read_exact(&mut tok).is_err() {
                return sh.fail(ctx);
            }
            let token_ok = sh.token.lock().unwrap().as_ref().is_some_and(|t| kdf::ct_eq(&t[..], &tok));
            let info = sh.session_info.lock().unwrap().clone();
            let (Some((code, peer_name)), true) = (info, token_ok) else {
                let _ = w.write(&[0]).and_then(|_| w.flush());
                return sh.fail(ctx);
            };
            if w.write(&[1]).and_then(|_| w.flush()).is_err() {
                return;
            }
            let _ = s.set_read_timeout(None);
            sh.cancel.register(&s);
            let _ =
                tx.resume.send(Control { reader: r, writer: w, verification_code: code, peer_name, peer, stream: s });
        }
        KIND_DATA => {
            let mut tok = [0u8; 34];
            if r.read_exact(&mut tok).is_err() {
                return sh.fail(ctx);
            }
            let idx = u16::from_be_bytes([tok[32], tok[33]]);
            let token_ok = sh.token.lock().unwrap().as_ref().is_some_and(|t| kdf::ct_eq(&t[..], &tok[..32]));
            let slot_ok = token_ok && (1..=MAX_DATA_STREAMS).contains(&idx) && {
                let mut slots = sh.slots.lock().unwrap();
                !std::mem::replace(&mut slots[idx as usize], true)
            };
            if !slot_ok {
                let _ = w.write(&[0]).and_then(|_| w.flush());
                return;
            }
            let guard = SlotGuard { slots: sh.slots.clone(), idx };
            if w.write(&[1]).and_then(|_| w.flush()).is_err() {
                return;
            }
            let _ = s.set_read_timeout(Some(Duration::from_millis(IO_TIMEOUT_MS)));
            sh.cancel.register(&s);
            r.set_stats(Some(sh.stats.clone()));
            w.set_stats(Some(sh.stats.clone()));
            sh.stats.streams_opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = dtx.send(DataConn { reader: r, writer: w, index: idx, stream: s, _slot: Some(guard) });
        }
        _ => sh.fail(ctx),
    }
}

// ---------------------------------------------------------------- client

/// The scanning side. Holds what it needs to open (and re-open) data streams.
pub struct Client {
    qr: QrPayload,
    addr: SocketAddr,
    code: String,
    peer_name: String,
    token: Zeroizing<[u8; 32]>,
    cancel: Arc<CancelToken>,
    stats: Arc<StageStats>,
}

impl Client {
    /// Validate the QR, connect to the first reachable address, run the handshake and
    /// exchange Hello/Role. Joining a Wi-Fi network (if `qr.wifi` is set) is done by the
    /// platform layer *before* calling this.
    pub fn connect(
        qr: &QrPayload,
        device_name: &str,
        cancel: Arc<CancelToken>,
        stats: Arc<StageStats>,
    ) -> Result<(Client, Control)> {
        qr.validate()?;
        qr.check_fresh(now_unix())?;
        let mode = qr.server_mode()?;
        let s = TcpLink::default().connect(&qr.ipv4s(), qr.port)?;
        let addr = s.peer_addr()?;
        let _ = s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)));
        let mut hs_stream = s.try_clone()?;
        let hs = noise::initiator(&mut hs_stream, &qr.sid, &qr.pk, &qr.psk)?;
        let (mut r, mut w, s) = split(s, &hs, None)?;
        w.write(&[KIND_CONTROL])?;
        send_ctrl(&mut w, &hello(device_name))?;
        let peer_name = match recv_ctrl(&mut r).map_err(|_| FliqError::Handshake)? {
            Ctrl::Hello { version, device_name, .. } if version == PROTOCOL_VERSION => device_name,
            _ => return Err(FliqError::Handshake),
        };
        let expected = match mode {
            ServerMode::Send => "sender",
            ServerMode::Receive => "receiver",
        };
        match recv_ctrl(&mut r)? {
            Ctrl::Role { server_role } if server_role == expected => {}
            _ => return Err(FliqError::Protocol("role mismatch")),
        }
        let _ = s.set_read_timeout(None);
        cancel.register(&s);
        let token = kdf::data_token(&hs.hash);
        let code = kdf::verification_code(&hs.hash);
        let ctrl = Control {
            reader: r,
            writer: w,
            verification_code: code.clone(),
            peer_name: peer_name.clone(),
            peer: addr,
            stream: s,
        };
        Ok((Client { qr: qr.clone(), addr, code, peer_name, token, cancel, stats }, ctrl))
    }

    /// Re-open the control stream after it dropped (same session, same token).
    pub fn resume_control(&self) -> Result<Control> {
        self.resume_control_with_token(&self.token[..])
    }

    #[doc(hidden)]
    pub fn resume_control_with_token(&self, token: &[u8]) -> Result<Control> {
        let s = net::connect_one(self.addr)?;
        let _ = s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)));
        let mut hs_stream = s.try_clone()?;
        let hs = noise::initiator(&mut hs_stream, &self.qr.sid, &self.qr.pk, &self.qr.psk)?;
        let (mut r, mut w, s) = split(s, &hs, None)?;
        let mut first = vec![KIND_RESUME];
        first.extend_from_slice(token);
        w.write(&first)?;
        w.flush()?;
        let mut ok = [0u8; 1];
        r.read_exact(&mut ok).map_err(|_| FliqError::Protocol("resume refused"))?;
        if ok[0] != 1 {
            return Err(FliqError::Protocol("resume refused"));
        }
        let _ = s.set_read_timeout(None);
        self.cancel.register(&s);
        Ok(Control {
            reader: r,
            writer: w,
            verification_code: self.code.clone(),
            peer_name: self.peer_name.clone(),
            peer: self.addr,
            stream: s,
        })
    }

    /// Open data stream `idx` (1..=16).
    pub fn dial_data(&self, idx: u16) -> Result<DataConn> {
        self.dial_data_with_token(idx, &self.token[..])
    }

    #[doc(hidden)]
    pub fn dial_data_with_token(&self, idx: u16, token: &[u8]) -> Result<DataConn> {
        let s = net::connect_one(self.addr)?;
        let _ = s.set_read_timeout(Some(Duration::from_millis(HANDSHAKE_TIMEOUT_MS)));
        let mut hs_stream = s.try_clone()?;
        let hs = noise::initiator(&mut hs_stream, &self.qr.sid, &self.qr.pk, &self.qr.psk)?;
        let (mut r, mut w, s) = split(s, &hs, Some(self.stats.clone()))?;
        let mut first = vec![KIND_DATA];
        first.extend_from_slice(token);
        first.extend_from_slice(&idx.to_be_bytes());
        w.write(&first)?;
        w.flush()?;
        let mut ok = [0u8; 1];
        r.read_exact(&mut ok).map_err(|_| FliqError::Protocol("data stream refused"))?;
        if ok[0] != 1 {
            return Err(FliqError::Protocol("data stream refused"));
        }
        let _ = s.set_read_timeout(Some(Duration::from_millis(IO_TIMEOUT_MS)));
        self.cancel.register(&s);
        self.stats.streams_opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(DataConn { reader: r, writer: w, index: idx, stream: s, _slot: None })
    }
}

// Debug output never includes keys, tokens or the PSK.
impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control").field("peer", &self.peer).finish_non_exhaustive()
    }
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("addr", &self.addr).finish_non_exhaustive()
    }
}
impl std::fmt::Debug for DataConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataConn").field("index", &self.index).finish_non_exhaustive()
    }
}

/// Reachability check for one address of a listening session (spec 8.2 self-connect test).
/// Sends an empty frame and expects the session's probe reply. Does not use the PSK.
pub fn probe(addr: SocketAddr, timeout: Duration) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = TcpStream::connect_timeout(&addr, timeout) else { return false };
    let _ = s.set_read_timeout(Some(timeout));
    let mut r = [0u8; 4];
    s.write_all(&[0, 0]).is_ok() && s.read_exact(&mut r).is_ok() && r == PROBE_REPLY
}

impl Server {
    /// Probe every advertised address. Returns (reachable, unreachable).
    ///
    /// Note: on Windows, connections from this PC to its own address are not filtered by the
    /// inbound firewall, so a pass here proves the listener and addresses are good but cannot
    /// prove that other devices are allowed in. The app pairs this with a firewall-rule check.
    pub fn self_check(&self) -> (Vec<String>, Vec<String>) {
        let mut ok = vec![];
        let mut bad = vec![];
        for ip in &self.qr.ip {
            let addr = SocketAddr::new(
                ip.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                self.qr.port,
            );
            if probe(addr, Duration::from_millis(1000)) { ok.push(ip.clone()) } else { bad.push(ip.clone()) }
        }
        (ok, bad)
    }
}
