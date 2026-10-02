//! Transfer engine (spec 7.1-7.3, 7.7): bounded streaming pipeline over N data streams.
//!
//! Sender:   [disk reader] -> bounded work queue -> [hash + encrypt + send, one per stream]
//! Receiver: [recv + decrypt, one per stream] -> verify chunk hash -> positional write
//!
//! Memory is bounded by a fixed pool of 4 MiB chunk buffers (INFLIGHT_BUDGET / CHUNK_SIZE)
//! on the sender, plus one chunk buffer and ~1.3 MiB of record buffers per stream.
//!
//! Recovery within a session: when a data stream dies, the chunk it was sending is
//! requeued, the client re-dials a replacement stream, and the receiver asks for any chunk
//! still missing (Ack bitmap) once the sender has announced FileDone. Duplicate chunks are
//! accepted only if their hash equals the chunk already written.

use crate::bitmap::Bitmap;
use crate::consts::*;
use crate::error::{FliqError, Result};
use crate::fsutil;
use crate::msg::*;
use crate::noise::io_err;
use crate::sanitize;
use crate::session::{CancelToken, Client, Control, DataConn};

use crate::stats::{Bottleneck, StageSnapshot, StageStats, Timer};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(100);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

// ------------------------------------------------------------------ public types

/// Where chunk bytes come from. `File` covers both paths and Android SAF descriptors
/// (the platform layer wraps the raw fd in a `File`).
pub enum Source {
    File(File),
    /// Benchmark only: generated data, no disk read.
    Synthetic(Arc<Vec<u8>>),
}

impl Source {
    fn read_chunk(&self, idx: u32, buf: &mut [u8]) -> std::io::Result<()> {
        match self {
            Source::File(f) => fsutil::read_exact_at(f, buf, idx as u64 * CHUNK_SIZE),
            Source::Synthetic(block) => {
                buf.copy_from_slice(&block[..buf.len()]);
                for (b, x) in buf.iter_mut().zip((idx as u64 + 1).to_le_bytes()) {
                    *b ^= x;
                }
                Ok(())
            }
        }
    }
}

/// A 4 MiB pseudo-random block for synthetic sources.
pub fn synthetic_block(seed: u64) -> Arc<Vec<u8>> {
    let mut v = vec![0u8; CHUNK_SIZE as usize];
    blake3::Hasher::new().update(&seed.to_le_bytes()).finalize_xof().fill(&mut v);
    Arc::new(v)
}

pub struct SendItem {
    pub source: Source,
    pub name: String,
    pub size: u64,
    pub mtime: u64,
}

impl SendItem {
    pub fn from_path(p: &Path) -> Result<Self> {
        let f = File::open(p)?;
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        Self::from_file(f, name)
    }
    /// For descriptors handed over by the platform (Android SAF): `name` comes from the
    /// content provider, `f` must be a seekable regular file.
    pub fn from_file(f: File, name: String) -> Result<Self> {
        let md = f.metadata()?;
        if !md.is_file() {
            return Err(FliqError::Limit("Only regular local files can be sent.".into()));
        }
        let mtime = fsutil::mtime_unix(&f);
        Ok(SendItem { source: Source::File(f), name, size: md.len(), mtime })
    }
}

#[derive(Clone)]
pub struct EngineOptions {
    /// Data streams the client keeps open (2..=16).
    pub streams: u16,
    pub inflight_budget: usize,
    pub limits: Limits,
    pub cancel: Arc<CancelToken>,
    pub stats: Arc<StageStats>,
    /// How long the session waits for progress or a reconnect before failing (spec 7.7: 60 s).
    pub linger: Duration,
    #[doc(hidden)]
    pub fault: Option<Arc<Fault>>,
}

impl EngineOptions {
    pub fn new(cancel: Arc<CancelToken>, stats: Arc<StageStats>) -> Self {
        EngineOptions {
            streams: DEFAULT_STREAMS,
            inflight_budget: DEFAULT_INFLIGHT_BUDGET_BYTES,
            limits: Limits::default(),
            cancel,
            stats,
            linger: Duration::from_millis(SESSION_LINGER_MS),
            fault: None,
        }
    }
    fn pool_buffers(&self) -> usize {
        (self.inflight_budget.max(MIN_INFLIGHT_BUDGET_BYTES) / CHUNK_SIZE as usize).max(1)
    }
}

/// Test hook for fault injection on the sending side.
#[doc(hidden)]
#[derive(Default)]
pub struct Fault {
    /// Kill one data stream after this many chunks (0 = off).
    pub kill_stream_after_chunks: u64,
    /// Kill the control stream after this many chunks (0 = off).
    pub kill_control_after_chunks: u64,
    pub fired: AtomicBool,
    pub control_fired: AtomicBool,
}

/// How streams are obtained: the server receives data streams and resumed control
/// streams; the client dials them.
pub enum Plane {
    Server { data: Receiver<DataConn>, resume: Receiver<Control> },
    Client(Arc<Client>),
}

impl Plane {
    pub fn server(s: &crate::session::Server) -> Plane {
        Plane::Server { data: s.data_conns(), resume: s.resumed_controls() }
    }
    fn split(self) -> (DataPlane, Reconn) {
        match self {
            Plane::Server { data, resume } => (DataPlane::Server(data), Reconn::Server(resume)),
            Plane::Client(c) => (DataPlane::Client(c.clone()), Reconn::Client(c)),
        }
    }
}

