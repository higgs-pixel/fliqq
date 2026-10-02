//! fliq_cli: send / receive / benchmark tool for the Fliq core (spec 13.2).
//!
//!   fliq_cli serve-send FILE... [--move] [--streams N] [--budget 64M]
//!   fliq_cli serve-recv --out DIR [--yes]
//!   fliq_cli connect URI [--out DIR] [--yes] [FILE...] [--move]
//!   fliq_cli bench [--size 1G] [--streams 4] [--budget 64M] [--synthetic] [--dir DIR]
//!   fliq_cli crypto-bench

use fliq_core::consts::*;
use fliq_core::engine::*;
use fliq_core::error::FliqError;
use fliq_core::fsutil;
use fliq_core::net::TcpLink;
use fliq_core::qr::{QrPayload, ServerMode};
use fliq_core::session::*;
use fliq_core::stats::{StageSnapshot, StageStats};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse() -> Args {
        let mut pos = vec![];
        let mut flags = vec![];
        let mut it = std::env::args().skip(1).peekable();
        const VALUED: &[&str] = &["--out", "--streams", "--budget", "--size", "--dir", "--name"];
        while let Some(a) = it.next() {
            if a.starts_with("--") {
                let v = if VALUED.contains(&a.as_str()) { it.next() } else { None };
                flags.push((a, v));
            } else {
                pos.push(a);
            }
        }
        Args { pos, flags }
    }
    fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == f)
    }
    fn get(&self, f: &str) -> Option<&str> {
        self.flags.iter().find(|(k, _)| k == f).and_then(|(_, v)| v.as_deref())
    }
}

fn parse_size(s: &str) -> u64 {
    let s = s.trim().to_ascii_uppercase();
    let (num, mul) = match s.chars().last() {
        Some('K') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M') => (&s[..s.len() - 1], 1 << 20),
        Some('G') => (&s[..s.len() - 1], 1 << 30),
        _ => (&s[..], 1),
    };
    (num.parse::<f64>().expect("bad size") * mul as f64) as u64
}

fn engine_opts(a: &Args, cancel: Arc<CancelToken>, stats: Arc<StageStats>) -> EngineOptions {
    let mut o = EngineOptions::new(cancel, stats);
    if let Some(s) = a.get("--streams") {
        o.streams = s.parse::<u16>().expect("--streams").clamp(1, MAX_DATA_STREAMS);
    }
    if let Some(b) = a.get("--budget") {
        o.inflight_budget = parse_size(b) as usize;
    }
    o
}

fn device_name(a: &Args) -> String {
    a.get("--name").map(String::from).unwrap_or_else(|| std::env::var("HOSTNAME").unwrap_or_else(|_| "Fliq CLI".into()))
}

fn print_progress(e: &Event) {
    match e {
        Event::Reconnecting => eprint!("\n  Connection lost. Reconnecting…"),
        Event::Reconnected => eprintln!(" reconnected."),
        _ => {}
    }
    if let Event::Progress(p) = e {
        let pct = if p.total == 0 { 100.0 } else { p.bytes_done as f64 * 100.0 / p.total as f64 };
        let eta = p.eta_secs.map(|s| format!("{s:.0}s")).unwrap_or_else(|| "--".into());
        eprint!(
            "\r  {pct:5.1}%  {:8.1} MB/s  ETA {eta:>5}  streams {}  limit: {}      ",
            p.mbps,
            p.streams,
            p.bottleneck.as_str()
        );
    }
}

fn decide_cli(yes: bool) -> impl FnMut(&OfferInfo) -> bool {
    move |o: &OfferInfo| {
        eprintln!("\nVerification code: {}  (make sure it matches the other device)", o.verification_code);
        eprintln!("From: {}", o.peer_name);
        for (n, s) in &o.files {
            eprintln!("  {n}  ({:.2} MB)", *s as f64 / 1e6);
        }
        eprintln!("Total {:.2} MB, free {:.2} MB", o.total as f64 / 1e6, o.free_space as f64 / 1e6);
        if o.fat32_warning {
            eprintln!("WARNING: destination looks like FAT32; files over 4 GiB will fail.");
        }
        if yes {
            return true;
        }
        eprint!("Accept? [y/N] ");
        let mut l = String::new();
        let _ = std::io::stdin().read_line(&mut l);
        l.trim().eq_ignore_ascii_case("y")
    }
}

