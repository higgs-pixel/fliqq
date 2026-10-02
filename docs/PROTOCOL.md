# Protocol v1 (as implemented)

## QR
`fliq://v1?d=<base64url-no-pad(CBOR map)>` with keys `v, sid(16), pk(32), psk(16), ip[],
port, wifi?{ssid,pass,band_hint?}, mode("send"|"receive"), exp, name`. A typical payload is
~200 characters. The client rejects a wrong version, wrong field lengths, invalid IPv4, more
than 16 IPs, or `now > exp`.

## Connections
Every TCP connection (control or data) runs:

```
-> 2-byte len | Noise msg1 (psk, e, es)        prologue = "Fliq/1" || sid
<- 2-byte len | Noise msg2 (e, ee)              psk32 = HKDF(sid, psk16, "Fliq/1 noise-psk")
```
then transport records: `2-byte len | ciphertext` (≤ 65,535 bytes, ≤ 65,519 plaintext).
Each direction uses its own nonce counter starting at 0, so any modified, dropped,
reordered or replayed record fails to decrypt.

The first plaintext byte selects the stream kind:
- `0x00` control, followed by a `Hello` control message. Accepted once per QR and only
  before `exp`.
- `0x02` control resume, followed by `token(32)` (the data token). Accepted any time
  after the first control handshake while the session is open; the server answers `1`/`0`
  and replaces its control stream with the new one. Used when the control stream drops.
- `0x01` data, followed by `token(32) || index(u16 BE)`. `token = HKDF(control_hash,
  "data-token")`. Valid indexes 1..=16, one live connection per index. The server answers
  one byte: `1` accepted, `0` refused.

A failed handshake, or a failure to produce the first encrypted message, counts as a failed
attempt. After 3, the PSK is destroyed.

Verification code: `HKDF-SHA256(ikm = control_hash, info = "verify")`, first 4 bytes as a
big-endian u32, mod 1,000,000, zero-padded to 6 digits.

## Control messages
`u32 BE length | CBOR` inside the record layer; length ≤ 1 MiB, checked before allocation.

```
client -> Hello{version, ciphers, device_name, os}
server -> Hello{...}, Role{server_role}
sender -> Offer{files:[{id,name,size,mtime,chunk_count}]}
receiver -> Accept{files} | Reject{reason}
client -> StreamsReady{count}           (only when the client is the sender; informational)
sender -> FileDone{id, manifest_hash}   (after every chunk of that file was sent once)
receiver -> Ack{id, bitmap}             (repair request: sender resends every unset chunk)
receiver -> Verified{id}
sender -> Complete
receiver -> Close
either -> Error{code, text}
```
Unknown or malformed messages and out-of-range fields are rejected.

After a control resume, the sender repeats `FileDone` for every file it has announced and the
receiver repeats `Verified` for every finalized file and sends `Ack` for unfinished ones.
Repeats are idempotent. If the control stream drops after every file is verified, both sides
finish successfully without waiting for `Complete`/`Close`.

## Data frames
`file_id u32 | chunk_index u32 | chunk_hash [32] (BLAKE3 of plaintext) | len u32 | payload`.
`file_id = 0xFFFFFFFF` marks the clean end of a data stream. The receiver rejects unknown
file ids, indexes ≥ `chunk_count`, and any `len` other than `min(4 MiB, size − index·4 MiB)`.

`manifest_hash = BLAKE3(chunk_hash_0 || … || chunk_hash_{n−1})`; a 0-byte file has the hash
of the empty string.

## Limits (defaults)
50 GiB per transfer, 1,000 files, 255-byte sanitized names, 16 data streams, 120 s QR
lifetime, 60 s with no progress before "Connection lost".
