# Decisions needing the owner

Each item is implemented the way described so work could continue. All are easy to change.

1. **PSK length (spec 5.2 vs 6.1).** The QR carries a 16-byte PSK; Noise requires 32 bytes.
   Implemented: `psk32 = HKDF-SHA256(salt = sid, ikm = psk16, info = "Fliq/1 noise-psk")`.
   Alternative: put a 32-byte PSK in the QR (+16 bytes, about +22 URI characters).

2. **When the PSK is destroyed (spec 5.3 vs 6.3/7.7).** 5.3 says destroy it after the first
   successful handshake; 6.3 needs it for every data-stream handshake, and 7.7 needs it to
   re-dial. Implemented: the QR is single-use for the *control* stream (a second control
   handshake is refused), the PSK stays in memory (zeroized on drop) until the session ends,
   and every data stream must also present `HKDF(control_hash, "data-token")`. After 3 failed
   handshakes the PSK is destroyed immediately.

3. **Failed-handshake counting covers all connections.** A device on the same network that
   sends garbage to the port 3 times locks the session (denial of service, not a breach). This
   follows 5.3 literally. Alternative: count only before the control handshake succeeds.

4. **Pipeline shape.** Decryption must happen on the thread that owns each Noise stream, so the
   receiver does decrypt + verify + positional write per stream thread rather than a separate
   writer stage. Throughput and memory are measured in `docs/BENCHMARKS.md`; this can be split
   if a phone benchmark shows the write stage blocking the network.

5. **Milestone order.** M1 was started before M0's iperf3 results, because the localhost work
   does not depend on them.

6. **Reconnect window is a setting.** `EngineOptions.linger` defaults to the spec's 60 s;
   tests use 3 s. Up to 8 control reconnects per session.

7. **Temporary file suffix.** `.fliq.part` instead of `.part`, so start-up clean-up of crashed
   sessions never touches other programs' `.part` files.

## Added in M2 (need the owner)

8. **Manual mode (spec 4.2 method 3) is blocked.** IP + port + a 6-digit code cannot carry the
   server key and a strong secret, and inventing a PAKE is not allowed (rule 4). Options:
   (a) a longer typed code (~40 characters: 128-bit secret + key fingerprint);
   (b) 6-digit secret + learn the key over the network + **mandatory** comparison of the
       verification code on both screens (numeric-comparison model, like Bluetooth);
   (c) postpone; the M2 exit test then uses QR only. Nothing is built until you choose.

9. **Firewall self-check cannot detect a block on its own.** Windows does not apply inbound
   firewall rules to a PC connecting to its own address, so the spec's self-connect test
   passes even when other devices are blocked. Implemented: the self-connect probe (proves
   the listener and addresses) **plus** a check that an inbound rule named "Fliq" exists for
   this exe (`netsh advfirewall firewall show rule`). A missing rule shows the warning and
   the one-click Fix (elevated `netsh`).

10. **Display details.** Max Single Payload is shown as "50.00 GiB" (the limit is 50 GiB =
    53.69 GB). Icons are Material outlined (one consistent set, no extra package) instead of
    Lucide. "Measured Engine Ceiling" shows "Not measured yet" until the M4 link test exists.
