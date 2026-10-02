# Fliq: Build Specification

Offline, end-to-end encrypted file transfer between a Windows desktop app (.exe) and an Android app. Files up to 50 GB. QR-code pairing. Dark, minimal "Fliq" design system (Section 10). Wi-Fi link setup with no internet. Target speed: 100 MB/s when the hardware allows it.

> **How to use this document:** give the whole file to Claude Code as the project brief (save it as `SPEC.md` in the repo root, and tell Claude to read it first and obey Section 1). Work milestone by milestone (Section 12). Do not skip ahead.

---

## 1. Rules for the AI building this (read first)

1. **Plan before coding.** First reply must be a short written plan: repo layout, crate/package choices, risks. Wait for the owner's go-ahead on anything that deviates from this spec.
2. **Work in vertical slices** (Section 12). After each milestone, run the tests, print the results, and stop with a summary. Never start the next milestone until the exit criteria are met.
3. **Never claim something works without evidence.** Evidence means: test output, benchmark numbers, or a manual checklist item the owner confirmed. You cannot test on a physical phone or a real Wi-Fi card from your sandbox. For anything that needs hardware (hotspot, Wi-Fi join, camera scan, USB), write the code, then add it to `docs/MANUAL_TESTS.md` marked **"needs owner test"**, and say so plainly in your summary.
4. **No custom cryptography.** Use only the vetted libraries named in Section 6. Do not invent handshakes, ciphers or key derivations beyond what this spec describes.
5. **Never put file bytes through Dart or Java/Kotlin.** All file I/O, crypto and networking for the data path live in Rust (Section 7).
6. **Measure, don't guess.** Every performance claim must come from the benchmark tool (Section 13). Report MB/s with the device pair, link type and file size.
7. **Fail loudly and safely.** Every error maps to a user-readable message and a safe state (partial files deleted, keys wiped). See Section 14.
8. **Keep it simple.** If a feature is marked v2, do not build it. Ask before adding anything not in this spec.
9. **Document as you go:** `README.md`, `docs/ARCHITECTURE.md`, `docs/PROTOCOL.md`, `docs/THREAT_MODEL.md`, `docs/MANUAL_TESTS.md`, and a `DEPENDENCIES.md` listing every third-party package and why.
10. **The UI follows Section 10 exactly.** Use only the listed color tokens, radii and type scale. No hard-coded colors, no gradients, no shadows, no extra accent colors, no mascot or decorative animation. If the design cannot express something (for example an error state), ask the owner instead of adding a color.

---

## 2. Product definition

### 2.1 What it does
Two devices transfer files directly to each other. One device shows a QR code. The other scans it. The scan carries everything needed to find the first device and encrypt the session. The transfer starts as soon as the receiver accepts the file list. No internet, no accounts, no cloud, no telemetry.

### 2.2 Supported pairs

| Sender | Receiver | MVP |
|---|---|---|
| Windows | Android | Yes |
| Android | Windows | Yes |
| Android | Android | Yes |
| Windows | Windows | Same-network + manual entry only |
| iOS | any | v2 (not in this spec) |

### 2.3 Speed: the honest version
- **100 MB/s = about 800 Mbps of real payload.** The app can only be as fast as the weakest of: the wireless link, the phone's storage write speed, and the CPU. Software cannot beat the link.
- Reaching 100 MB/s wirelessly requires both devices on **Wi-Fi 6 or better, 2x2, 5/6 GHz, 80 to 160 MHz channel**, and fast phone storage (UFS). Older or cheaper hardware will be slower. 2.4 GHz is about 3 to 10 MB/s.
- The app's job: **never be the bottleneck.** Target at least 90% of the raw `iperf3` throughput measured on the same device pair and link, and always show the user the real speed and which part is limiting.
- A wired USB mode is the only way to guarantee 100+ MB/s. It is **v2** and out of scope here, but keep the transport layer abstract so it can be added (Section 7.6).

### 2.4 Non-goals (v1)
Cloud sync, accounts, internet transfer, folders, iOS, USB mode, Wi-Fi Direct on Android, desktop camera scanning, background auto-receive.

---

## 3. Architecture overview

```
 ┌────────────── Flutter UI (Dart) ──────────────┐   screens, animations, QR display/scan
 │  state: transfer progress, link status, errors │
 └───────────────┬───────────────────────────────┘
                 │ flutter_rust_bridge (control calls + progress streams ONLY)
 ┌───────────────▼───────────────────────────────┐
 │               Rust core  (fliq_core)          │
 │  session   handshake   protocol   scheduler    │
 │  crypto    chunk I/O   net (TCP)   discovery   │
 └───────┬───────────────────────────┬───────────┘
         │ platform traits           │ platform traits
 ┌───────▼─────────┐         ┌───────▼───────────┐
 │ Android (Kotlin) │         │ Windows (Rust+WinRT)│
 │ hotspot, join,   │         │ Wi-Fi Direct AP,    │
 │ SAF fd, service  │         │ hotspot, firewall   │
 └──────────────────┘         └─────────────────────┘
```

- **Stack:** Flutter (UI) + Rust core via `flutter_rust_bridge` v2. Kotlin for Android platform glue. Rust `windows` crate for Windows platform glue.
- The Rust core is a library with a clean API plus a **CLI** (`fliq_cli`) used for benchmarks and tests. The CLI is built in Milestone 1 and is the main tool for verifying speed and security.

---

## 4. Connection model (QR and Wi-Fi)

### 4.1 Roles
Two independent concepts, do not mix them up:

