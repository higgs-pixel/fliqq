# Benchmarks

All numbers below come from `fliq_cli`. MB/s is decimal (1 MB = 10^6 bytes).

## Sandbox (Claude), 2026-10-02
Machine: Linux VM, **1 CPU core**, loopback only (no Wi-Fi). Both sides compete for the same
core, so these show the CPU ceiling of the protocol, not link speed. Stage percentages are
thread-seconds per wall-second and exceed 100 % because threads share one core.

`fliq_cli crypto-bench` (single core): BLAKE3 4,827 MB/s · Noise ChaCha20-Poly1305 encrypt
672 MB/s · decrypt 629 MB/s.

| Test | Size | Streams | Budget | MB/s | Peak RSS | Verified |
|---|---|---|---|---|---|---|
| bench, disk → disk, one process | 1 GiB | 4 | 64 MiB | 248 | 98 MB (both sides) | manifest OK |
| bench, synthetic → disk, one process | 5 GiB | 4 | 64 MiB | 275 | 99 MB (both sides) | manifest OK |
| two processes, `serve-send` / `connect` | 3 GiB | 4 | 64 MiB | 268–273 | — | SHA-256 identical |
| two processes | 1 GiB | 4 | 64 MiB | 224–226 | 76 MB sender / 22 MB receiver | `cmp` identical |
| sweep, synthetic → disk | 2 GiB | 1 | 64 MiB | 278 | 81 MB | OK |
| | 2 GiB | 2 | 64 MiB | 287 | 87 MB | OK |
| | 2 GiB | 4 | 64 MiB | 245 | 99 MB | OK |
| | 2 GiB | 8 | 64 MiB | 240 | 122 MB | OK |
| | 2 GiB | 4 | 16 MiB | 275 | 48 MB | OK |

Reading: with one core shared by both ends, the theoretical ceiling is about 280 MB/s
(encrypt + decrypt + two hashes + write per byte), and the engine reaches 85–100 % of it.
More streams cost a little here only because of thread contention on one core; on real
hardware, extra streams matter for Wi-Fi. A 16 MiB budget did not cost throughput in this
test. The stream-count and budget defaults should be decided from the device runs below.

## Recovery checks (sandbox, two processes)
| Test | Result |
|---|---|
| Receiver killed (SIGKILL) 1.5 s into a 1 GiB transfer | Sender showed "Connection lost. Reconnecting…", failed after 60 s with `E_CONN_LOST` |
| Next receive into the same folder | Stale `src.bin.fliq.part` removed; new file received identical |
| Control stream cut after 6 of 30 chunks (automated, both flows, with and without a data stream cut) | Files identical; both sides report ≥ 1 control reconnect |

## Device pairs (owner) — to fill in
| Pair | Link | iperf3 MB/s | Fliq MB/s | % of iperf3 | File | Notes |
|---|---|---|---|---|---|---|
| | | | | | | |
