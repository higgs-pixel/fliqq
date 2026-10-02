use crossbeam_channel::{Receiver, unbounded};
use fliq_core::service::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn cfg(dir: &Path) -> AppConfig {
    let mut c = AppConfig::new("dev", dir.to_path_buf());
    c.loopback = true;
    c
}

fn sink() -> (Sink, Receiver<UiEvent>) {
    let (tx, rx) = unbounded();
    (
        Arc::new(move |e| {
            let _ = tx.send(e);
        }),
        rx,
    )
}

fn local_uri(uri: &str) -> String {
    let mut q = fliq_core::qr::QrPayload::from_uri(uri).unwrap();
    q.ip = vec!["127.0.0.1".into()];
    q.to_uri()
}

fn next(rx: &Receiver<UiEvent>, pred: impl Fn(&UiEvent) -> bool) -> UiEvent {
    loop {
        let e = rx.recv_timeout(Duration::from_secs(20)).expect("event timeout");
        if pred(&e) {
            return e;
        }
        if let UiEvent::Failed { code, message } = &e {
            panic!("unexpected failure {code}: {message}");
        }
    }
}

#[test]
fn host_send_join_receive_with_events() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = src.path().join("photo.jpg");
    std::fs::write(&p, vec![9u8; 5_000_000]).unwrap();
    let (hs, hrx) = sink();
    let _host = host(cfg(src.path()), vec![p.clone()], hs).unwrap();
    let UiEvent::ShowQr { uri, .. } = next(&hrx, |e| matches!(e, UiEvent::ShowQr { .. })) else { unreachable!() };
    let UiEvent::SelfCheck { reachable, .. } = next(&hrx, |e| matches!(e, UiEvent::SelfCheck { .. })) else {
        unreachable!()
    };
    assert!(reachable.contains(&"127.0.0.1".to_string()));
    let (js, jrx) = sink();
    let join_h = join(cfg(dst.path()), &local_uri(&uri), vec![], js).unwrap();
    let UiEvent::OfferReceived { code, files, .. } = next(&jrx, |e| matches!(e, UiEvent::OfferReceived { .. })) else {
        unreachable!()
    };
    assert_eq!(files, vec![NamedSize { name: "photo.jpg".into(), size: 5_000_000 }]);
    let UiEvent::Connected { code: host_code, .. } = next(&hrx, |e| matches!(e, UiEvent::Connected { .. })) else {
        unreachable!()
    };
    assert_eq!(code, host_code);
    join_h.decide(true);
    let UiEvent::Finished { files, .. } = next(&jrx, |e| matches!(e, UiEvent::Finished { .. })) else { unreachable!() };
    next(&hrx, |e| matches!(e, UiEvent::Finished { .. }));
    let saved = files[0].path.clone().unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), std::fs::read(&p).unwrap());
    assert!(p.exists(), "copy mode must keep the source");
}

#[test]
fn move_removes_source_only_after_verification() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = src.path().join("doc.pdf");
    std::fs::write(&p, vec![1u8; 300_000]).unwrap();
    let (hs, hrx) = sink();
    let host_h = host(cfg(dst.path()), vec![], hs).unwrap();
    let UiEvent::ShowQr { uri, .. } = next(&hrx, |e| matches!(e, UiEvent::ShowQr { .. })) else { unreachable!() };
    let mut c = cfg(src.path());
    c.move_sources = true;
    let (js, jrx) = sink();
    let _j = join(c, &local_uri(&uri), vec![p.clone()], js).unwrap();
    next(&hrx, |e| matches!(e, UiEvent::OfferReceived { .. }));
    assert!(p.exists(), "source removed before the receiver accepted");
    host_h.decide(true);
    next(&jrx, |e| matches!(e, UiEvent::FileVerified { .. }));
    next(&jrx, |e| matches!(e, UiEvent::Finished { .. }));
    assert!(!p.exists(), "move did not remove the source");
    assert!(dst.path().join("doc.pdf").exists());
}

#[test]
fn decline_fails_both_sides_and_keeps_source() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = src.path().join("a.bin");
    std::fs::write(&p, b"abc").unwrap();
    let (hs, hrx) = sink();
    let mut c = cfg(src.path());
    c.move_sources = true;
    let _h = host(c, vec![p.clone()], hs).unwrap();
    let UiEvent::ShowQr { uri, .. } = next(&hrx, |e| matches!(e, UiEvent::ShowQr { .. })) else { unreachable!() };
    let (js, jrx) = sink();
    let j = join(cfg(dst.path()), &local_uri(&uri), vec![], js).unwrap();
    next(&jrx, |e| matches!(e, UiEvent::OfferReceived { .. }));
    j.decide(false);
    let failed = |rx: &Receiver<UiEvent>| loop {
        if let UiEvent::Failed { code, .. } = rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            break code;
        }
    };
    assert_eq!(failed(&jrx), "E_REJECTED");
    assert_eq!(failed(&hrx), "E_REJECTED");
    assert!(p.exists());
}

#[test]
fn cancel_while_waiting_for_scan() {
    let d = tempfile::tempdir().unwrap();
    let (hs, hrx) = sink();
    let h = host(cfg(d.path()), vec![], hs).unwrap();
    next(&hrx, |e| matches!(e, UiEvent::ShowQr { .. }));
    h.cancel();
    loop {
        if let UiEvent::Failed { code, .. } = hrx.recv_timeout(Duration::from_secs(5)).unwrap() {
            assert_eq!(code, "E_CANCELLED");
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(100));
    assert!(h.is_finished());
}

#[test]
fn join_rejects_bad_codes_immediately() {
    let d = tempfile::tempdir().unwrap();
    let (js, _rx) = sink();
    assert!(matches!(join(cfg(d.path()), "hello", vec![], js), Err(fliq_core::error::FliqError::QrInvalid)));
}

#[test]
fn self_check_never_counts_as_failed_handshake() {
    use fliq_core::qr::ServerMode;
    use fliq_core::session::*;
    let mut sc = ServerConfig::new(ServerMode::Send, "s");
    sc.link = fliq_core::net::TcpLink { preferred: None, include_loopback: true };
    let s = Server::start(sc, CancelToken::new(), Arc::new(fliq_core::stats::StageStats::default())).unwrap();
    for _ in 0..5 {
        let (ok, _) = s.self_check();
        assert!(ok.contains(&"127.0.0.1".to_string()));
    }
    assert_eq!(s.failed_handshakes(), 0);
    let fake: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
    assert!(!probe(fake, Duration::from_millis(300)));
}