- **Server / client** = who listens and who connects. **The device that shows the QR is always the TCP server. The device that scans is always the client.**
- **Sender / receiver** = who has the files. This is negotiated after connecting and is independent of server/client.

This gives two flows, and it means a PC never needs a camera:

| Flow | Who shows QR (server) | Who scans (client) | Who sends files |
|---|---|---|---|
| **A: Send** | Sender | Receiver | Server pushes to client |
| **B: Receive** | Receiver | Sender | Client pushes to server |

Desktop to phone: desktop shows the QR in either flow, and the phone scans. Phone to desktop: user taps Send on the phone, but the **desktop** is put in Receive mode and shows the QR, then the phone scans and sends. Phone to phone: either one shows the QR.

### 4.2 Link methods (how the two devices end up on one network)
The user picks a method on the server's screen. Default is auto.

| Method | What happens | MVP |
|---|---|---|
| **1. Same network** | Both devices are already on the same router, hotspot or Wi-Fi (internet not required). QR carries the server's IP addresses. | Yes |
| **2. Create offline network** | The server device creates its own Wi-Fi network. QR also carries SSID and password. The scanning phone joins automatically, then connects. | Yes |
| **3. Manual** | Screen shows IP + port + 6-digit code. User types it on the other device. Works whenever a route exists. | Yes (always-available fallback) |
| **4. mDNS discovery** | Server advertises `_fliq._tcp`; client lists nearby servers. | Optional in v1 |
| **5. USB tethering** | Detect a USB network adapter and use its IP. | v2 |

**Method 2, per platform (server side creates the network):**

- **Android server:** `WifiManager.startLocalOnlyHotspot()`. Read SSID and passphrase from the reservation (`SoftApConfiguration` on API 30+, `WifiConfiguration` on API 29). The app cannot choose the band; the OS decides. Needs `NEARBY_WIFI_DEVICES` (API 33+) or location permission (API 29-32), and Location services on for some versions. Some phones cannot run the hotspot while connected to Wi-Fi; detect failure and tell the user.
- **Windows server:** try in this order, falling through on failure:
  1. **Wi-Fi Direct in legacy access-point mode** (`Windows.Devices.WiFiDirect`, `WiFiDirectAdvertisementPublisher` with `LegacySettings.IsEnabled = true`, set `Ssid` and `Passphrase`). Appears to the phone as a normal Wi-Fi network and does not need an internet connection. Depends on the Wi-Fi driver, so test on several laptops.
  2. **Mobile Hotspot** (`NetworkOperatorTetheringManager`). Note: it often refuses to start when the PC has no internet connection profile, so this is the fallback, not the primary.
  3. If both fail: show a clear message, offer "Same network" or "Manual" instead.
- Request 5 GHz and the widest channel the driver allows. Warn the user if the PC's Wi-Fi card is also connected to a router on the same radio (speed will drop, and some cards cannot do both).

**Client side (scanner) joining the network:**
- **Android client:** `WifiNetworkSpecifier` + `ConnectivityManager.requestNetwork()`. The OS shows a one-time approval dialog (tell the user to tap Connect). Then **bind the process to that network** (`bindProcessToNetwork`) so traffic goes over it and not mobile data. The network will have no internet; that is expected, so handle the "no internet" notice without disconnecting. Unbind and release when done.
- **Windows client:** MVP only supports Same network and Manual. Joining a phone's hotspot from Windows is v2 (needs desktop QR scanning or pasted pairing text).

### 4.3 Connecting (client algorithm)
1. Parse and validate the QR (Section 5). Reject if expired, wrong version, or malformed.
2. If `wifi` is present and the device is not already on that network, join it (4.2). Show "Joining network…". Timeout 20 s.
3. Try **all** IP addresses from the QR **in parallel** (happy-eyeballs style), 3 s timeout each, keep the first success, drop the rest. Prefer addresses on the same subnet as the client.
4. Perform the handshake (Section 6). If it fails, show the failure reason (Section 14) and offer Manual entry.

### 4.4 Server address selection
Enumerate all local IPv4 interfaces. Include every usable address in the QR. Order: the created hotspot/AP interface first, then private LAN addresses (192.168/16, 10/8, 172.16/12). Deprioritize link-local (169.254/16) and known virtual adapters (VMware, VirtualBox, Hyper-V, WSL, Docker, VPN tunnels). Re-enumerate if the network changes while the QR is on screen and refresh the QR.

### 4.5 Port
Bind a random free port in 49152-65535. All connections for a session use that one port.

---

## 5. QR payload

### 5.1 Format
A URI so third-party scanners can open the app: `fliq://v1?d=<base64url(CBOR)>`. Keep the encoded payload under ~700 bytes so the QR stays easy to scan (error correction level M, large quiet zone, minimum 280 px on screen).

### 5.2 CBOR fields

| Key | Type | Meaning |
|---|---|---|
| `v` | uint | Protocol version (1) |
| `sid` | bytes(16) | Random session ID |
| `pk` | bytes(32) | Server's static X25519 public key (generated per session) |
| `psk` | bytes(16) | One-time secret (CSPRNG) |
| `ip` | array of strings | Server IPv4 addresses, in priority order |
| `port` | uint | TCP port |
| `wifi` | map, optional | `{ssid, pass, band_hint}` if the server created the network |
| `mode` | text | `"send"` or `"receive"` (what the server will do) |
| `exp` | uint | Expiry, unix seconds (now + 120) |
| `name` | text | Server device display name (for the accept dialog) |