enum DataPlane {
    Server(Receiver<DataConn>),
    Client(Arc<Client>),
}

enum Reconn {
    Server(Receiver<Control>),
    Client(Arc<Client>),
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub bytes_done: u64,
    pub total: u64,
    pub mbps: f64,
    pub eta_secs: Option<f64>,
    pub streams: u16,
    pub bottleneck: Bottleneck,
}

#[derive(Clone, Debug)]
pub enum Event {
    Progress(Progress),
    /// Receiver: file verified and finalized. Sender: peer confirmed it (Move may now delete).
    FileVerified {
        id: u32,
    },
    /// The control connection dropped; trying to restore it (UI: "Connection lost. Reconnecting…").
    Reconnecting,
    Reconnected,
}

#[derive(Clone, Debug)]
pub struct FileResult {
    pub id: u32,
    pub size: u64,
    /// Final path on the receiver; None on the sender.
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub bytes: u64,
    pub secs: f64,
    /// Decimal megabytes per second (1 MB = 1,000,000 bytes).
    pub mbps: f64,
    pub files: Vec<FileResult>,
    pub stages: StageSnapshot,
    pub chunks_resent: u64,
    /// How many times the control stream was re-established during the session.
    pub control_reconnects: u32,
}

pub struct OfferInfo {
    pub verification_code: String,
    pub peer_name: String,
    /// (sanitized name, size)
    pub files: Vec<(String, u64)>,
    pub total: u64,
    pub free_space: u64,
    pub fat32_warning: bool,
}

// ------------------------------------------------------------------ stream manager

struct Streams {
    live: Arc<AtomicU16>,
    done: Arc<AtomicBool>,
    socks: Arc<Mutex<Vec<std::net::TcpStream>>>,
    handle: Option<JoinHandle<()>>,
}

impl Streams {
    fn start<F>(plane: DataPlane, target: u16, cancel: Arc<CancelToken>, worker: F) -> Self
    where
        F: Fn(DataConn) + Send + Sync + 'static,
    {
        let live = Arc::new(AtomicU16::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let worker = Arc::new(worker);
        let socks: Arc<Mutex<Vec<std::net::TcpStream>>> = Arc::default();
        let (l2, d2, s2) = (live.clone(), done.clone(), socks.clone());
        let handle = std::thread::spawn(move || {
            let mut threads: Vec<JoinHandle<()>> = vec![];
            let spawn = |c: DataConn, threads: &mut Vec<JoinHandle<()>>, on_exit: Box<dyn FnOnce() + Send>| {
                if let Ok(k) = c.stream.try_clone() {
                    s2.lock().unwrap().push(k);
                }
                l2.fetch_add(1, SeqCst);
                let (w, l3) = (worker.clone(), l2.clone());
                threads.push(std::thread::spawn(move || {
                    w(c);
                    l3.fetch_sub(1, SeqCst);
                    on_exit();
                }));
            };
            match plane {
                DataPlane::Server(rx) => {
                    while !d2.load(SeqCst) && !cancel.is_cancelled() {
                        if let Ok(c) = rx.recv_timeout(TICK) {
                            spawn(c, &mut threads, Box::new(|| {}));
                        }
                    }
                }
                DataPlane::Client(client) => {
                    let slots = Arc::new(Mutex::new([false; MAX_DATA_STREAMS as usize + 1]));
                    let mut next = 1u16;
                    while !d2.load(SeqCst) && !cancel.is_cancelled() {
                        if l2.load(SeqCst) >= target {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        let idx = {
                            let s = slots.lock().unwrap();
                            (0..MAX_DATA_STREAMS)
                                .map(|k| (next - 1 + k) % MAX_DATA_STREAMS + 1)
                                .find(|&i| !s[i as usize])
                        };
                        let Some(idx) = idx else {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        };
                        next = idx % MAX_DATA_STREAMS + 1;
                        match client.dial_data(idx) {
                            Ok(c) => {
                                slots.lock().unwrap()[idx as usize] = true;
                                let s2 = slots.clone();
                                spawn(c, &mut threads, Box::new(move || s2.lock().unwrap()[idx as usize] = false));
                            }
                            Err(_) => std::thread::sleep(Duration::from_millis(200)),
                        }
                    }
                }
            }
            for t in threads {
                let _ = t.join();
            }
        });
        Streams { live, done, socks, handle: Some(handle) }
    }

    /// Error path: shut every data socket so blocked workers return immediately.
    fn abort(&self) {
        self.done.store(true, SeqCst);
        for k in self.socks.lock().unwrap().drain(..) {
            let _ = k.shutdown(std::net::Shutdown::Both);
        }
    }

    fn live(&self) -> u16 {
        self.live.load(SeqCst)
    }

    fn finish(&mut self) {
        self.done.store(true, SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.socks.lock().unwrap().clear();
    }
}

type CtrlReader = crate::noise::SecureReader<std::net::TcpStream>;
type CtrlWriter = crate::noise::SecureWriter<std::net::TcpStream>;
type Wrap<E> = fn(u32, Result<Ctrl>) -> E;

fn spawn_ctrl_reader<E: Send + 'static>(mut r: CtrlReader, tx: Sender<E>, wrap: Wrap<E>, generation: u32) {
    std::thread::spawn(move || {
        loop {
            let m = recv_ctrl(&mut r);
            let stop = m.is_err();
            if tx.send(wrap(generation, m)).is_err() || stop {
                break;
            }
        }
    });
}

/// Upper bound on control re-connects in one session.
const MAX_CONTROL_RECONNECTS: u32 = 8;

/// The control stream plus what is needed to bring it back (spec 7.7). The client re-dials
/// with the session's data token; the server adopts whichever resumed stream arrives last.
/// Events from a replaced stream carry an old generation number and are ignored.
struct CtrlLink<E: Send + 'static> {
    w: CtrlWriter,
    stream: std::net::TcpStream,
    generation: u32,
    lost: bool,
    reconn: Reconn,
    tx: Sender<E>,
    wrap: Wrap<E>,
    reconnects: u32,
    error_sent: bool,
}

impl<E: Send + 'static> CtrlLink<E> {
    fn new(ctrl: Control, reconn: Reconn, tx: Sender<E>, wrap: Wrap<E>) -> Self {
        let Control { reader, writer, stream, .. } = ctrl;
        spawn_ctrl_reader(reader, tx.clone(), wrap, 0);
        CtrlLink { w: writer, stream, generation: 0, lost: false, reconn, tx, wrap, reconnects: 0, error_sent: false }
    }