fn report(s: &Summary) {
    eprintln!();
    println!(
        "transferred {:.2} MB in {:.2} s = {:.1} MB/s (resent chunks: {}, control reconnects: {})",
        s.bytes as f64 / 1e6,
        s.secs,
        s.mbps,
        s.chunks_resent,
        s.control_reconnects
    );
    if let Some(r) = fsutil::peak_rss_bytes() {
        println!("peak RSS of this process: {:.1} MB", r as f64 / 1e6);
    }
}

fn items_from(paths: &[String]) -> Vec<SendItem> {
    paths.iter().map(|p| SendItem::from_path(Path::new(p)).unwrap_or_else(|e| die(e))).collect()
}

fn die(e: FliqError) -> ! {
    eprintln!("\nerror [{}]: {e}", e.code());
    std::process::exit(2)
}

fn main() {
    let a = Args::parse();
    let cmd = a.pos.first().cloned().unwrap_or_default();
    let rest: Vec<String> = a.pos.iter().skip(1).cloned().collect();
    match cmd.as_str() {
        "serve-send" => serve(&a, ServerMode::Send, rest),
        "serve-recv" => serve(&a, ServerMode::Receive, rest),
        "connect" => connect(&a, rest),
        "bench" => bench(&a),
        "crypto-bench" => crypto_bench(),
        _ => {
            eprintln!("{}", include_str!("usage.txt"));
            std::process::exit(1)
        }
    }
}

fn delete_moved(paths: &[String], id: u32, mv: bool) {
    if mv {
        // CLI only: the app sends to the Recycle Bin / asks Android for consent instead.
        let _ = std::fs::remove_file(&paths[id as usize]);
    }
}

fn serve(a: &Args, mode: ServerMode, files: Vec<String>) {
    let cancel = CancelToken::new();
    let stats = Arc::new(StageStats::default());
    let mut cfg = ServerConfig::new(mode, &device_name(a));
    cfg.link = TcpLink { preferred: None, include_loopback: a.has("--loopback") };
    let srv = Server::start(cfg, cancel.clone(), stats.clone()).unwrap_or_else(|e| die(e));
    println!("{}", srv.qr().to_uri());
    eprintln!("Show this to the other device only. Expires in {QR_TTL_SECS} s. Addresses: {}", srv.qr().ip.join(", "));
    let ctrl = srv.wait_control().unwrap_or_else(|e| die(e));
    eprintln!("Connected to {} — verification code {}", ctrl.peer_name, ctrl.verification_code);
    let opts = engine_opts(a, cancel, stats);
    let plane = Plane::server(&srv);
    let res = match mode {
        ServerMode::Send => {
            let mv = a.has("--move");
            run_sender(ctrl, plane, items_from(&files), &opts, &mut |e| {
                print_progress(&e);
                if let Event::FileVerified { id } = e {
                    delete_moved(&files, id, mv)
                }
            })
        }
        ServerMode::Receive => {
            let out = PathBuf::from(a.get("--out").unwrap_or("."));
            let ro = ReceiveOptions { out_dir: out, engine: opts };
            run_receiver(ctrl, plane, &ro, &mut decide_cli(a.has("--yes")), &mut |e| print_progress(&e))
        }
    };
    report(&res.unwrap_or_else(|e| die(e)));
}