### 5.3 Rules
- File names and sizes are **not** in the QR. They are sent over the encrypted channel.
- The QR is single-use and expires in 120 s. After a successful handshake, or after 3 failed handshakes, or on expiry, the server destroys `psk` and the session key material.
- The QR contains a secret. Show a visible warning "Only show this to the person you are sending to" and hide it (blur) when the app goes to the background.
- When the user needs a non-app Wi-Fi QR (for another person's stock camera app), optionally render a second standard Wi-Fi QR `WIFI:T:WPA;S:<ssid>;P:<pass>;;`. Optional in v1.

---

## 6. Protocol and security

### 6.1 Libraries (use these, nothing else for crypto)
- Handshake and record encryption: Rust `snow` (Noise). Pattern **`Noise_NKpsk0_25519_ChaChaPoly_BLAKE2s`**.
- Optional faster cipher: if benchmarks show AES-256-GCM with hardware acceleration beats ChaCha on the target device, use the `AESGCM` variant via snow's `ring` resolver. Decide by benchmark in Milestone 4, and negotiate in the first control message (both sides must support it).
- Hashing: `blake3`. Key derivation: `hkdf` with SHA-256. Randomness: OS CSPRNG (`getrandom`). Zeroization: `zeroize`.
- No other crypto crates without owner approval.

### 6.2 Handshake
- Initiator = client (the scanner). Responder = server (shows QR). The server's static public key `pk` and the one-time `psk` come from the QR, so the client is authenticated by knowing the PSK and the server by its key. This blocks anyone who did not scan the QR.
- Noise **prologue** = `"Fliq/1" || sid`.
- Ephemeral keys on both sides, so past sessions stay safe even if keys leak later (forward secrecy).
- After the control handshake, both sides compute a **6-digit verification code** from the handshake hash (take `HKDF(handshake_hash, "verify")`, first 4 bytes as u32, mod 1,000,000, zero-padded). Both screens show it. The receiver's accept dialog says "Make sure this code matches the other device".

### 6.3 Streams
- **Stream 0 = control** (one TCP connection). **Streams 1..N = data** (up to 16 more TCP connections). Each connection runs its own Noise handshake with the same `pk`/`psk`.
- The first encrypted message on a data connection must contain a token derived from the control handshake hash (`HKDF(control_hash, "data-token")`) plus its stream index. The server rejects any data connection without a valid token, or beyond the stream limit.
- Record layer: plaintext frames are split into Noise transport messages of at most 65,519 bytes plaintext, each prefixed on the wire with a 2-byte length. Noise handles nonces and authentication per message, which also detects reorder, drop and truncation within a stream.

### 6.4 Messages (CBOR, on the control stream)
`Hello{version, ciphers, device_name, os}` → `Role{sender|receiver}` → `Offer{files:[{id,name,size,mtime,chunk_count,manifest_hash?}]}` → `Accept{files, resume_bitmaps?}` or `Reject{reason}` → `StreamsReady` → `Progress`/`Ack` (periodic bitmaps from the receiver) → `FileDone{id, manifest_hash}` → `Verified{id}` (receiver to sender, sent only after the manifest hash matched and the file was finalized) → `Complete` → `Close`. Errors: `Error{code, text}`. Every message type has a size cap; reject oversized or unknown messages.

### 6.5 Data frames (on data streams)
`Chunk{file_id: u32, chunk_index: u32, chunk_hash: [u8;32], len: u32, payload}`
- Fixed chunk size **4 MiB** (last chunk may be smaller). Receiver verifies `chunk_hash` (BLAKE3) after decryption.
- **Integrity of the whole file:** the manifest hash for a file is `BLAKE3(chunk_hash_0 || chunk_hash_1 || …)`. The sender sends it in `FileDone`, the receiver recomputes it from its own verified chunk hashes. Mismatch → delete the file and report an error. (This avoids needing in-order whole-file hashing while chunks arrive out of order.)
- Optional setting: "Verify file on disk after transfer" re-reads the written file and rehashes each chunk (off by default).

### 6.6 Receiver safety
- Sanitize names: strip path separators and `..`, remove control characters, block Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9), limit length (255), no leading/trailing dots or spaces.
- Never overwrite: rename collisions to `name (1).ext`.
- Check free disk space before Accept; refuse and explain if insufficient.
- Enforce limits: max total size (default 50 GiB, configurable; protocol sizes are 64-bit, and the receiver must check free space and warn about FAT32 before accepting), max files per offer (default 1,000), max file name length.
- Write to a temporary `.part` target, finalize (rename or MediaStore publish) only after verification passes. Delete on failure or cancel.
- Reject any data that does not match the accepted offer (unknown file id, chunk index out of range, size mismatch).
- **Copy vs Move:** Copy leaves the source untouched. Move deletes the source file **only after** the sender receives `Verified{id}` for that file, never earlier, and never if the transfer was cancelled or failed. On Android 11+ deleting requires user consent through the system dialog (`MediaStore.createDeleteRequest` or SAF delete); on Windows send the source to the Recycle Bin. Show Move clearly in the confirm screen ("The original will be removed after the file is verified").

### 6.7 Memory and logging
- Zeroize keys and PSK at session end. Do not log keys, PSKs, QR payloads, file names or file contents. Diagnostic logs contain only error codes and counters. The user-visible **Transfer Logs** (file names, sizes, speeds, status) are a separate local feature: stored only on the device, clearable from Settings, never sent anywhere, and never mixed into diagnostic logs.
- No network traffic of any kind except the local transfer. No analytics SDKs. Release builds must not request any permission the app does not use.

