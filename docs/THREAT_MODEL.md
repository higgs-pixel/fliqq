# Threat model (v1, core only)

Assets: file contents and names, the QR secrets (PSK, Wi-Fi password), the receiving
device's file system.

| Threat | Mitigation | Test |
|---|---|---|
| Eavesdropper on the Wi-Fi link | Every stream is Noise (X25519, ChaCha20-Poly1305, BLAKE2s). File names and sizes travel only inside it. Ephemeral keys give forward secrecy. | `noise::tests::*` |
| Rogue device connects without scanning | NKpsk0 needs the PSK from the QR; the server's static key authenticates the server. Data streams also need the token bound to the control handshake. | `wrong_psk_is_rejected_and_counted`, `data_stream_token_and_limits` |
| Guessing / brute force on the port | 3 failed handshakes destroy the PSK; QR lives 120 s; 16-byte PSK. | `fourth_attempt_after_three_failures_is_locked_out`, `expired_code_is_refused_by_both_sides` |
| Recorded session replayed | Responder ephemeral key differs each time, so a replayed msg1 cannot yield a working session; replayed records fail the nonce check. | `replayed_data_stream_handshake_is_rejected`, `replayed_record_fails` |
| Tampering, reordering, dropping, truncation in transit | Per-record AEAD with sequential nonces; chunk hash; manifest hash; files finalized only after verification. | `tampered/reordered/dropped/truncated_*`, `truncated_transfer_never_finalizes` |
| Hijacking a session by "resuming" its control stream | Resume needs the PSK handshake **and** the data token derived from the original control handshake hash. | `control_resume_requires_session_token` |
| QR shoulder-surfing | Single use, 120 s, 6-digit code on both screens; UI (M2/M3) blurs the QR in the background. Someone who sees the QR *and* connects first within 120 s wins the race — the code mismatch is what exposes it. | `qr_is_single_use`, `verification_codes_match` |
| Malicious sender: path traversal, reserved names | Names reduced to one component, control characters and `<>:"|?*` replaced, reserved Windows names prefixed, length capped, never overwrite. | `sanitize::tests::*`, `hostile_names_are_sanitized_and_collisions_renamed` |
| Malicious sender: giant or inconsistent offers | Limits on total size, file count, name length, chunk count; free-space check before Accept; every frame checked against the accepted offer. | `offer_limits`, `receiver_enforces_*`, `oversized_chunk_rejected`, `chunk_index_out_of_range_rejected`, `unknown_file_id_rejected` |
| Malformed control messages | 1 MiB cap before allocation; CBOR decode never panics. | `decode_never_panics` (20,000 cases), `oversized_control_frame_rejected_before_allocation` |
| Secrets in logs or debug output | `Debug` impls omit PSK, keys, tokens, Wi-Fi password; error texts carry codes only. | `debug_hides_secrets` |
| Malicious app on the same device | Out of the core's control. Received files go to the user's chosen folder; Android will use MediaStore/SAF (M3). | — |

Out of scope: a compromised device, OS or Wi-Fi driver; an attacker who sees the QR and is on
the link within its 120 s window *and* whose code mismatch the user ignores; denial of service
by a device on the same network (it can lock a session with 3 bad attempts).