fn connect(a: &Args, rest: Vec<String>) {
    let uri = rest.first().cloned().unwrap_or_else(|| die(FliqError::QrInvalid));
    let files: Vec<String> = rest.iter().skip(1).cloned().collect();
    let qr = QrPayload::from_uri(&uri).unwrap_or_else(|e| die(e));
    let cancel = CancelToken::new();
    let stats = Arc::new(StageStats::default());
    let (client, ctrl) =
        Client::connect(&qr, &device_name(a), cancel.clone(), stats.clone()).unwrap_or_else(|e| die(e));
    eprintln!("Connected to {} — verification code {}", ctrl.peer_name, ctrl.verification_code);
    let opts = engine_opts(a, cancel, stats);
    let plane = Plane::Client(Arc::new(client));
    let res = match qr.server_mode().unwrap() {
        ServerMode::Send => {
            let out = PathBuf::from(a.get("--out").unwrap_or("."));
            let ro = ReceiveOptions { out_dir: out, engine: opts };
            run_receiver(ctrl, plane, &ro, &mut decide_cli(a.has("--yes")), &mut |e| print_progress(&e))
        }
        ServerMode::Receive => {
            let mv = a.has("--move");
            run_sender(ctrl, plane, items_from(&files), &opts, &mut |e| {
                print_progress(&e);
                if let Event::FileVerified { id } = e {
                    delete_moved(&files, id, mv)
                }
            })
        }
    };
    report(&res.unwrap_or_else(|e| die(e)));
}

fn stage_line(label: &str, s: &StageSnapshot, wall: f64) {
    let p = |x: f64| x / wall * 100.0;
    println!(
        "  {label:<9} read {:5.1}%  hash {:5.1}%  encrypt {:5.1}%  net-send {:5.1}%  net-recv {:5.1}%  decrypt {:5.1}%  verify {:5.1}%  write {:5.1}%",
        p(s.read_s),
        p(s.hash_s),
        p(s.encrypt_s),
        p(s.net_send_s),
        p(s.net_recv_s),
        p(s.decrypt_s),
        p(s.verify_s),
        p(s.write_s)
    );
}