### 6.8 Threat model (document in `docs/THREAT_MODEL.md`)
Covers: eavesdropper on the Wi-Fi link, a rogue device that connects to the port without scanning, QR shoulder-surfing, malicious sender (path traversal, giant files, decompression-style tricks), replay of recorded sessions, and a malicious local app on the same device. State clearly what is out of scope (a compromised device, someone who both sees the QR and is on the link within its 120 s window, which the verification code is meant to expose).

---

## 7. Transfer engine and performance

### 7.1 Pipeline
Sender: `[disk reader] → [chunk hash + encrypt workers] → [N TCP streams]`.
Receiver: `[N TCP streams] → [decrypt + verify workers] → [disk writer (pwrite at offset)]`.
- Bounded queues between stages (backpressure, fixed memory use). Peak memory target: under 200 MB total regardless of file size. The pipeline's in-flight buffer budget is one configurable constant (`INFLIGHT_BUDGET_BYTES`, default 64 MiB, minimum 16 MiB); benchmark whether a smaller budget costs throughput, and display the configured value in the UI (10.9). **Never load a whole file into memory.**
- A scheduler hands chunk numbers to whichever stream is free, so a slow stream does not stall the rest.

### 7.2 Streams
- Start with 4 data streams; adapt between 2 and 16 by measured throughput (add a stream while throughput scales, stop when it does not).
- TCP tuning: `TCP_NODELAY`, send/receive buffers of 4 MiB (accept the OS clamp), keepalive on.
- **Do not use QUIC/UDP for the bulk path** (userspace QUIC is CPU-bound on phones).

### 7.3 Disk I/O
- Receiver pre-allocates the target size if the OS allows it (ignore failure on content-provider file descriptors), writes each chunk at `chunk_index * 4 MiB` using positional writes, so chunks can arrive out of order.
- Use large aligned writes. Flush and fsync once at the end, not per chunk.
- Windows: sequential large writes; document that Defender real-time scanning can slow writes (offer a doc note, never disable it).
- Warn before accepting a file over 4 GiB if the destination is FAT32.

### 7.4 Android file access (critical for speed)
- Sender: `ContentResolver.openFileDescriptor(uri, "r")` → `ParcelFileDescriptor.detachFd()` → pass the integer fd to Rust, which wraps it with `File::from_raw_fd` and takes ownership. Do the same for the receiver using `MediaStore.Downloads` (insert with `IS_PENDING=1`, open `"w"` descriptor, set `IS_PENDING=0` on completion) under a `Fliq/` folder.
- Support only seekable local files for the sender. If a picked URI is not seekable or is a remote provider (for example a cloud drive stub), show "Download this file to your phone first".
- No bytes cross the Dart or JNI boundary except control messages and progress numbers.

### 7.5 Optional compression
Off by default in v1. If added later: fast `zstd`, per chunk, only for compressible types, decided by sampling; never for media/archives. Not needed for the MVP.

### 7.6 Transport abstraction
Define a Rust trait `Link` (listen/connect, local addresses, bind-to-network hints) so USB tethering or Wi-Fi Direct can be added without touching the protocol or engine.

### 7.7 Resume
- **MVP:** if a connection drops mid-transfer, the client re-dials within the same session (session stays alive 60 s after the last connection drops) and the receiver's chunk bitmap tells the sender what is missing.
- **v1.1:** cross-session resume keyed by `(name, size, mtime)`, using a saved `.part` file and bitmap sidecar; requires a fresh QR.

---

## 8. Platform specifics

