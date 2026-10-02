//! High-level session API for user interfaces (the Flutter bridge and the CLI).
//!
//! One call starts a whole session on a background thread. Everything the UI needs comes
//! back as [`UiEvent`]s through a sink; the UI talks back only through [`SessionHandle`]
//! (accept/decline, cancel). No file bytes ever cross this boundary.

use crate::consts::*;
use crate::engine::*;
use crate::error::{FliqError, Result};
use crate::fsutil;
use crate::msg::Limits;
use crate::net::TcpLink;
use crate::qr::{QrPayload, ServerMode, WifiInfo, now_unix};
use crate::session::{CancelToken, Client, Server, ServerConfig};
use crate::stats::StageStats;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub device_name: String,
    pub save_dir: PathBuf,
    pub streams: u16,
    pub inflight_budget: usize,
    pub max_total: u64,
    pub max_files: usize,
    /// Sender only: send each source to the Recycle Bin once the receiver verified it.
    pub move_sources: bool,
    /// Tests and local benchmarks: advertise 127.0.0.1.
    pub loopback: bool,
    /// Wi-Fi details to put in the QR when this device created the network (M4).
    pub wifi: Option<(String, String)>,
}

impl AppConfig {
    pub fn new(device_name: &str, save_dir: PathBuf) -> Self {
        AppConfig {
            device_name: device_name.into(),
            save_dir,
            streams: DEFAULT_STREAMS,
            inflight_budget: DEFAULT_INFLIGHT_BUDGET_BYTES,
            max_total: DEFAULT_MAX_TOTAL_BYTES,
            max_files: DEFAULT_MAX_FILES,
            move_sources: false,
            loopback: false,
            wifi: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedSize {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    /// Server only: show this QR. `uri` contains a secret.
    ShowQr {
        uri: String,
        addresses: Vec<String>,
        port: u16,
        expires_unix: u64,
    },
    /// Server only: result of probing each advertised address.
    SelfCheck {
        reachable: Vec<String>,
        unreachable: Vec<String>,
    },
    Connected {
        code: String,
        peer_name: String,
    },
    /// Receiver only: call `SessionHandle::decide` to continue.
    OfferReceived {
        code: String,
        peer_name: String,
        files: Vec<NamedSize>,
        total: u64,
        free_space: u64,
        fat32_warning: bool,
    },
    /// Sender only: what is about to be offered.
    Sending {
        files: Vec<NamedSize>,
        total: u64,
        move_sources: bool,
    },
    Progress {
        done: u64,
        total: u64,
        mbps: f64,
        eta_secs: Option<f64>,
        streams: u16,
        bottleneck: String,
    },
    Reconnecting,
    Reconnected,
    FileVerified {
        index: u32,
        name: String,
        path: Option<String>,
    },
    /// The source could not be moved to the Recycle Bin (the copy on the other device is fine).
    MoveFailed {
        name: String,
    },
    Finished {
        bytes: u64,
        secs: f64,
        mbps: f64,
        files: Vec<FinishedFile>,
        stream_count: u16,
    },
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub struct FinishedFile {
    pub name: String,
    pub size: u64,
    /// Receiver: where it was saved.
    pub path: Option<String>,
}

pub type Sink = Arc<dyn Fn(UiEvent) + Send + Sync>;

/// Talk back to a running session.
pub struct SessionHandle {
    cancel: Arc<CancelToken>,
    decision: Sender<bool>,
    finished: Arc<AtomicBool>,
}

impl SessionHandle {
    /// Receiver: accept or decline the offer shown in `OfferReceived`.
    pub fn decide(&self, accept: bool) {
        let _ = self.decision.try_send(accept);
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(SeqCst)
    }
}

fn fail(sink: &Sink, e: &FliqError) {
    sink(UiEvent::Failed { code: e.code().into(), message: e.to_string() });
}

fn engine_opts(cfg: &AppConfig, cancel: Arc<CancelToken>, stats: Arc<StageStats>) -> EngineOptions {
    let mut o = EngineOptions::new(cancel, stats);
    o.streams = cfg.streams.clamp(1, MAX_DATA_STREAMS);
    o.inflight_budget = cfg.inflight_budget;
    o.limits = Limits { max_total: cfg.max_total, max_files: cfg.max_files };
    o
}

fn load_items(files: &[PathBuf]) -> Result<Vec<SendItem>> {
    if files.is_empty() {
        return Err(FliqError::Limit("Choose at least one file to send.".into()));
    }
    files.iter().map(|p| SendItem::from_path(p)).collect()
}

fn decider(sink: Sink, rx: Receiver<bool>, cancel: Arc<CancelToken>) -> impl FnMut(&OfferInfo) -> bool {
    move |o: &OfferInfo| {
        sink(UiEvent::OfferReceived {
            code: o.verification_code.clone(),
            peer_name: o.peer_name.clone(),
            files: o.files.iter().map(|(n, s)| NamedSize { name: n.clone(), size: *s }).collect(),
            total: o.total,
            free_space: o.free_space,
            fat32_warning: o.fat32_warning,
        });
        loop {
            if cancel.is_cancelled() {
                return false;
            }
            if let Ok(d) = rx.recv_timeout(Duration::from_millis(100)) {
                return d;
            }
        }
    }
}

fn forward(sink: &Sink, e: Event, names: &[String], sources: &[PathBuf], move_sources: bool) {
    match e {
        Event::Progress(p) => sink(UiEvent::Progress {
            done: p.bytes_done,
            total: p.total,
            mbps: p.mbps,
            eta_secs: p.eta_secs,
            streams: p.streams,
            bottleneck: p.bottleneck.as_str().into(),
        }),
        Event::Reconnecting => sink(UiEvent::Reconnecting),
        Event::Reconnected => sink(UiEvent::Reconnected),
        Event::FileVerified { id } => {
            let i = id as usize;
            let name = names.get(i).cloned().unwrap_or_default();
            if move_sources && let Some(src) = sources.get(i) {
                // Only after Verified, never on cancel or failure (spec 6.6).
                if fsutil::trash(src).is_err() {
                    sink(UiEvent::MoveFailed { name: name.clone() });
                }
            }
            sink(UiEvent::FileVerified { index: id, name, path: None });
        }
    }
}

fn finished(sink: &Sink, s: &Summary, names: &[String], streams: u16) {
    let files = s
        .files
        .iter()
        .map(|f| FinishedFile {
            name: match &f.path {
                Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                None => names.get(f.id as usize).cloned().unwrap_or_default(),
            },
            size: f.size,
            path: f.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        })
        .collect();
    sink(UiEvent::Finished { bytes: s.bytes, secs: s.secs, mbps: s.mbps, files, stream_count: streams });
}

/// Show a QR and wait for the other device. `send` = files to send (Flow A), or empty to
/// receive into `cfg.save_dir` (Flow B).
pub fn host(cfg: AppConfig, send: Vec<PathBuf>, sink: Sink) -> Result<SessionHandle> {
    let mode = if send.is_empty() { ServerMode::Receive } else { ServerMode::Send };
    let items = if send.is_empty() { vec![] } else { load_items(&send)? };
    let cancel = CancelToken::new();
    let stats = Arc::new(StageStats::default());
    let mut sc = ServerConfig::new(mode, &cfg.device_name);
    sc.link = TcpLink { preferred: None, include_loopback: cfg.loopback };
    sc.wifi = cfg.wifi.clone().map(|(ssid, pass)| WifiInfo { ssid, pass, band_hint: None });
    let server = Server::start(sc, cancel.clone(), stats.clone())?;
    let (dtx, drx) = bounded(1);
    let finished_flag = Arc::new(AtomicBool::new(false));
    let handle = SessionHandle { cancel: cancel.clone(), decision: dtx, finished: finished_flag.clone() };
    let qr = server.qr();
    sink(UiEvent::ShowQr { uri: qr.to_uri(), addresses: qr.ip.clone(), port: qr.port, expires_unix: qr.exp });
    std::thread::spawn(move || {
        let (reachable, unreachable) = server.self_check();
        sink(UiEvent::SelfCheck { reachable, unreachable });
        let r = (|| -> Result<()> {
            let ctrl = server.wait_control()?;
            sink(UiEvent::Connected { code: ctrl.verification_code.clone(), peer_name: ctrl.peer_name.clone() });
            let opts = engine_opts(&cfg, cancel.clone(), stats.clone());
            let plane = Plane::server(&server);
            if mode == ServerMode::Send {
                let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
                sink(UiEvent::Sending {
                    files: items.iter().map(|i| NamedSize { name: i.name.clone(), size: i.size }).collect(),
                    total: items.iter().map(|i| i.size).sum(),
                    move_sources: cfg.move_sources,
                });
                let s =
                    run_sender(ctrl, plane, items, &opts, &mut |e| forward(&sink, e, &names, &send, cfg.move_sources))?;
                finished(&sink, &s, &names, opts.streams);
            } else {
                let ro = ReceiveOptions { out_dir: cfg.save_dir.clone(), engine: opts };
                let mut decide = decider(sink.clone(), drx, cancel.clone());
                let s = run_receiver(ctrl, plane, &ro, &mut decide, &mut |e| forward(&sink, e, &[], &[], false))?;
                finished(&sink, &s, &[], ro.engine.streams);
            }
            Ok(())
        })();
        server.close();
        if let Err(e) = r {
            fail(&sink, &e);
        }
        finished_flag.store(true, SeqCst);
    });
    Ok(handle)
}

/// Use a code from the other device. If it sends, files land in `cfg.save_dir`; if it
/// receives, `send` must list the files to send. The platform joins any Wi-Fi network named
/// in the code *before* calling this (Android, M3).
pub fn join(cfg: AppConfig, uri: &str, send: Vec<PathBuf>, sink: Sink) -> Result<SessionHandle> {
    let qr = QrPayload::from_uri(uri)?;
    qr.check_fresh(now_unix())?;
    let mode = qr.server_mode()?;
    let items = if mode == ServerMode::Receive { load_items(&send)? } else { vec![] };
    let cancel = CancelToken::new();
    let stats = Arc::new(StageStats::default());
    let (dtx, drx) = bounded(1);
    let finished_flag = Arc::new(AtomicBool::new(false));
    let handle = SessionHandle { cancel: cancel.clone(), decision: dtx, finished: finished_flag.clone() };
    std::thread::spawn(move || {
        let r = (|| -> Result<()> {
            let (client, ctrl) = Client::connect(&qr, &cfg.device_name, cancel.clone(), stats.clone())?;
            sink(UiEvent::Connected { code: ctrl.verification_code.clone(), peer_name: ctrl.peer_name.clone() });
            let opts = engine_opts(&cfg, cancel.clone(), stats.clone());
            let plane = Plane::Client(Arc::new(client));
            if mode == ServerMode::Receive {
                let names: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
                sink(UiEvent::Sending {
                    files: items.iter().map(|i| NamedSize { name: i.name.clone(), size: i.size }).collect(),
                    total: items.iter().map(|i| i.size).sum(),
                    move_sources: cfg.move_sources,
                });
                let s =
                    run_sender(ctrl, plane, items, &opts, &mut |e| forward(&sink, e, &names, &send, cfg.move_sources))?;
                finished(&sink, &s, &names, opts.streams);
            } else {
                let ro = ReceiveOptions { out_dir: cfg.save_dir.clone(), engine: opts };
                let mut decide = decider(sink.clone(), drx, cancel.clone());
                let s = run_receiver(ctrl, plane, &ro, &mut decide, &mut |e| forward(&sink, e, &[], &[], false))?;
                finished(&sink, &s, &[], ro.engine.streams);
            }
            Ok(())
        })();
        if let Err(e) = r {
            fail(&sink, &e);
        }
        finished_flag.store(true, SeqCst);
    });
    Ok(handle)
}

/// Values for the dashboard metrics (spec 10.9). The engine ceiling is supplied by the UI
/// from its last measured result; everything else is the configured truth.
#[derive(Clone, Debug)]
pub struct EngineFacts {
    pub max_payload_bytes: u64,
    pub crypto_label: String,
    pub cipher: String,
    pub inflight_budget_bytes: u64,
    pub chunk_bytes: u64,
}

pub fn engine_facts(cfg: &AppConfig) -> EngineFacts {
    EngineFacts {
        max_payload_bytes: cfg.max_total,
        crypto_label: CRYPTO_LABEL.into(),
        cipher: CIPHER_NAME.into(),
        inflight_budget_bytes: cfg.inflight_budget.max(MIN_INFLIGHT_BUDGET_BYTES) as u64,
        chunk_bytes: CHUNK_SIZE,
    }
}

/// Free space and FAT warning for the Save Location screen.
pub fn folder_info(dir: &std::path::Path) -> Result<(u64, bool)> {
    std::fs::create_dir_all(dir)?;
    Ok((fsutil::free_space(dir)?, fsutil::is_fat(dir)))
}

// Keeps a mutex type in the public API stable for bindings that need interior mutability.
#[doc(hidden)]
pub type SharedHandle = Arc<Mutex<Option<SessionHandle>>>;