/// Loopback benchmark: both sides in this process, real disk on the receiving side.
fn bench(a: &Args) {
    let size = parse_size(a.get("--size").unwrap_or("1G"));
    let synthetic = a.has("--synthetic");
    let base = a.get("--dir").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let work = tempfile::tempdir_in(&base).expect("temp dir");
    let need = if synthetic { size } else { 2 * size };
    let free = fsutil::free_space(work.path()).unwrap_or(0);
    if need + (256 << 20) > free {
        eprintln!(
            "not enough disk: need {:.2} GB, have {:.2} GB (use --synthetic to skip the source file)",
            need as f64 / 1e9,
            free as f64 / 1e9
        );
        std::process::exit(3);
    }
    let out = work.path().join("out");
    let src_path = work.path().join("source.bin");
    let block = synthetic_block(42);
    if !synthetic {
        eprintln!("writing {:.2} GB test file...", size as f64 / 1e9);
        let mut f = std::io::BufWriter::with_capacity(8 << 20, std::fs::File::create(&src_path).unwrap());
        let mut left = size;
        let mut i = 0u64;
        while left > 0 {
            let n = left.min(CHUNK_SIZE) as usize;
            let mut chunk = block[..n].to_vec();
            for (b, x) in chunk.iter_mut().zip((i + 1).to_le_bytes()) {
                *b ^= x;
            }
            f.write_all(&chunk).unwrap();
            left -= n as u64;
            i += 1;
        }
        f.into_inner().unwrap().sync_all().unwrap();
    }
    let item = if synthetic {
        SendItem { source: Source::Synthetic(block), name: "bench.bin".into(), size, mtime: 0 }
    } else {
        SendItem::from_path(&src_path).unwrap()
    };

    let s_stats = Arc::new(StageStats::default());
    let r_stats = Arc::new(StageStats::default());
    let mut cfg = ServerConfig::new(ServerMode::Send, "bench-sender");
    cfg.link = TcpLink { preferred: None, include_loopback: true };
    let s_cancel = CancelToken::new();
    let srv = Server::start(cfg, s_cancel.clone(), s_stats.clone()).unwrap_or_else(|e| die(e));
    let mut qr = QrPayload::from_uri(&srv.qr().to_uri()).unwrap();
    qr.ip = vec!["127.0.0.1".into()];
    let s_opts = engine_opts(a, s_cancel, s_stats.clone());
    let budget = s_opts.inflight_budget;
    let streams = s_opts.streams;
    let t0 = Instant::now();
    let sender = std::thread::spawn(move || {
        let ctrl = srv.wait_control()?;
        run_sender(ctrl, Plane::server(&srv), vec![item], &s_opts, &mut |_| {})
    });
    let r_cancel = CancelToken::new();
    let (client, ctrl) =
        Client::connect(&qr, "bench-receiver", r_cancel.clone(), r_stats.clone()).unwrap_or_else(|e| die(e));
    let ro = ReceiveOptions { out_dir: out.clone(), engine: engine_opts(a, r_cancel, r_stats.clone()) };
    let recv = run_receiver(ctrl, Plane::Client(Arc::new(client)), &ro, &mut |_| true, &mut |e| print_progress(&e))
        .unwrap_or_else(|e| die(e));
    let send = sender.join().unwrap().unwrap_or_else(|e| die(e));
    let wall = t0.elapsed().as_secs_f64();
    eprintln!();
    let rss = fsutil::peak_rss_bytes().map(|b| format!("{:.1} MB", b as f64 / 1e6)).unwrap_or_else(|| "n/a".into());
    println!("== fliq_cli bench ==");
    println!(
        "  file           {:.3} GiB ({}), {} chunks",
        size as f64 / (1u64 << 30) as f64,
        if synthetic { "synthetic source -> disk" } else { "disk -> disk" },
        size.div_ceil(CHUNK_SIZE)
    );
    println!(
        "  streams        {streams}   in-flight budget {} MiB   cipher {CIPHER_NAME}   {CRYPTO_LABEL}",
        budget >> 20
    );
    println!(
        "  end-to-end     {:.1} MB/s  ({:.2} s, receiver view {:.1} MB/s)",
        size as f64 / wall / 1e6,
        wall,
        recv.mbps
    );
    println!("  verified       manifest hash OK ({} file), resent chunks {}", recv.files.len(), send.chunks_resent);
    println!("  peak RSS       {rss}  (both sides in one process)");
    println!("  cpus           {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    println!("  stage busy (thread-seconds / wall-seconds):");
    stage_line("sender", &send.stages, wall);
    stage_line("receiver", &recv.stages, wall);
    println!(
        "  bottleneck     sender: {}  receiver: {}",
        send.stages.bottleneck().as_str(),
        recv.stages.bottleneck().as_str()
    );
    if a.has("--keep") {
        let _ = work.keep();
    }
}

/// Raw single-core throughput of the primitives on this machine.
fn crypto_bench() {
    let data = vec![7u8; 64 << 20];
    let t = Instant::now();
    let h = blake3::hash(&data);
    let b3 = data.len() as f64 / t.elapsed().as_secs_f64() / 1e6;
    let _ = h;
    let (sk, pk) = fliq_core::noise::generate_static_keypair().unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let th = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        fliq_core::noise::responder(&mut s, &[1; 16], &sk, &[2; 16]).unwrap()
    });
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    let hc = fliq_core::noise::initiator(&mut c, &[1; 16], &pk, &[2; 16]).unwrap();
    let hs = th.join().unwrap();
    let mut w = fliq_core::noise::SecureWriter::new(Vec::with_capacity(70 << 20), hc.transport, None);
    let t = Instant::now();
    w.write(&data).unwrap();
    w.flush().unwrap();
    let enc = data.len() as f64 / t.elapsed().as_secs_f64() / 1e6;
    let wire = w.get_ref().clone();
    let mut r = fliq_core::noise::SecureReader::new(std::io::Cursor::new(wire), hs.transport, None);
    let mut out = vec![0u8; data.len()];
    let t = Instant::now();
    r.read_exact(&mut out).unwrap();
    let dec = data.len() as f64 / t.elapsed().as_secs_f64() / 1e6;
    assert_eq!(out, data);
    println!("single core: BLAKE3 {b3:.0} MB/s   Noise {CIPHER_NAME} encrypt {enc:.0} MB/s   decrypt {dec:.0} MB/s");
}
