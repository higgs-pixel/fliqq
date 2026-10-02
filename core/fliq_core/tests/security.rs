mod common;
use common::*;
use fliq_core::consts::*;
use fliq_core::engine::*;
use fliq_core::error::FliqError;
use fliq_core::msg::*;
use fliq_core::net::TcpLink;
use fliq_core::qr::ServerMode;
use fliq_core::session::*;
use fliq_core::stats::StageStats;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn connect(q: &fliq_core::qr::QrPayload) -> fliq_core::error::Result<(Client, Control)> {
    Client::connect(q, "c", CancelToken::new(), Arc::new(StageStats::default()))
}

#[test]
fn wrong_psk_is_rejected_and_counted() {
    let s = server(ServerMode::Send);
    let mut q = qr_local(&s);
    q.psk[0] ^= 1;
    assert!(is(&connect(&q), |e| matches!(e, FliqError::Handshake | FliqError::ConnectionLost)));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(s.failed_handshakes(), 1);
}

#[test]
fn fourth_attempt_after_three_failures_is_locked_out() {
    let s = server(ServerMode::Send);
    let good = qr_local(&s);
    for _ in 0..3 {
        let mut q = good.clone();
        q.psk[0] ^= 0x55;
        assert!(connect(&q).is_err());
    }
    std::thread::sleep(Duration::from_millis(200));
    assert!(is(&s.wait_control(), |e| matches!(e, FliqError::Locked)));
    assert!(connect(&good).is_err(), "PSK must be destroyed after 3 failures");
}

#[test]
fn expired_code_is_refused_by_both_sides() {
    let mut cfg = ServerConfig::new(ServerMode::Send, "s");
    cfg.link = TcpLink { preferred: None, include_loopback: true };
    cfg.ttl_secs = 0;
    let s = Server::start(cfg, CancelToken::new(), Arc::new(StageStats::default())).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let q = qr_local(&s);
    assert!(is(&connect(&q), |e| matches!(e, FliqError::QrExpired)));
    let mut forged = q.clone();
    forged.exp += 3600; // client is fooled, server is not
    assert!(connect(&forged).is_err());
}

#[test]
fn qr_is_single_use() {
    let s = server(ServerMode::Send);
    let q = qr_local(&s);
    let (_c, _ctrl) = connect(&q).unwrap();
    let _server_ctrl = s.wait_control().unwrap();
    assert!(connect(&q).is_err());
}

#[test]
fn verification_codes_match() {
    let s = server(ServerMode::Receive);
    let q = qr_local(&s);
    let (_c, ctrl) = connect(&q).unwrap();
    let sc = s.wait_control().unwrap();
    assert_eq!(ctrl.verification_code, sc.verification_code);
    assert_eq!(ctrl.verification_code.len(), 6);
    assert_eq!(sc.peer_name, "c");
    assert_eq!(ctrl.peer_name, "test-server");
}

#[test]
fn data_stream_token_and_limits() {
    let s = server(ServerMode::Send);
    let q = qr_local(&s);
    let (c, _ctrl) = connect(&q).unwrap();
    let _sc = s.wait_control().unwrap();
    assert!(c.dial_data_with_token(1, &[0u8; 32]).is_err(), "bad token accepted");
    assert!(c.dial_data(0).is_err(), "index 0 accepted");
    let mut held = vec![];
    for i in 1..=MAX_DATA_STREAMS {
        held.push(c.dial_data(i).unwrap_or_else(|e| panic!("stream {i}: {e}")));
    }
    assert!(c.dial_data(MAX_DATA_STREAMS + 1).is_err(), "17th stream accepted");
    assert!(c.dial_data(3).is_err(), "duplicate live index accepted");
    // Server side drops its end -> slot is free again.
    let rx = s.data_conns();
    let mut server_side: Vec<DataConn> = (0..MAX_DATA_STREAMS).map(|_| rx.recv().unwrap()).collect();
    let pos = server_side.iter().position(|d| d.index == 3).unwrap();
    drop(server_side.remove(pos));
    assert!(c.dial_data(3).is_ok(), "slot not released");
}

