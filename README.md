# Fliq

Offline, end-to-end encrypted file transfer between Windows and Android. QR-code pairing,
files up to 50 GB, no internet, no accounts, no telemetry. The full brief is in `SPEC.md`.

## Status

| Milestone | State |
|---|---|
| M0 Feasibility (owner) | Guide written: `docs/FEASIBILITY.md`. Waiting for the owner's iperf3 numbers. |
| M1 Rust core + CLI | Core, CLI and 61 automated tests passing on Linux, including in-session reconnect of data and control streams. Waiting for owner review. |
| M2 Windows app | In progress. App-facing Rust API + bridge built and tested (67 Rust tests). Flutter UI, installer and CI job written but **not yet compiled** (no Flutter SDK in the build sandbox). Manual mode waits for an owner decision. |
| M3–M6 | Not started. |

There is **no tested Windows app yet**. The first CI run on GitHub compiles the Flutter app and
builds the installer; expect a round of compile fixes. `fliq_cli` remains the verified tool.

### M1 items still without full evidence
- **5 GiB between two processes.** The sandbox only had room for 3 GiB (source + copy);
  5 GiB was verified in a single process. See `docs/BENCHMARKS.md`.
- **Windows.** The code is written for Windows (positional I/O, FAT32 detection) but was
  only compiled on Linux. CI builds and tests it on `windows-latest`.
- Six spec decisions need the owner: see `docs/DECISIONS.md`.

### Recovery within a session (spec 7.7)
- A dropped **data** stream: its chunk is requeued, the client re-dials, the receiver asks
  for anything still missing.
- A dropped **control** stream: the client re-dials it with the session's data token; both
  sides repeat their state (FileDone / Verified / repair requests) and continue. The CLI
  shows "Connection lost. Reconnecting…".
- If nothing comes back within 60 s (configurable `EngineOptions.linger`), the transfer
  fails with `E_CONN_LOST` and partial files are deleted. A receiver that was killed leaves a
  `.fliq.part`; the next receive into that folder removes it.
- Cancel tells the other device first, so it fails immediately instead of waiting.

## Repository layout

```
SPEC.md  README.md  DEPENDENCIES.md
docs/    ARCHITECTURE  PROTOCOL  THREAT_MODEL  MANUAL_TESTS  BENCHMARKS  FEASIBILITY  DECISIONS
core/    Rust workspace
  fliq_core/   session, handshake, protocol, crypto record layer, engine, links
  fliq_cli/    send / receive / bench CLI
tools/   iperf3 comparison scripts
.github/workflows/ci.yml
```
`app/` holds the Flutter app (`lib/`), the bridge crate (`rust/`), the generated FRB plugin
glue (`rust_builder/`), the Inno Setup script (`windows_installer/`) and `tools/patch_windows_runner.py`.
The `windows/` runner folder is generated in CI by `flutter create` and then patched.

## Building the Windows app (needs Windows, Flutter stable >= 3.27, Rust, Inno Setup 6)

```
cd app
cargo install flutter_rust_bridge_codegen@2.11.1 cargo-expand
flutter create --platforms=windows --project-name fliq --org app.fliq .
del test\widget_test.dart
python tools\patch_windows_runner.py
flutter_rust_bridge_codegen generate
flutter test
flutter build windows --release
iscc windows_installer\fliq.iss          # -> app\build\installer\FliqSetup-0.2.0.exe
```
Or push to GitHub: the `app-windows` CI job does all of this and uploads the installer.

## Build and test

Requires stable Rust 1.85 or newer.

```
cd core
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo build --release -p fliq_cli        # target/release/fliq_cli(.exe)
```

## Getting a Windows `fliq_cli.exe` without installing Rust

Push this repository to GitHub. The `ci` workflow builds and tests on Windows and Linux and
attaches `fliq_cli.exe` as a download on the workflow run page (Actions → latest run →
Artifacts → `fliq_cli-windows-latest`). The binary is unsigned.

## Using the CLI

```
# Machine A shows a code (printed as a fliq:// URI) and sends:
fliq_cli serve-send big.iso
# Machine B uses that URI to receive:
fliq_cli connect "fliq://v1?d=..." --out ./incoming

# Or the other way round (B shows the code and receives, A sends):
fliq_cli serve-recv --out ./incoming
fliq_cli connect "fliq://v1?d=..." big.iso

# Benchmarks
fliq_cli bench --size 2G               # loopback, disk -> disk
fliq_cli bench --size 5G --synthetic   # generated source, real disk on the receiver
fliq_cli crypto-bench
```
Both sides print the same 6-digit verification code. The URI contains a one-time secret:
treat it like a password. It expires after 120 s and works once.

On Windows, allow `fliq_cli.exe` through Windows Firewall (Private and Public) when asked,
or the other device will not reach it. The M2 installer adds this rule automatically.

## Speed expectations

The app can only be as fast as the slowest of the Wi-Fi link, the phone's storage and the
CPU. 100 MB/s needs Wi-Fi 6, 5/6 GHz, 80–160 MHz on both devices plus fast phone storage.
2.4 GHz gives roughly 3–10 MB/s. Measure your own link first with `docs/FEASIBILITY.md`.

## Known limitations (current)
- CLI only; no GUI yet.
- No offline-network creation yet (M4); both machines must share a network.
- Unsigned binaries trigger SmartScreen on Windows.