    /// Tell the other device why we stopped (once), so it fails at once instead of
    /// waiting for a reconnect.
    fn send_error(&mut self, e: &FliqError) {
        if !self.error_sent && !matches!(e, FliqError::Rejected) {
            self.error_sent = true;
            self.send(&error_msg(e));
        }
    }

    /// Best effort: a failed send marks the link lost; state is re-sent after recovery.
    fn send(&mut self, m: &Ctrl) {
        if !self.lost && send_ctrl(&mut self.w, m).is_err() {
            self.lost = true;
        }
    }

    /// For the negotiation phase, where a lost control stream ends the session.
    fn send_strict(&mut self, m: &Ctrl) -> Result<()> {
        send_ctrl(&mut self.w, m).inspect_err(|_| self.lost = true)
    }

    fn is_current(&self, generation: u32) -> bool {
        generation == self.generation
    }

    fn install(&mut self, c: Control) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        self.generation += 1;
        let Control { reader, writer, stream, .. } = c;
        spawn_ctrl_reader(reader, self.tx.clone(), self.wrap, self.generation);
        self.w = writer;
        self.stream = stream;
        self.lost = false;
    }

    /// Server side: adopt a stream the client re-opened. Returns true if one was adopted.
    fn poll_resume(&mut self) -> bool {
        let got = match &self.reconn {
            Reconn::Server(rx) => rx.try_recv().ok(),
            Reconn::Client(_) => None,
        };
        match got {
            Some(c) => {
                self.install(c);
                true
            }
            None => false,
        }
    }

    /// Block until the control stream is back (60 s at most).
    fn recover(&mut self, cancel: &CancelToken, linger: Duration) -> Result<()> {
        self.reconnects += 1;
        if self.reconnects > MAX_CONTROL_RECONNECTS {
            return Err(FliqError::ConnectionLost);
        }
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        let deadline = Instant::now() + linger;
        while Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(FliqError::Cancelled);
            }
            let got = match &self.reconn {
                Reconn::Client(c) => {
                    let r = c.resume_control().ok();
                    if r.is_none() {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    r
                }
                Reconn::Server(rx) => rx.recv_timeout(Duration::from_millis(500)).ok(),
            };
            if let Some(c) = got {
                self.install(c);
                return Ok(());
            }
        }
        Err(FliqError::ConnectionLost)
    }

    fn kill_for_test(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

struct Meter {
    start: Instant,
    last_emit: Instant,
    last_bytes: u64,
    last_t: Instant,
    rate: f64,
}

impl Meter {
    fn new() -> Self {
        let n = Instant::now();
        Meter { start: n, last_emit: n, last_bytes: 0, last_t: n, rate: 0.0 }
    }
    fn maybe(&mut self, done: u64, total: u64, streams: u16, stats: &StageStats) -> Option<Progress> {
        let now = Instant::now();
        if now - self.last_emit < PROGRESS_EVERY {
            return None;
        }
        let dt = (now - self.last_t).as_secs_f64();
        let inst = (done - self.last_bytes) as f64 / dt.max(1e-6);
        self.rate = if self.rate == 0.0 { inst } else { 0.7 * self.rate + 0.3 * inst };
        self.last_bytes = done;
        self.last_t = now;
        self.last_emit = now;
        let eta = (self.rate > 1.0).then(|| (total - done) as f64 / self.rate);
        Some(Progress {
            bytes_done: done,
            total,
            mbps: self.rate / 1e6,
            eta_secs: eta,
            streams,
            bottleneck: stats.snapshot().bottleneck(),
        })
    }
    fn summary(&self, bytes: u64, files: Vec<FileResult>, stats: &StageStats) -> Summary {
        let secs = self.start.elapsed().as_secs_f64();
        let st = stats.snapshot();
        Summary {
            bytes,
            secs,
            mbps: bytes as f64 / secs.max(1e-9) / 1e6,
            files,
            chunks_resent: st.chunks_resent,
            stages: st,
            control_reconnects: 0,
        }
    }
}

fn error_msg(e: &FliqError) -> Ctrl {
    Ctrl::Error { code: e.code().into(), text: e.code().into() }
}

// ------------------------------------------------------------------ sender

struct Job {
    file: usize,
    idx: u32,
    buf: Vec<u8>,
    len: usize,
}

enum SendEv {
    Sent { file: usize, idx: u32, len: usize },
    Ctrl(u32, Result<Ctrl>),
    ReadError,
}

/// Run the sending side after `Control` is established (either role).
pub fn run_sender(
    ctrl: Control,
    plane: Plane,
    items: Vec<SendItem>,
    opts: &EngineOptions,
    on_event: &mut dyn FnMut(Event),
) -> Result<Summary> {
    let is_client = matches!(plane, Plane::Client(_));
    let (data_plane, reconn) = plane.split();
    let (ev_tx, ev_rx) = unbounded::<SendEv>();
    let mut link = CtrlLink::new(ctrl, reconn, ev_tx.clone(), SendEv::Ctrl);
    let r = run_sender_inner(&mut link, ev_tx, ev_rx, data_plane, items, opts, on_event, is_client);
    if let Err(e) = &r {
        link.send_error(e);
        opts.cancel.cancel();
    }
    link.shutdown();
    opts.cancel.close_all();
    r
}

#[allow(clippy::too_many_arguments)]
fn run_sender_inner(
    link: &mut CtrlLink<SendEv>,
    ev_tx: Sender<SendEv>,
    ev_rx: Receiver<SendEv>,
    data_plane: DataPlane,
    items: Vec<SendItem>,
    opts: &EngineOptions,
    on_event: &mut dyn FnMut(Event),
    is_client: bool,
) -> Result<Summary> {
    if items.len() > opts.limits.max_files {
        return Err(FliqError::Limit(format!("Too many files: the limit is {} per transfer.", opts.limits.max_files)));
    }
    let metas: Vec<FileMeta> = items
        .iter()
        .enumerate()
        .map(|(i, it)| FileMeta {
            id: i as u32,
            name: it.name.clone(),
            size: it.size,
            mtime: it.mtime,
            chunk_count: chunk_count(it.size) as u32,
        })
        .collect();
    let total = validate_offer(&metas, &opts.limits)?;
    link.send_strict(&Ctrl::Offer { files: metas.clone() })?;
    // The receiver's user decides; no timeout here (they can cancel).
    loop {
        if opts.cancel.is_cancelled() {
            return Err(FliqError::Cancelled);
        }
        match ev_rx.recv_timeout(TICK) {
            Ok(SendEv::Ctrl(_, Ok(m))) => match m {
                Ctrl::Accept { files } if files.len() == metas.len() => break,
                Ctrl::Accept { .. } => return Err(FliqError::Protocol("partial accept")),
                Ctrl::Reject { .. } => return Err(FliqError::Rejected),
                Ctrl::Error { code, .. } => return Err(FliqError::Remote(code)),
                _ => return Err(FliqError::Protocol("expected Accept")),
            },
            Ok(SendEv::Ctrl(_, Err(e))) => return Err(e),
            _ => {}
        }
    }
    if is_client {
        link.send_strict(&Ctrl::StreamsReady { count: opts.streams })?;
    }

    let items = Arc::new(items);
    let n = items.len();
    let hashes: Arc<Vec<Mutex<Vec<[u8; 32]>>>> =
        Arc::new(metas.iter().map(|m| Mutex::new(vec![[0u8; 32]; m.chunk_count as usize])).collect());
    let pool_n = opts.pool_buffers();
    let (pool_tx, pool_rx) = bounded::<Vec<u8>>(pool_n);
    for _ in 0..pool_n {
        pool_tx.send(Vec::new()).unwrap();
    }
    let (work_tx, work_rx) = bounded::<Job>(pool_n);
    let (resend_tx, resend_rx) = unbounded::<(usize, u32)>();
    let stop = Arc::new(AtomicBool::new(false));

    // Disk reader.
    let reader = {
        let (items, pool_rx, work_tx, stop, ev, stats) =
            (items.clone(), pool_rx.clone(), work_tx.clone(), stop.clone(), ev_tx.clone(), opts.stats.clone());
        std::thread::spawn(move || {
            let read_one = |file: usize, idx: u32| -> Option<()> {
                let mut buf = loop {
                    if stop.load(SeqCst) {
                        return None;
                    }
                    if let Ok(b) = pool_rx.recv_timeout(TICK) {
                        break b;
                    }
                };
                let len = chunk_len(items[file].size, idx) as usize;
                buf.resize(len, 0);
                let t = Timer::start();
                if items[file].source.read_chunk(idx, &mut buf[..len]).is_err() {
                    let _ = ev.send(SendEv::ReadError);
                    return None;
                }
                t.stop(&stats.read_ns);
                work_tx.send(Job { file, idx, buf, len }).ok()
            };
            'outer: for f in 0..items.len() {
                for idx in 0..chunk_count(items[f].size) as u32 {
                    while let Ok((rf, ri)) = resend_rx.try_recv() {
                        if read_one(rf, ri).is_none() {
                            break 'outer;
                        }
                    }
                    if read_one(f, idx).is_none() {
                        break 'outer;
                    }
                }
            }
            while !stop.load(SeqCst) {
                if let Ok((rf, ri)) = resend_rx.recv_timeout(TICK)
                    && read_one(rf, ri).is_none()
                {
                    break;
                }
            }
        })
    };

    // Data stream workers.
    let mut streams = {
        let (work_rx, work_tx, pool_tx, ev, stop, hashes, stats, fault) = (
            work_rx.clone(),
            work_tx.clone(),
            pool_tx.clone(),
            ev_tx.clone(),
            stop.clone(),
            hashes.clone(),
            opts.stats.clone(),
            opts.fault.clone(),
        );
        let sent_total = Arc::new(AtomicU64::new(0));
        Streams::start(data_plane, opts.streams, opts.cancel.clone(), move |mut c: DataConn| {
            loop {
                if stop.load(SeqCst) {
                    break;
                }
                let job = match work_rx.recv_timeout(TICK) {
                    Ok(j) => j,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let t = Timer::start();
                let h = *blake3::hash(&job.buf[..job.len]).as_bytes();
                t.stop(&stats.hash_ns);
                hashes[job.file].lock().unwrap()[job.idx as usize] = h;
                let hdr =
                    ChunkHeader { file_id: job.file as u32, chunk_index: job.idx, chunk_hash: h, len: job.len as u32 };
                let res = c
                    .writer
                    .write(&hdr.encode())
                    .and_then(|_| c.writer.write(&job.buf[..job.len]))
                    .and_then(|_| c.writer.flush());
                if res.is_err() {
                    let _ = work_tx.send(job); // requeue on another stream
                    return;
                }
                let (file, idx, len) = (job.file, job.idx, job.len);
                let _ = pool_tx.send(job.buf);
                let _ = ev.send(SendEv::Sent { file, idx, len });
                let k = sent_total.fetch_add(1, SeqCst) + 1;
                if let Some(f) = &fault
                    && f.kill_stream_after_chunks > 0
                    && k >= f.kill_stream_after_chunks
                    && !f.fired.swap(true, SeqCst)
                {
                    c.shutdown();
                    return;
                }
            }
            let _ = c.writer.write(&ChunkHeader::end().encode()).and_then(|_| c.writer.flush());
        })
    };

    // Coordinator.
    let mut sent: Vec<Bitmap> = metas.iter().map(|m| Bitmap::new(m.chunk_count)).collect();
    let mut done_sent = vec![false; n];
    let mut verified = vec![false; n];
    let mut bytes_unique: u64 = 0;
    let mut chunks_sent: u64 = 0;
    let mut meter = Meter::new();
    let mut last_activity = Instant::now();
    let result: Result<()> = (|| {
        let file_done = |f: usize| Ctrl::FileDone {
            id: f as u32,
            manifest_hash: manifest_hash(&hashes[f].lock().unwrap()).to_vec(),
        };
        // After a control reconnect, repeat every FileDone (the receiver treats repeats as no-ops).
        let resync = |link: &mut CtrlLink<SendEv>, done_sent: &[bool]| {
            for (f, d) in done_sent.iter().enumerate() {
                if *d {
                    link.send(&file_done(f));
                }
            }
        };
        for (f, bm) in sent.iter().enumerate() {
            if bm.is_complete() {
                link.send(&file_done(f));
                done_sent[f] = true;
            }
        }
        while verified.iter().any(|v| !v) {
            if opts.cancel.is_cancelled() {
                return Err(FliqError::Cancelled);
            }
            match ev_rx.recv_timeout(TICK) {
                Ok(SendEv::Sent { file, idx, len }) => {
                    last_activity = Instant::now();
                    chunks_sent += 1;
                    if sent[file].set(idx) {
                        bytes_unique += len as u64;
                        if sent[file].is_complete() && !done_sent[file] {
                            link.send(&file_done(file));
                            done_sent[file] = true;
                        }
                    }
                    if let Some(f) = &opts.fault
                        && f.kill_control_after_chunks > 0
                        && chunks_sent >= f.kill_control_after_chunks
                        && !f.control_fired.swap(true, SeqCst)
                    {
                        link.kill_for_test();
                    }
                }
                Ok(SendEv::Ctrl(g, Ok(m))) if link.is_current(g) => {
                    last_activity = Instant::now();
                    match m {
                        Ctrl::Ack { id, bitmap } => {
                            let f = id as usize;
                            let have = (f < n).then(|| Bitmap::from_bytes(metas[f].chunk_count, &bitmap)).flatten();
                            let have = have.ok_or(FliqError::Protocol("bad Ack"))?;
                            for i in have.missing() {
                                opts.stats.chunks_resent.fetch_add(1, Relaxed);
                                let _ = resend_tx.send((f, i));
                            }
                        }
                        Ctrl::Verified { id } if (id as usize) < n => {
                            if !std::mem::replace(&mut verified[id as usize], true) {
                                on_event(Event::FileVerified { id });
                            }
                        }
                        Ctrl::Error { code, .. } => return Err(FliqError::Remote(code)),
                        Ctrl::Close => return Err(FliqError::ConnectionLost),
                        _ => {}
                    }
                }
                Ok(SendEv::Ctrl(g, Err(_))) if link.is_current(g) => link.lost = true,
                Ok(SendEv::Ctrl(..)) => {} // from a replaced control stream
                Ok(SendEv::ReadError) => return Err(FliqError::Io(std::io::Error::other("source read failed"))),
                Err(_) => {}
            }
            if link.poll_resume() {
                resync(link, &done_sent);
                last_activity = Instant::now();
            }
            if link.lost && verified.iter().any(|v| !v) {
                on_event(Event::Reconnecting);
                link.recover(&opts.cancel, opts.linger)?;
                on_event(Event::Reconnected);
                resync(link, &done_sent);
                last_activity = Instant::now();
            }
            if last_activity.elapsed() > opts.linger {
                return Err(FliqError::ConnectionLost);
            }
            if let Some(p) = meter.maybe(bytes_unique, total, streams.live(), &opts.stats) {
                on_event(Event::Progress(p));
            }
        }
        link.send(&Ctrl::Complete);
        // Wait briefly for Close so the receiver sees Complete before streams end.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !link.lost {
            match ev_rx.recv_timeout(TICK) {
                Ok(SendEv::Ctrl(g, Ok(Ctrl::Close))) | Ok(SendEv::Ctrl(g, Err(_))) if link.is_current(g) => break,
                _ => {}
            }
        }
        Ok(())
    })();
    drop(ev_tx);
    stop.store(true, SeqCst);
    if let Err(e) = &result {
        link.send_error(e);
        streams.abort(); // unblock workers stuck on socket I/O
    }
    streams.finish();
    let _ = reader.join();
    opts.cancel.close_all();
    result?;
    let files = metas.iter().map(|m| FileResult { id: m.id, size: m.size, path: None }).collect();
    on_event(Event::Progress(Progress {
        bytes_done: total,
        total,
        mbps: meter.summary(total, vec![], &opts.stats).mbps,
        eta_secs: Some(0.0),
        streams: 0,
        bottleneck: opts.stats.snapshot().bottleneck(),
    }));
    let mut summary = meter.summary(total, files, &opts.stats);
    summary.control_reconnects = link.generation;
    Ok(summary)
}