type ConnLogs = Arc<Mutex<Vec<Arc<Mutex<Vec<u8>>>>>>;

#[test]
fn control_resume_requires_session_token() {
    let s = server(ServerMode::Send);
    let q = qr_local(&s);
    let (c, _ctrl) = connect(&q).unwrap();
    let _sc = s.wait_control().unwrap();
    let resumed = s.resumed_controls();
    assert!(c.resume_control_with_token(&[7u8; 32]).is_err(), "resume with wrong token accepted");
    assert!(resumed.recv_timeout(Duration::from_millis(300)).is_err());
    let ok = c.resume_control().unwrap();
    let got = resumed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(ok.verification_code, got.verification_code);
}

/// Forwarding proxy that records the client->server bytes of every connection.
fn recording_proxy(target: u16) -> (u16, ConnLogs) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let logs: ConnLogs = Arc::default();
    let logs2 = logs.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let up = TcpStream::connect(("127.0.0.1", target)).unwrap();
            let log = Arc::new(Mutex::new(vec![]));
            logs2.lock().unwrap().push(log.clone());
            let (mut c2, mut up2) = (c.try_clone().unwrap(), up.try_clone().unwrap());
            let mut c = c;
            let mut up = up;
            std::thread::spawn(move || {
                let mut b = [0u8; 65536];
                while let Ok(n) = c.read(&mut b) {
                    if n == 0 || up.write_all(&b[..n]).is_err() {
                        break;
                    }
                    log.lock().unwrap().extend_from_slice(&b[..n]);
                }
            });
            std::thread::spawn(move || {
                let mut b = [0u8; 65536];
                while let Ok(n) = up2.read(&mut b) {
                    if n == 0 || c2.write_all(&b[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    (port, logs)
}

#[test]
fn replayed_data_stream_handshake_is_rejected() {
    let s = server(ServerMode::Send);
    let mut q = qr_local(&s);
    let real_port = q.port;
    let (pport, logs) = recording_proxy(real_port);
    q.port = pport;
    let (c, _ctrl) = connect(&q).unwrap();
    let _sc = s.wait_control().unwrap();
    let rx = s.data_conns();
    let _d = c.dial_data(1).unwrap();
    let _legit = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let recorded = logs.lock().unwrap()[1].lock().unwrap().clone();
    let before = s.failed_handshakes();
    let mut replay = TcpStream::connect(("127.0.0.1", real_port)).unwrap();
    replay.write_all(&recorded).unwrap();
    replay.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut sink = vec![];
    let _ = replay.read_to_end(&mut sink);
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "replayed stream was accepted");
    assert_eq!(s.failed_handshakes(), before + 1);
}

// ---------------------------------------------------------------- malicious sender

/// Server receives; the test drives a hand-written (hostile) sender over the client side.
fn hostile_sender(
    file_size: u64,
    act: impl FnOnce(&mut Control, &Client, &FileMeta),
) -> (fliq_core::error::Result<Summary>, tempfile::TempDir) {
    let dst = tempfile::tempdir().unwrap();
    let s = server(ServerMode::Receive);
    let q = qr_local(&s);
    let out = dst.path().to_path_buf();
    let h = std::thread::spawn(move || {
        let ctrl = s.wait_control().unwrap();
        let mut engine = opts();
        engine.linger = Duration::from_secs(3); // keep the reconnect wait short in tests
        let ropts = ReceiveOptions { out_dir: out, engine };
        run_receiver(ctrl, Plane::server(&s), &ropts, &mut |_| true, &mut |_| {})
    });
    let (c, mut ctrl) = connect(&q).unwrap();
    let meta =
        FileMeta { id: 7, name: "x.bin".into(), size: file_size, mtime: 0, chunk_count: chunk_count(file_size) as u32 };
    send_ctrl(&mut ctrl.writer, &Ctrl::Offer { files: vec![meta.clone()] }).unwrap();
    assert!(matches!(recv_ctrl(&mut ctrl.reader).unwrap(), Ctrl::Accept { .. }));
    act(&mut ctrl, &c, &meta);
    (h.join().unwrap(), dst)
}

fn send_chunk(d: &mut DataConn, hdr: ChunkHeader, payload: &[u8]) {
    let _ = d.writer.write(&hdr.encode());
    let _ = d.writer.write(payload);
    let _ = d.writer.flush();
}

#[test]
fn oversized_chunk_rejected() {
    let (r, dst) = hostile_sender(100, |_, c, m| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![1u8; 200];
        send_chunk(
            &mut d,
            ChunkHeader { file_id: m.id, chunk_index: 0, chunk_hash: *blake3::hash(&p).as_bytes(), len: 200 },
            &p,
        );
        std::thread::sleep(Duration::from_millis(300));
    });
    assert!(is(&r, |e| matches!(e, FliqError::Protocol(_))));
    assert!(no_parts(dst.path()));
}

#[test]
fn chunk_index_out_of_range_rejected() {
    let (r, dst) = hostile_sender(100, |_, c, m| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![1u8; 100];
        send_chunk(
            &mut d,
            ChunkHeader { file_id: m.id, chunk_index: 5, chunk_hash: *blake3::hash(&p).as_bytes(), len: 100 },
            &p,
        );
        std::thread::sleep(Duration::from_millis(300));
    });
    assert!(is(&r, |e| matches!(e, FliqError::Protocol(_))));
    assert!(no_parts(dst.path()));
}

#[test]
fn unknown_file_id_rejected() {
    let (r, _dst) = hostile_sender(100, |_, c, _| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![1u8; 100];
        send_chunk(
            &mut d,
            ChunkHeader { file_id: 99, chunk_index: 0, chunk_hash: *blake3::hash(&p).as_bytes(), len: 100 },
            &p,
        );
        std::thread::sleep(Duration::from_millis(300));
    });
    assert!(is(&r, |e| matches!(e, FliqError::Protocol(_))));
}

