mod common;
use common::*;
use fliq_core::consts::CHUNK_SIZE;
use fliq_core::engine::*;
use fliq_core::error::FliqError;
use std::sync::Arc;

const C: usize = CHUNK_SIZE as usize;

fn roundtrip(flow_a: bool, sizes: &[usize]) {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let paths: Vec<_> =
        sizes.iter().enumerate().map(|(i, &n)| write_file(src.path(), &format!("f{i}.bin"), n, i as u8)).collect();
    let items = paths.iter().map(|p| SendItem::from_path(p).unwrap()).collect();
    let o = transfer(flow_a, items, dst.path(), opts(), opts(), true);
    let recv = o.recv.expect("receiver ok");
    o.send.expect("sender ok");
    assert_eq!(o.sender_verified.len(), sizes.len());
    for (p, r) in paths.iter().zip(&recv.files) {
        let got = r.path.as_ref().unwrap();
        assert_eq!(got.file_name(), p.file_name());
        assert!(same(p, got), "content mismatch for {got:?}");
    }
    assert!(no_parts(dst.path()));
}

#[test]
fn edge_sizes_flow_a() {
    roundtrip(true, &[0, 1, C, C + 1, 3 * C + 12345]);
}

#[test]
fn edge_sizes_flow_b() {
    roundtrip(false, &[0, 1, C, C + 1]);
}

#[test]
fn many_small_files() {
    let sizes: Vec<usize> = (0..60).map(|i| i * 997 + 1).collect();
    roundtrip(true, &sizes);
}

#[test]
fn larger_file_parallel_streams() {
    roundtrip(false, &[24 * C + 777]);
}

#[test]
fn hostile_names_are_sanitized_and_collisions_renamed() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    std::fs::write(dst.path().join("evil.txt"), b"existing").unwrap();
    let p = write_file(src.path(), "a.bin", 1000, 1);
    let mk = |name: &str| SendItem::from_file(std::fs::File::open(&p).unwrap(), name.into()).unwrap();
    let items = vec![mk("../../evil.txt"), mk("..\\..\\evil.txt"), mk("CON"), mk("x/../../")];
    let o = transfer(true, items, dst.path(), opts(), opts(), true);
    let r = o.recv.unwrap();
    let names: Vec<String> =
        r.files.iter().map(|f| f.path.as_ref().unwrap().file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["evil (1).txt", "evil (2).txt", "_CON", "file"]);
    for f in &r.files {
        assert_eq!(f.path.as_ref().unwrap().parent().unwrap(), dst.path());
    }
    assert_eq!(std::fs::read(dst.path().join("evil.txt")).unwrap(), b"existing");
}

#[test]
fn receiver_declines() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = write_file(src.path(), "a.bin", 5000, 1);
    let o = transfer(true, vec![SendItem::from_path(&p).unwrap()], dst.path(), opts(), opts(), false);
    assert!(is(&o.send, |e| matches!(e, FliqError::Rejected)));
    assert!(is(&o.recv, |e| matches!(e, FliqError::Rejected)));
    assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
}

#[test]
fn receiver_enforces_file_count_limit() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let items = (0..3).map(|i| SendItem::from_path(&write_file(src.path(), &format!("{i}"), 10, i)).unwrap()).collect();
    let mut r = opts();
    r.limits.max_files = 2;
    let o = transfer(true, items, dst.path(), opts(), r, true);
    assert!(is(&o.recv, |e| matches!(e, FliqError::Limit(_))));
    assert!(o.send.is_err());
}

#[test]
fn receiver_enforces_size_limit() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = write_file(src.path(), "a", 2 * C, 1);
    let mut r = opts();
    r.limits.max_total = C as u64;
    let o = transfer(false, vec![SendItem::from_path(&p).unwrap()], dst.path(), opts(), r, true);
    assert!(is(&o.recv, |e| matches!(e, FliqError::Limit(_))));
    assert!(o.send.is_err());
}

#[test]
fn recovers_from_dropped_data_stream() {
    for flow_a in [true, false] {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let p = write_file(src.path(), "big.bin", 20 * C + 5, 9);
        let mut s = opts();
        s.fault = Some(Arc::new(Fault { kill_stream_after_chunks: 4, ..Default::default() }));
        let o = transfer(flow_a, vec![SendItem::from_path(&p).unwrap()], dst.path(), s, opts(), true);
        let r = o.recv.expect("receiver recovers");
        let sd = o.send.expect("sender recovers");
        // The client re-dialed at least one replacement stream.
        let client_side = if flow_a { &r } else { &sd };
        assert!(client_side.stages.streams_opened >= 5, "no re-dial: {}", client_side.stages.streams_opened);
        assert!(same(&p, r.files[0].path.as_ref().unwrap()));
        assert!(no_parts(dst.path()));
    }
}

#[test]
fn cancel_mid_transfer_cleans_up() {
    let dst = tempfile::tempdir().unwrap();
    let block = synthetic_block(1);
    let item = SendItem { source: Source::Synthetic(block), name: "s.bin".into(), size: 400 * CHUNK_SIZE, mtime: 0 };
    let r = opts();
    let cancel = r.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        cancel.cancel();
    });
    let o = transfer(true, vec![item], dst.path(), opts(), r, true);
    assert!(o.recv.is_err());
    assert!(o.send.is_err());
    assert!(no_parts(dst.path()), "partial file left behind");
    assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
}

#[test]
fn stale_parts_are_swept() {
    let dst = tempfile::tempdir().unwrap();
    std::fs::write(dst.path().join("old.bin.fliq.part"), b"junk").unwrap();
    std::fs::write(dst.path().join("keep.part"), b"not ours").unwrap();
    assert_eq!(sweep_stale_parts(dst.path()), 1);
    assert!(dst.path().join("keep.part").exists());
}

fn control_drop(flow_a: bool, also_data: bool) {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let p = write_file(src.path(), "big.bin", 30 * C + 9, 3);
    let mut s = opts();
    s.fault = Some(Arc::new(Fault {
        kill_control_after_chunks: 6,
        kill_stream_after_chunks: if also_data { 6 } else { 0 },
        ..Default::default()
    }));
    let o = transfer(flow_a, vec![SendItem::from_path(&p).unwrap()], dst.path(), s, opts(), true);
    let r = o.recv.expect("receiver survives control drop");
    let sd = o.send.expect("sender survives control drop");
    assert!(r.control_reconnects >= 1 && sd.control_reconnects >= 1, "control was never re-established");
    assert_eq!(o.sender_verified, vec![0]);
    assert!(same(&p, r.files[0].path.as_ref().unwrap()));
    assert!(no_parts(dst.path()));
}

#[test]
fn recovers_from_dropped_control_stream_flow_a() {
    control_drop(true, false);
}

#[test]
fn recovers_from_dropped_control_stream_flow_b() {
    control_drop(false, false);
}

#[test]
fn recovers_when_control_and_data_drop_together() {
    control_drop(true, true);
    control_drop(false, true);
}