### 8.1 Android
- **minSdk 29** (needed for `WifiNetworkSpecifier`), target the current stable SDK.
- Manifest permissions: `INTERNET`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_STATE`, `ACCESS_NETWORK_STATE`, `CHANGE_NETWORK_STATE`, `NEARBY_WIFI_DEVICES` (with `neverForLocation`, API 33+), `ACCESS_FINE_LOCATION` (`maxSdkVersion=32`), `CAMERA`, `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_DATA_SYNC`, `POST_NOTIFICATIONS`, `WAKE_LOCK`, `CHANGE_WIFI_MULTICAST_STATE` (only if mDNS is built). Request permissions just-in-time with a plain-language reason. Check current requirements for the chosen target SDK, including any local-network permission rules, before release.
- **Foreground service** (type `dataSync`) for the whole transfer with a progress notification and a Cancel action. Hold a partial wake lock and a Wi-Fi lock (use `WIFI_MODE_FULL_LOW_LATENCY` where available, else high-perf). Note in docs that the best speed is with the screen on, and offer a "Keep screen on during transfer" option (default on).
- Hotspot/join code lives in a Kotlin class exposed to Dart by a `MethodChannel`. Rust never calls Android APIs directly; Kotlin gives Rust the fd, the bound network state and the IP list through the bridge.
- Handle process death and screen rotation: transfer state lives in the Rust core and the service, not the Activity.
- QR scanning with `mobile_scanner`. If the camera is denied, offer Manual entry.

### 8.2 Windows
- Flutter Windows desktop app, single installer (Inno Setup or MSIX) that:
  - installs the app,
  - adds an inbound **Windows Firewall rule** for the app executable on **Private and Public** profiles (hotspot networks are often classified Public), running with admin rights in the installer,
  - registers an uninstaller that removes the rule.
- Code-sign the installer and exe. State in the README that unsigned builds will trigger SmartScreen warnings.
- On start of a server session, run a **self-connect test** to the chosen port on each advertised IP. If it fails, show a "Firewall may be blocking Fliq" dialog with a one-click "Fix" (re-adds the rule via an elevated `netsh advfirewall` call).
- WinRT calls (Wi-Fi Direct, tethering) run from Rust via the `windows` crate behind a trait so they can be mocked.
- Detect Wi-Fi capability and tell the user, in plain words, what link speed to expect (adapter standard, channel width if the API exposes it).
- Default save folder: `Downloads\Fliq`. Windows long path support enabled in the manifest.

---

## 9. Feature list

### MVP (Milestones 1-4)
1. Pick one or many files (any type), up to 50 GiB total per transfer, with a **Copy or Move** choice (6.6).
2. Flow A and Flow B with QR pairing; verification code; accept/reject with file list and sizes.
3. Same-network, Create-offline-network (Android server and Windows server), and Manual connection.
4. End-to-end encryption and integrity (Section 6).
5. Parallel streams, adaptive tuning, live MB/s, ETA, percent, "what is the bottleneck" hint.
6. Cancel at any time on either side, with clean-up.
7. In-session reconnect and resume.
8. Windows installer with firewall rule and self-check; Android foreground service.
9. Pre-transfer **link test** (3-second throughput probe on the encrypted connection) with a plain-language result ("Great: about 80 MB/s", "Slow link: try 5 GHz or a cable") before the user confirms.
10. Transfer Logs and Recent Transfers (local only, clearable), settings (device name, save folder, max size, keep screen on).

### v1.1 (after MVP is solid)
Cross-session resume; mDNS discovery; second standard Wi-Fi QR; "verify on disk" option; more UI polish.

### v2 (do not build now)
USB tethering mode, Wi-Fi Direct on Android, Windows scanning a QR (webcam) or pasted pairing text, Windows joining a phone's hotspot, trusted devices, folders, compression, iOS.

---

## 10. UI/UX: Fliq design system

This section replaces any earlier visual direction. The app is **Fliq** (not Fliqay). The look is a premium, calm, minimal **dark** interface in the style of high-end fintech/productivity software. It is **not** a gaming, neon, cyberpunk or claymorphism UI. No mascot, no animated parcels, no gradients, no glow, no heavy shadows, no glassmorphism.

The owner supplies the Fliq logo in `app/assets/brand/`. Use it as given; do not redraw or rename it, and do not add a glow around it.

### 10.1 Color tokens (the only colors allowed)

Define these once in `app/lib/theme/tokens.dart`. No widget may contain a hard-coded color.

| Token | Hex | Role |
|---|---|---|
| `onyxCanvas` | `#171721` | App background (desktop and mobile), bottom nav background |
| `graphiteCard` | `#1e1e2a` | Cards, desktop sidebar, info panels |
| `obsidianButton` | `#272735` | Secondary buttons, inputs, active nav pill, progress track, list dividers |
| `slateBorder` | `#70707d` | Resting borders, dividers, disabled elements (**never for text**) |
| `mistBorder` | `#e2e3ed` | Strong/light border where a clear outline is needed (for example error and focus-adjacent outlines) |
| `ashText` | `#c3c3cc` | Secondary text, descriptions, labels |
| `ivoryText` | `#ededf3` | Primary text, icons |
| `cobalt` | `#5266eb` | The single accent: primary action, selected state, progress fill |
| `white` | `#ffffff` | Only: text on cobalt buttons, and the QR code's background panel |

Rules:
- No green, red, orange, cyan, pink, purple, rainbow or gradient colors anywhere. State is shown by **icon + text + outline weight**, never by color alone (see 10.8).
- Use cobalt sparingly: one primary action per screen, the selected nav item, the progress fill, and focus rings.
- **Contrast limits (checked):** ivory on onyx and ash on graphite pass easily. White on cobalt is about 4.7:1 (passes AA for normal text). **Cobalt as small text on onyx/graphite is only about 3.8:1 and fails AA, so never set small text in cobalt**; use cobalt for fills, icons 24px or larger, indicators and focus rings only. `slateBorder` is not allowed for text.

### 10.2 Surfaces and shape
- Hierarchy by value contrast only: canvas `#171721` → card `#1e1e2a` → control `#272735`.
- Radii: **12 px** for cards and panels; **32 px** for buttons and inputs; **40 px** for nav pills and tags.
- No drop shadows, no glow, no blur. Borders only where needed (inputs, secondary buttons, error outlines), 1 px.
- Material 3 may be used as a base, but disable its tonal elevation tint (`surfaceTint` transparent) and set splash/ripple colors from tokens so no stray purple/blue overlays appear.
- Dark theme only in v1. Do not build a light theme.

### 10.3 Typography
- UI/body: **Inter** (open license). Display/headings: the owner may supply a licensed wide display sans (such as Söhne Breit); if it is not provided, use Inter with medium weight and slightly tight tracking. Bundle fonts as assets (no network font loading, the app must work offline). Check every font license before shipping.
- Weights: 400 for body, **500 for headings and buttons**. Do not use 700-900.
- Numeric metrics (speeds, sizes, percentages) use tabular figures so values do not jitter while updating.

| Style | Size | Notes |
|---|---|---|
| Caption | 12 px | Labels, metadata |
| Secondary | 14 px | Descriptions, list meta |
| Body | 16 px | Default, line-height 1.5 |
| Large body | 18 px | Subtitles |
| Subheading | 21 px | |
| Small heading | 28 px | Card titles, metric values |
| Heading | 32 px | Screen titles on mobile and secondary desktop headings |
| Large heading | 42 px | Desktop page title |
| Display | up to 65 px | Reserved; not needed in v1 screens |

Headings use line-height 1.1-1.2. Primary text is ivory, descriptions are ash.