#[test]
fn chunk_hash_mismatch_rejected() {
    let (r, dst) = hostile_sender(100, |_, c, m| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![1u8; 100];
        send_chunk(&mut d, ChunkHeader { file_id: m.id, chunk_index: 0, chunk_hash: [0u8; 32], len: 100 }, &p);
        std::thread::sleep(Duration::from_millis(300));
    });
    assert!(is(&r, |e| matches!(e, FliqError::HashMismatch)));
    assert!(no_parts(dst.path()));
}

#[test]
fn manifest_mismatch_deletes_file() {
    let (r, dst) = hostile_sender(100, |ctrl, c, m| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![1u8; 100];
        send_chunk(
            &mut d,
            ChunkHeader { file_id: m.id, chunk_index: 0, chunk_hash: *blake3::hash(&p).as_bytes(), len: 100 },
            &p,
        );
        send_ctrl(&mut ctrl.writer, &Ctrl::FileDone { id: m.id, manifest_hash: vec![0u8; 32] }).unwrap();
        std::thread::sleep(Duration::from_millis(300));
    });
    assert!(is(&r, |e| matches!(e, FliqError::HashMismatch)));
    assert!(no_parts(dst.path()));
    assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0, "damaged file kept");
}

#[test]
fn truncated_transfer_never_finalizes() {
    // Sender sends half the chunks, announces FileDone, then vanishes.
    let (r, dst) = hostile_sender(2 * CHUNK_SIZE, |ctrl, c, m| {
        let mut d = c.dial_data(1).unwrap();
        let p = vec![3u8; CHUNK_SIZE as usize];
        let h = *blake3::hash(&p).as_bytes();
        send_chunk(&mut d, ChunkHeader { file_id: m.id, chunk_index: 0, chunk_hash: h, len: CHUNK_SIZE as u32 }, &p);
        send_ctrl(&mut ctrl.writer, &Ctrl::FileDone { id: m.id, manifest_hash: manifest_hash(&[h, h]).to_vec() })
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        ctrl.shutdown();
    });
    assert!(is(&r, |e| matches!(e, FliqError::ConnectionLost | FliqError::Protocol(_))));
    assert!(no_parts(dst.path()));
    assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
}
