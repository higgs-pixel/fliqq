# Architecture

```
 Flutter UI (M2/M3)  ── flutter_rust_bridge: control calls + progress only ──┐
                                                                             ▼
 fliq_core
   qr.rs        fliq:// URI, CBOR payload, validation, expiry
   noise.rs     NKpsk0 handshake, record layer (65,519-byte records, 2-byte length)
   kdf.rs       HKDF-SHA256: verification code, data token, PSK expansion
   session.rs   Server (shows QR, listens) / Client (scans, dials); control + data streams
   msg.rs       CBOR control messages, data frame header, offer validation, manifest hash
   engine.rs    streaming transfer pipeline, scheduler, repair, receiver safety
   sanitize.rs  file-name rules, collision renaming, .fliq.part paths
   net.rs       Link trait, address ranking, port binding, socket tuning, happy eyeballs
   fsutil.rs    positional read/write, free space, FAT32 detection, peak RSS
   stats.rs     per-stage timers, bottleneck heuristic
 fliq_cli       serve-send / serve-recv / connect / bench / crypto-bench
```

## Roles
The device that shows the QR is the TCP **server** and Noise responder; the scanner is the
**client** and initiator. Sender/receiver is independent: `QrPayload.mode` says what the server
will do, and the server confirms it in `Role` right after `Hello`.

## Threads in one transfer

Sender
- 1 disk reader: reads 4 MiB chunks in order (resend requests first), takes a buffer from a
  fixed pool. The pool size is `INFLIGHT_BUDGET_BYTES / 4 MiB` (default 16 buffers = 64 MiB);
  an empty pool is the backpressure.
- 1 worker per data stream: takes the next chunk from the shared bounded queue (so a slow
  stream never stalls the others), hashes it, encrypts it, writes it, returns the buffer.
  On a socket error it puts the chunk back on the queue and exits.
- 1 control reader, 1 coordinator (FileDone, Ack handling, Verified, progress, timeouts).

Receiver
- 1 worker per data stream: read header, decrypt payload, check the chunk hash, write at
  `chunk_index × 4 MiB` with a positional write, mark the bitmap. Duplicates from repair
  rounds are skipped if their hash matches.
- 1 control reader, 1 coordinator: on `FileDone` plus a complete bitmap it recomputes the
  manifest, fsyncs, renames `.fliq.part` to the final name and sends `Verified`. If a file
  stays incomplete for 1.5 s after `FileDone`, it sends `Ack` with its bitmap and the sender
  resends what is missing.

Stream manager: on the client it keeps `streams` data connections open and re-dials when one
dies; on the server it starts a worker for each authenticated data connection.

## Memory
Sender: pool (64 MiB default) + ~1.3 MiB per stream. Receiver: one 4 MiB buffer + ~0.3 MiB
per stream. Measured peak RSS: 76 MB sender process, 22 MB receiver process (1 GiB file,
4 streams). Files are never loaded whole.

## Control stream recovery
`CtrlLink` owns the control writer, a generation number and the way to get a new stream back
(client: `Client::resume_control`; server: the `resumed_controls` channel). Each control
reader thread tags its messages with its generation, so errors from a replaced stream are
ignored. On loss the client re-dials (every 300 ms) and the server waits, both for
`EngineOptions.linger` (60 s); the server also adopts a resumed stream whenever one arrives.
At most 8 reconnects per session.

## Failure handling
Any error, including the user's cancel: the coordinator sends `Error{code}` to the peer
first (so it fails at once instead of trying to reconnect), shuts every data socket, joins
the workers, and the receiver's `PartGuard` deletes every non-finalized `.fliq.part`. On
start, the receiver deletes leftover `*.fliq.part` files from crashed sessions.

## Platform boundary
The core never calls Android or Windows APIs. Platform code (M2/M3) supplies: a `File` from an
Android fd (`SendItem::from_file`), the hotspot address for `TcpLink.preferred`, Wi-Fi join
before `Client::connect`, and Move deletion after `Event::FileVerified`.

## App layer (M2)
`fliq_core::service` is the UI-facing API: `host(config, files, sink)` / `join(config, code,
files, sink)` start a session thread and return a `SessionHandle` (`decide`, `cancel`). All
progress arrives as `UiEvent`s. `app/rust/src/api/fliq.rs` exposes this through
flutter_rust_bridge as `FliqSession.host/join` returning a Dart `Stream<FliqEvent>` (a flat
struct + plain enum, so no Dart code generator besides FRB is needed). The Flutter side
(`TransferController`) maps events to screens; settings and Transfer Logs are JSON files in
the app-support folder. Move is executed in Rust after `Verified` (Windows Recycle Bin via
`SHFileOperationW`).