### 10.4 Spacing and layout
- 4 px base. Allowed steps: 8, 12, 16, 24, 32, 40, 56, 72.
- Desktop: 32 px card padding, about 72 px between major sections, content max width about 1120 px, minimum window size 960×640.
- Mobile: 16-24 px page padding, 16-24 px inside cards, touch targets at least 48 dp. The mobile layout is designed for touch; it is not a shrunken desktop layout.

### 10.5 Components

| Component | Spec |
|---|---|
| Primary button | `cobalt` fill, `white` text, 500 weight, 32 px radius, flat, no shadow or glow. At most one per screen area. |
| Secondary button | `obsidianButton` fill (or transparent with a 1 px `slateBorder`), `ivoryText`, 32 px radius. |
| Text button | `ivoryText`, no fill; used for low-emphasis actions like "Can't scan? Enter code". |
| Card | `graphiteCard`, 12 px radius, generous padding, no shadow. |
| Input | `obsidianButton` fill, 32 px radius, 1 px `slateBorder`, ivory text. Focus: 2 px `cobalt` outline. |
| Icons | Simple line/glyph icons from one consistent set (for example Lucide, ISC license), 1.5 px stroke, 20-24 px, `ivoryText` or `ashText`. No oversized or cartoonish icons. |
| Progress bar | Pill shape, 8 px high, track `obsidianButton`, fill `cobalt`. Flat, no animation beyond value changes. |
| Metric tile | 12 px caption label in ash, value in 28 px ivory with tabular figures. |
| List row | 56-64 px high: file icon, filename (ivory, ellipsized in the middle so the extension stays visible), size and speed (ash, 14 px), status glyph + label. Divider `obsidianButton`. |
| Nav pill | 40 px radius; active = `obsidianButton` fill + ivory text + small cobalt indicator; inactive = ash. |
| QR panel | **`#ffffff` background**, QR modules in `onyxCanvas`, error correction M, quiet zone at least 4 modules, 12 px radius, at least 280 px (desktop) or 260 dp (mobile). Never invert or tint the QR; scanners need hard black-on-white contrast. |

### 10.6 Desktop (Windows) layout

- **Window:** `onyxCanvas` background. **Left sidebar** about 248 px wide in `graphiteCard` (a subtle tonal step, not heavy): Fliq logo and wordmark at the top, then **Dashboard, Send File, Receive File, Save Location, Transfer Logs**.
- **Dashboard (main content):**
  - Title **"High-Speed Offline Transfer"** (42 px, ivory) and a one-line subtitle (18 px, ash).
  - Two equal cards side by side: **Send Files** and **Receive Files**. Each has a large restrained line glyph (upload / download), a 28 px title, a 16 px description, and one button. **Send Files uses the cobalt primary button; Receive Files uses the secondary button.** Both cards are otherwise identical in style.
  - A metrics strip as one `graphiteCard` with four metric tiles (see 10.9 for the data).
  - A **Target Save Directory** card: folder icon, the full path (middle-ellipsized), free space, and a secondary "Change" button.
- **Send File flow screens** (inside the same shell): choose files with total size and the 50 GiB limit indicator and a **Copy / Move** selector → link method (Auto, Same network, Create offline network, Manual) → QR screen (QR panel, 2-minute countdown, verification code, network name if created, "Can't scan? Use manual code") → confirm → transfer → done.
- **Receive File flow:** shows the receive QR (Flow B) with the same QR layout, plus manual code entry → confirm → transfer → done.
- **Save Location:** path, free space, change button, Windows long-path note if relevant. **Transfer Logs:** full list of past transfers (search, clear).

### 10.7 Android layout

- **Header:** Fliq logo and wordmark. No glow.
- **Transfer (home) screen**, top to bottom:
  1. Storage card: small folder icon, the save path (ellipsized), free space.
  2. **SEND** and **RECEIVE** as two balanced equal-height cards in a 2-column grid. SEND: "Copy or Move up to 50 GB" with the cobalt primary treatment. RECEIVE: "Scan QR & Auto-Join" with the secondary treatment. Simple ivory upload/download glyphs.
  3. **Direct Link Tier** card (compact, technical, visually secondary): the active tier in 14 px ivory, with a 12 px ash line for crypto and I/O details (see 10.9).
  4. **Recent transfers**: clean list rows (10.5). "See all" opens Transfer Logs.
- **Bottom navigation:** **Transfer, Send, Receive, Storage**. Background `onyxCanvas` with a 1 px `obsidianButton` top divider so it merges with the canvas. Icons ash; the selected item uses a cobalt icon (24 dp, so the 3:1 graphics contrast applies) with an ivory label. Minimum 48 dp targets.
- Send, Receive (camera scan screen with a plain frame and a manual-entry text button) and the connecting/confirm/transfer/done screens follow the same tokens. **Storage** holds save location, free space and settings. The transfer screen shows progress, speed, ETA, per-file list and Cancel, and nothing decorative.

### 10.8 States without extra colors

| State | Treatment |
|---|---|
| In progress | Cobalt progress fill, ivory "Sending…" label, live MB/s |
| Success / verified | Check glyph (ivory) + "Verified" (ash/ivory), no color change |
| Warning (slow link, low space) | Info/alert glyph (ivory) + plain sentence, `slateBorder` outline on the card |
| Error / failed | Alert glyph (ivory) + clear message + the code, **`mistBorder` 1 px outline** on the card, and a primary "Try again" action |
| Disabled | Ash text at reduced emphasis with `slateBorder` outline, plus it must not respond to taps |
| Selected | Cobalt indicator + ivory label |

Because the palette has no red/green, errors must be unmistakable through icon, wording and the brighter outline. Test every state with a grayscale screenshot: it must still be understandable.

