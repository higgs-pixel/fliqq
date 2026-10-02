#![allow(dead_code)]
use fliq_core::engine::*;
use fliq_core::error::{FliqError, Result};
use fliq_core::net::TcpLink;
use fliq_core::qr::{QrPayload, ServerMode};
use fliq_core::session::*;
use fliq_core::stats::StageStats;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn opts() -> EngineOptions {
    EngineOptions::new(CancelToken::new(), Arc::new(StageStats::default()))
}

pub fn server(mode: ServerMode) -> Server {
    let mut cfg = ServerConfig::new(mode, "test-server");
    cfg.link = TcpLink { preferred: None, include_loopback: true };
    Server::start(cfg, CancelToken::new(), Arc::new(StageStats::default())).unwrap()
}

/// Loopback-only copy of the server's QR (keeps tests independent of host interfaces).
pub fn qr_local(s: &Server) -> QrPayload {
    let mut q = QrPayload::from_uri(&s.qr().to_uri()).unwrap();
    q.ip = vec!["127.0.0.1".into()];
    q
}

pub fn write_file(dir: &Path, name: &str, len: usize, seed: u8) -> PathBuf {
    let p = dir.join(name);
    let mut v = vec![0u8; len];
    let mut x: u32 = 0x9e3779b9 ^ seed as u32;
    for b in v.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    std::fs::write(&p, v).unwrap();
    p
}

pub struct Outcome {
    pub send: Result<Summary>,
    pub recv: Result<Summary>,
    pub sender_verified: Vec<u32>,
}

/// Run a full transfer. `flow_a`: server sends (QR on sender). Otherwise server receives.
pub fn transfer(
    flow_a: bool,
    items: Vec<SendItem>,
    out: &Path,
    sopts: EngineOptions,
    ropts_engine: EngineOptions,
    accept: bool,
) -> Outcome {
    let mode = if flow_a { ServerMode::Send } else { ServerMode::Receive };
    let srv = server(mode);
    let qr = qr_local(&srv);
    let out = out.to_path_buf();
    let ropts = ReceiveOptions { out_dir: out, engine: ropts_engine };
    let mut decide = move |_: &OfferInfo| accept;
    if flow_a {
        let h = std::thread::spawn(move || {
            let mut verified = vec![];
            let ctrl = srv.wait_control()?;
            let r = run_sender(ctrl, Plane::server(&srv), items, &sopts, &mut |e| {
                if let Event::FileVerified { id } = e {
                    verified.push(id)
                }
            });
            r.map(|s| (s, verified))
        });
        let recv = Client::connect(&qr, "test-client", ropts.engine.cancel.clone(), ropts.engine.stats.clone())
            .and_then(|(c, ctrl)| run_receiver(ctrl, Plane::Client(Arc::new(c)), &ropts, &mut decide, &mut |_| {}));
        let send = h.join().unwrap();
        let (send, sender_verified) = match send {
            Ok((s, v)) => (Ok(s), v),
            Err(e) => (Err(e), vec![]),
        };
        Outcome { send, recv, sender_verified }
    } else {
        let h = std::thread::spawn(move || {
            let ctrl = srv.wait_control()?;
            run_receiver(ctrl, Plane::server(&srv), &ropts, &mut decide, &mut |_| {})
        });
        let mut verified = vec![];
        let send =
            Client::connect(&qr, "test-client", sopts.cancel.clone(), sopts.stats.clone()).and_then(|(c, ctrl)| {
                run_sender(ctrl, Plane::Client(Arc::new(c)), items, &sopts, &mut |e| {
                    if let Event::FileVerified { id } = e {
                        verified.push(id)
                    }
                })
            });
        let recv = h.join().unwrap();
        Outcome { send, recv, sender_verified: verified }
    }
}

pub fn same(a: &Path, b: &Path) -> bool {
    std::fs::read(a).unwrap() == std::fs::read(b).unwrap()
}

pub fn no_parts(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".fliq.part"))
}

pub fn is<T: std::fmt::Debug>(r: &Result<T>, f: fn(&FliqError) -> bool) -> bool {
    match r {
        Err(e) => f(e),
        Ok(_) => false,
    }
}