// ------------------------------------------------------------------ receiver

struct RecvFile {
    meta: FileMeta,
    file: Option<Arc<File>>,
    have: Bitmap,
    hashes: Vec<[u8; 32]>,
}

enum RecvEv {
    Written { file: usize, len: usize },
    Fatal(FliqError),
    Ctrl(u32, Result<Ctrl>),
}

/// Deletes every `.part` file not finalized when dropped (error, cancel, panic).
struct PartGuard {
    parts: Vec<(PathBuf, bool)>,
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        for (p, finalized) in &self.parts {
            if !finalized {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// Remove leftovers of crashed sessions (`*.fliq.part`) from the save folder.
pub fn sweep_stale_parts(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(PART_SUFFIX) && std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

pub struct ReceiveOptions {
    pub out_dir: PathBuf,
    pub engine: EngineOptions,
}

/// Run the receiving side. `decide` is shown the offer (with verification code) and
/// returns true to accept.
pub fn run_receiver(
    ctrl: Control,
    plane: Plane,
    ropts: &ReceiveOptions,
    decide: &mut dyn FnMut(&OfferInfo) -> bool,
    on_event: &mut dyn FnMut(Event),
) -> Result<Summary> {
    let (data_plane, reconn) = plane.split();
    let (code, peer) = (ctrl.verification_code.clone(), ctrl.peer_name.clone());
    let (ev_tx, ev_rx) = unbounded::<RecvEv>();
    let mut link = CtrlLink::new(ctrl, reconn, ev_tx.clone(), RecvEv::Ctrl);
    let r = run_receiver_inner(&mut link, ev_tx, ev_rx, &code, &peer, data_plane, ropts, decide, on_event);
    if let Err(e) = &r {
        link.send_error(e);
        ropts.engine.cancel.cancel();
    }
    link.shutdown();
    ropts.engine.cancel.close_all();
    r
}

#[allow(clippy::too_many_arguments)]
fn run_receiver_inner(
    link: &mut CtrlLink<RecvEv>,
    ev_tx: Sender<RecvEv>,
    ev_rx: Receiver<RecvEv>,
    verification_code: &str,
    peer_name: &str,
    data_plane: DataPlane,
    ropts: &ReceiveOptions,
    decide: &mut dyn FnMut(&OfferInfo) -> bool,
    on_event: &mut dyn FnMut(Event),
) -> Result<Summary> {
    let opts = &ropts.engine;
    let files = loop {
        if opts.cancel.is_cancelled() {
            return Err(FliqError::Cancelled);
        }
        match ev_rx.recv_timeout(TICK) {
            Ok(RecvEv::Ctrl(_, Ok(Ctrl::Offer { files }))) => break files,
            Ok(RecvEv::Ctrl(_, Ok(Ctrl::Error { code, .. }))) => return Err(FliqError::Remote(code)),
            Ok(RecvEv::Ctrl(_, Ok(_))) => return Err(FliqError::Protocol("expected Offer")),
            Ok(RecvEv::Ctrl(_, Err(e))) => return Err(e),
            _ => {}
        }
    };
    let total = match validate_offer(&files, &opts.limits) {
        Ok(t) => t,
        Err(e) => {
            link.send(&Ctrl::Reject { reason: e.code().into() });
            return Err(e);
        }
    };
    std::fs::create_dir_all(&ropts.out_dir)?;
    sweep_stale_parts(&ropts.out_dir);
    let free = fsutil::free_space(&ropts.out_dir)?;
    if total > free {
        link.send(&Ctrl::Reject { reason: "E_NO_SPACE".into() });
        return Err(FliqError::NoSpace { need: total, have: free });
    }
    let names: Vec<String> = files.iter().map(|f| sanitize::sanitize_name(&f.name)).collect();
    let info = OfferInfo {
        verification_code: verification_code.to_string(),
        peer_name: peer_name.to_string(),
        files: names.iter().cloned().zip(files.iter().map(|f| f.size)).collect(),
        total,
        free_space: free,
        fat32_warning: files.iter().any(|f| f.size >= 4 * 1024 * 1024 * 1024) && fsutil::is_fat(&ropts.out_dir),
    };
    if !decide(&info) {
        link.send(&Ctrl::Reject { reason: "declined".into() });
        return Err(FliqError::Rejected);
    }

    // Reserve targets and create .part files.
    let mut taken = HashSet::new();
    let mut guard = PartGuard { parts: vec![] };
    let mut finals = vec![];
    let mut state = vec![];
    for (f, name) in files.iter().zip(&names) {
        let fin = sanitize::unique_target(&ropts.out_dir, name, &mut taken);
        let part = sanitize::part_path(&fin);
        let fh = OpenOptions::new().read(true).write(true).create_new(true).open(&part)?;
        guard.parts.push((part, false));
        let _ = fh.set_len(f.size); // pre-allocate; ignore failure
        finals.push(fin);
        state.push(Mutex::new(RecvFile {
            meta: f.clone(),
            file: Some(Arc::new(fh)),
            have: Bitmap::new(f.chunk_count),
            hashes: vec![[0; 32]; f.chunk_count as usize],
        }));
    }
    let state = Arc::new(state);
    let by_id: Arc<HashMap<u32, usize>> = Arc::new(files.iter().enumerate().map(|(i, f)| (f.id, i)).collect());
    link.send_strict(&Ctrl::Accept { files: files.iter().map(|f| f.id).collect() })?;

    let mut streams = {
        let (state, by_id, ev, stats) = (state.clone(), by_id.clone(), ev_tx.clone(), opts.stats.clone());
        Streams::start(data_plane, opts.streams, opts.cancel.clone(), move |c: DataConn| {
            recv_worker(c, &state, &by_id, &ev, &stats);
        })
    };

    let n = files.len();
    let mut manifests: Vec<Option<Vec<u8>>> = vec![None; n];
    let mut verified = vec![false; n];
    let mut bytes_done: u64 = 0;
    let mut meter = Meter::new();
    let mut last_progress = Instant::now();
    let mut last_ack = vec![Instant::now(); n];
    let mut complete_seen = false;
    let mut force_ack = false;

    let result: Result<()> = (|| {
        let try_finalize = |f: usize,
                            manifests: &Vec<Option<Vec<u8>>>,
                            verified: &mut Vec<bool>,
                            link: &mut CtrlLink<RecvEv>,
                            guard: &mut PartGuard,
                            finals: &mut Vec<PathBuf>,
                            taken: &mut HashSet<String>|
         -> Result<Option<u32>> {
            if verified[f] {
                return Ok(None);
            }
            let Some(want) = &manifests[f] else { return Ok(None) };
            let mut st = state[f].lock().unwrap();
            if !st.have.is_complete() {
                return Ok(None);
            }
            if manifest_hash(&st.hashes)[..] != want[..] {
                return Err(FliqError::HashMismatch);
            }
            if let Some(fh) = st.file.take() {
                fh.sync_all()?;
            }
            drop(st);
            if finals[f].exists() {
                let name = finals[f].file_name().unwrap().to_string_lossy().into_owned();
                finals[f] = sanitize::unique_target(&ropts.out_dir, &name, taken);
            }
            std::fs::rename(&guard.parts[f].0, &finals[f])?;
            guard.parts[f].1 = true;
            verified[f] = true;
            link.send(&Ctrl::Verified { id: files[f].id });
            Ok(Some(files[f].id))
        };
        while !(complete_seen && verified.iter().all(|v| *v)) {
            if opts.cancel.is_cancelled() {
                return Err(FliqError::Cancelled);
            }
            match ev_rx.recv_timeout(TICK) {
                Ok(RecvEv::Written { file, len }) => {
                    bytes_done += len as u64;
                    last_progress = Instant::now();
                    if let Some(id) =
                        try_finalize(file, &manifests, &mut verified, link, &mut guard, &mut finals, &mut taken)?
                    {
                        on_event(Event::FileVerified { id });
                    }
                }
                Ok(RecvEv::Fatal(e)) => return Err(e),
                Ok(RecvEv::Ctrl(g, Ok(m))) if link.is_current(g) => match m {
                    Ctrl::FileDone { id, manifest_hash } => {
                        let f = *by_id.get(&id).ok_or(FliqError::Protocol("unknown file id"))?;
                        if verified[f] {
                            continue; // repeated after a control reconnect
                        }
                        manifests[f] = Some(manifest_hash);
                        last_ack[f] = Instant::now();
                        if let Some(id) =
                            try_finalize(f, &manifests, &mut verified, link, &mut guard, &mut finals, &mut taken)?
                        {
                            on_event(Event::FileVerified { id });
                        }
                    }
                    Ctrl::Complete => {
                        if !verified.iter().all(|v| *v) {
                            return Err(FliqError::Protocol("Complete before all files verified"));
                        }
                        complete_seen = true;
                    }
                    Ctrl::StreamsReady { .. } => {}
                    Ctrl::Error { code, .. } => return Err(FliqError::Remote(code)),
                    Ctrl::Close => return Err(FliqError::ConnectionLost),
                    _ => return Err(FliqError::Protocol("unexpected control message")),
                },
                Ok(RecvEv::Ctrl(g, Err(_))) if link.is_current(g) => link.lost = true,
                Ok(RecvEv::Ctrl(..)) => {} // from a replaced control stream
                Err(_) => {}
            }
            let mut resumed = link.poll_resume();
            if link.lost {
                if verified.iter().all(|v| *v) {
                    break; // every file is verified and on disk; Complete is a formality
                }
                on_event(Event::Reconnecting);
                link.recover(&opts.cancel, opts.linger)?;
                on_event(Event::Reconnected);
                resumed = true;
            }
            if resumed {
                // Repeat what the sender may have missed, and ask for repairs right away.
                for f in 0..n {
                    if verified[f] {
                        link.send(&Ctrl::Verified { id: files[f].id });
                    }
                }
                force_ack = true;
                last_progress = Instant::now();
            }
            // Repair: ask for missing chunks once the sender has announced the file and
            // nothing has arrived for a while (a data stream died with chunks in flight).
            for f in 0..n {
                if manifests[f].is_some() && !verified[f] {
                    let idle = last_progress.elapsed() > Duration::from_millis(REPAIR_IDLE_MS);
                    if force_ack || (idle && last_ack[f].elapsed() > Duration::from_millis(REPAIR_IDLE_MS)) {
                        let bm = state[f].lock().unwrap().have.as_bytes().to_vec();
                        link.send(&Ctrl::Ack { id: files[f].id, bitmap: bm });
                        last_ack[f] = Instant::now();
                    }
                }
            }
            force_ack = false;
            if last_progress.elapsed() > opts.linger {
                return Err(FliqError::ConnectionLost);
            }
            if let Some(p) = meter.maybe(bytes_done, total, streams.live(), &opts.stats) {
                on_event(Event::Progress(p));
            }
        }
        link.send(&Ctrl::Close);
        Ok(())
    })();
    drop(ev_tx);
    if let Err(e) = &result {
        link.send_error(e);
        streams.abort();
    }
    streams.finish();
    opts.cancel.close_all();
    if let Err(e) = result {
        drop(state);
        return Err(e); // PartGuard deletes partial files.
    }
    let out =
        files.iter().zip(&finals).map(|(f, p)| FileResult { id: f.id, size: f.size, path: Some(p.clone()) }).collect();
    let mut summary = meter.summary(total, out, &opts.stats);
    summary.control_reconnects = link.generation;
    Ok(summary)
}

fn recv_worker(
    mut c: DataConn,
    state: &[Mutex<RecvFile>],
    by_id: &HashMap<u32, usize>,
    ev: &Sender<RecvEv>,
    stats: &StageStats,
) {
    let mut buf = vec![0u8; CHUNK_SIZE as usize];
    let fatal = |e: FliqError| {
        let _ = ev.send(RecvEv::Fatal(e));
    };
    loop {
        let mut h = [0u8; DATA_HEADER_LEN];
        if c.reader.read_exact(&mut h).is_err() {
            return; // stream died; the client re-dials, repair covers lost chunks
        }
        let hdr = ChunkHeader::decode(&h);
        if hdr.file_id == END_OF_STREAM {
            return;
        }
        let Some(&f) = by_id.get(&hdr.file_id) else { return fatal(FliqError::Protocol("unknown file id")) };
        let (size, cc) = {
            let st = state[f].lock().unwrap();
            (st.meta.size, st.meta.chunk_count)
        };
        if hdr.chunk_index >= cc {
            return fatal(FliqError::Protocol("chunk index out of range"));
        }
        if hdr.len as u64 != chunk_len(size, hdr.chunk_index) {
            return fatal(FliqError::Protocol("chunk size mismatch"));
        }
        let len = hdr.len as usize;
        if c.reader.read_exact(&mut buf[..len]).is_err() {
            return;
        }
        let t = Timer::start();
        let hash = *blake3::hash(&buf[..len]).as_bytes();
        t.stop(&stats.verify_ns);
        if hash != hdr.chunk_hash {
            return fatal(FliqError::HashMismatch);
        }
        let file = {
            let st = state[f].lock().unwrap();
            if st.have.get(hdr.chunk_index) {
                if st.hashes[hdr.chunk_index as usize] != hash {
                    return fatal(FliqError::HashMismatch);
                }
                continue; // harmless duplicate from a repair round
            }
            match &st.file {
                Some(fh) => fh.clone(),
                None => continue,
            }
        };
        let t = Timer::start();
        if let Err(e) = fsutil::write_all_at(&file, &buf[..len], hdr.chunk_index as u64 * CHUNK_SIZE) {
            return fatal(io_err(e));
        }
        t.stop(&stats.write_ns);
        let newly = {
            let mut st = state[f].lock().unwrap();
            let newly = st.have.set(hdr.chunk_index);
            if newly {
                st.hashes[hdr.chunk_index as usize] = hash;
            }
            newly
        };
        if newly {
            stats.payload_bytes.fetch_add(len as u64, Relaxed);
            let _ = ev.send(RecvEv::Written { file: f, len });
        }
    }
}