### 10.9 Technical information to display (must be real, not hard-coded)

The sample numbers in older mockups are examples only. Every value comes from the app:

| Label | Source |
|---|---|
| **Measured Engine Ceiling** (MB/s) | Latest `fliq_cli bench` / in-app link test result on this device; show "Not measured yet" until a test has run |
| **Max Single Payload** | The configured size limit (default 50 GB) |
| **Crypto Integrity** | What is actually implemented: "Noise + BLAKE3" (update the string if the cipher or scheme changes) |
| **In-Flight RAM Bound** | The configured pipeline buffer budget (`INFLIGHT_BUDGET_BYTES`), shown as a fixed number |
| **Direct Link Tier** | The tier actually in use: `Same network`, `LocalOnlyHotspot` (with band if known), `Windows Wi-Fi Direct AP`, `Windows Mobile Hotspot`, or `USB Turbo Cable` (only once that v2 mode exists) |
| Crypto/I/O line | Actual cipher in use (`AES-256-GCM` or `ChaCha20-Poly1305`), and "Zero-copy · SAF fd passing" only on paths that really use it |
| Recent transfer row | Real filename, size, average MB/s, and verification status from the local log |

Never show a feature (for example USB Turbo Cable, or "Zero-copy") in the UI unless it is implemented and active on that transfer.

### 10.10 Motion
Minimal and functional: 150-200 ms ease-out fades or slides between screens, value changes on progress, no looping decorative animation. Respect the OS reduce-motion setting. The transfer screen must hold 60 fps and must not measurably reduce throughput. Haptics only on mobile for: connected, completed, failed.

### 10.11 Copy, accessibility and tests
- Keep the existing labels (High-Speed Offline Transfer, Send File, Receive File, Save Location, Transfer Logs, Direct Link Tier, Recent Transfers). Put all strings in one file so they can be translated.
- Plain, friendly wording; screen-reader labels on every control; keyboard navigation and visible focus on Windows; text scales with the OS font setting up to 200% without clipping.
- **Token lint:** a test fails the build if a hex color appears in any file other than `tokens.dart`.
- **Golden tests** for Dashboard (desktop), Transfer (mobile), QR, Confirm, Transfer-in-progress, Done and Error screens at the supported sizes.
- A contrast test asserts every text/background pair used meets 4.5:1 (3:1 for large text and graphics).

## 11. Repository layout and tooling

```
fliq/
  SPEC.md                      (this file)
  README.md  DEPENDENCIES.md
  docs/  ARCHITECTURE.md  PROTOCOL.md  THREAT_MODEL.md  MANUAL_TESTS.md  BENCHMARKS.md
  core/                        Rust workspace
    fliq_core/                session, handshake, protocol, crypto, engine, links
    fliq_cli/                 CLI: send/recv/bench for tests and benchmarks
    fliq_platform_windows/    WinRT: Wi-Fi Direct AP, tethering, firewall helpers
  app/                         Flutter app (Android + Windows)
    lib/ (ui, state, theme/tokens.dart, strings)
    assets/brand/ (owner-supplied Fliq logo)
    assets/fonts/ (bundled Inter, no network fonts)
    android/ (Kotlin: hotspot, join, SAF fd, foreground service)
    windows/ (runner, installer scripts)
  tools/  scripts for iperf3 comparison, test-file generation, CI
```

- Rust: stable toolchain, `cargo fmt`, `cargo clippy -D warnings`, `cargo deny` for dependency auditing.
- CI: build and test the Rust workspace on Windows and Linux; build the Flutter app; run lints.

---

## 12. Milestones and exit criteria

Stop after each milestone, show evidence, and wait for the owner.

**M0: Feasibility (owner does this; Claude writes the guide).** Produce `docs/FEASIBILITY.md` with exact steps and commands to run `iperf3` between the owner's phone and laptop over each link type (router Wi-Fi, Windows AP, USB tethering for reference), and a table to fill in. *Exit: owner reports their real link ceiling.*

**M1: Rust core and CLI.** Noise handshake from a QR-style payload (CLI prints/reads the URI instead of a QR image), control + data streams, chunk pipeline, parallel streams, BLAKE3 manifest verification, resume-within-session, receiver safety rules. *Exit:* a 5 GiB file transfers between two processes (localhost, then two machines if available); hashes match; tamper tests pass (modified, reordered, replayed, truncated, wrong PSK, wrong stream token, oversized frames, path-traversal file names all rejected); memory stays under 200 MB; localhost throughput is reported.

**M2: Windows app with QR flow.** Flutter Windows UI built with the final Section 10 tokens and components from the start, wrapping the core; QR display, verification code, accept dialog, progress; same-network and manual modes; installer with firewall rule and self-connect test. *Exit:* two Windows PCs transfer a 5 GiB file using the QR/manual flow; firewall self-check demonstrably catches a blocked port.

**M3: Android app.** Built with the same Section 10 tokens and components; camera scan, Flow A and Flow B with Windows, SAF/MediaStore fd passing, foreground service, locks, permissions, join-network from QR (`WifiNetworkSpecifier` + bind). *Exit (owner-tested on real hardware):* Windows↔Android both directions with a 5 GiB file on a shared network; screen-off and rotation do not break it.

**M4: Offline network creation and tuning.** Windows Wi-Fi Direct legacy AP (then tethering fallback), Android LocalOnlyHotspot, band/width requests, link test, adaptive streams, cipher benchmark. *Exit:* with no router and no internet, Windows↔Android and Android↔Android transfers work via the QR alone; `docs/BENCHMARKS.md` has a table of MB/s per device pair and link, each with the matching `iperf3` number and the percentage achieved.

**M5: Design-system review and polish.** Audit every screen against Section 10 (tokens, type scale, radii, spacing, states without extra colors), finish history/settings, reduce-motion, empty and error states. *Exit:* token lint and contrast tests pass, golden tests exist for the listed screens on desktop and mobile, grayscale screenshots of every state are understandable, the transfer screen holds 60 fps, and throughput is within 3% of the same transfer with all animation disabled.

**M6: Hardening and release.** Threat-model review, dependency audit, fuzzing the message parser, signed installer and APK, README with supported hardware, known limits, and troubleshooting. *Exit:* everything in Section 15 is true.

---

## 13. Testing and verification

### 13.1 Automated (Claude runs these)
- Unit tests: chunking math, bitmap logic, name sanitizer (including reserved names and traversal), QR encode/decode and expiry, verification-code derivation, limit enforcement.
- Crypto/protocol negative tests listed in M1, plus: replayed handshake, expired session, 4th failed handshake locks the session, 17th data stream refused.
- Property/fuzz tests on the CBOR message parser (`cargo fuzz` or `proptest`); it must never panic or allocate unboundedly.
- Integration: loopback transfers of 1 byte, 0 bytes (rejected or handled), exactly one chunk, one chunk plus 1 byte, many small files, one 5 GiB file; kill a connection mid-transfer and verify recovery; kill the process and verify `.part` clean-up.

### 13.2 Benchmark tool
`fliq_cli bench` runs a transfer of a generated file and prints: end-to-end MB/s, per-stage busy percentages (read / encrypt / network / decrypt / write), stream count over time, peak memory. `tools/` has a script that runs `iperf3` on the same pair and prints the ratio. The goal is at least 90% of iperf3. If a stage shows saturated CPU or disk, report it as the bottleneck rather than hiding it.

### 13.3 Manual tests (owner, on hardware; Claude maintains `docs/MANUAL_TESTS.md`)
Checklist, each with pass/fail and notes: Windows 10 and 11; at least 2 Wi-Fi chipsets (Intel and one other); 3 Android phones across versions (10, 13, 15) and storage types; same-router transfer; Windows AP offline transfer; Android hotspot offline transfer; Windows hotspot with and without internet; camera scan in bright and dim light; denied-permission paths; screen off; app switched away; Wi-Fi turned off mid-transfer; battery saver on; cable-less 5 GiB soak test with a hash check; firewall off and on; two transfers back-to-back.

---

## 14. Errors: what the user sees

| Situation | User message (plain language) | Action |
|---|---|---|
| QR expired | "This code expired. Ask the other device to show a new one." | Back to scan |
| Wrong/invalid QR | "That doesn't look like a Fliq code." | Rescan |
| Join network refused/timed out | "Couldn't join the other device's Wi-Fi. Tap Connect when your phone asks, or use manual entry." | Retry / Manual |
| No route / connect timeout | "Can't reach the other device. Make sure both are on the same Wi-Fi, or choose 'Create offline network'." | Retry / change method |
| Firewall blocked (Windows) | "Windows Firewall may be blocking Fliq." | One-click fix |
| Handshake failed / code mismatch | "Couldn't secure the connection. Start again." | Restart session |
| Not enough space | "Not enough space on this device (need X, have Y)." | Cancel |
| File too big / too many files | States the limit | Cancel |
| Connection dropped | "Connection lost. Reconnecting…" then resume, else clear failure | Auto retry 60 s |
| Hash mismatch | "The file arrived damaged and was deleted. Try again." | Delete partial |
| Slow link | "Your connection is slow (about X MB/s). A 5 GHz network or a cable will be faster." | Continue / cancel |
| Hotspot/AP failed to start | Explains the cause if known; offers Same network / Manual | Fallback |

All errors carry a short code (for logs and support) and never include file names or secrets.

---

## 15. Definition of done (v1)

1. All MVP features in Section 9 work on the manual test matrix, each marked by the owner.
2. A 5 GiB file transfers correctly (hash verified) in both directions between Windows and Android, on a shared network and on an offline Wi-Fi network created by either device, with no internet present. At least one 50 GiB transfer completes and verifies on one device pair (soak test).
3. The negative tests in 13.1 all pass; the fuzz run is clean for at least 10 minutes per parser target.
4. Measured throughput is at least 90% of `iperf3` on every tested pair, and the results are documented honestly, including pairs that fall short of 100 MB/s and why.
5. No file bytes pass through Dart/JNI; peak memory under 200 MB; no secrets in logs.
6. Signed Windows installer (with firewall rule) and signed Android release build are produced by the documented build steps.
7. `README.md` states supported hardware, the real-world speed expectations, known limitations (hotspot behavior varies by phone and Wi-Fi driver, no iOS, no USB mode yet), and troubleshooting.

---

## 16. Known risks to watch (report immediately if you hit them)

- Windows Wi-Fi Direct legacy AP not starting on some drivers → fall back as specified; record which hardware fails.
- Android LocalOnlyHotspot unavailable or unable to coexist with an active Wi-Fi connection on some phones → fall back; record the device.
- Android background and battery restrictions (OEM-specific) killing the service → document per-brand settings in the README.
- Phone storage or thermal throttling capping speed → report as the bottleneck in the UI and benchmarks.
- Windows Firewall profile quirks on hotspot networks → covered by the installer rule and the self-check.
- Anything in this spec that proves wrong or impossible on real hardware → stop and tell the owner; do not silently change the design.
