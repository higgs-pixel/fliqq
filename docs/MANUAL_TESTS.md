# Manual tests (owner, real hardware)

Mark each **pass / fail** with notes. Nothing here has been run by Claude: the sandbox has no
Wi-Fi, phone, camera or USB.

## M1 (CLI, two real machines) — needs owner test
| # | Test | Command | Result |
|---|---|---|---|
| 1 | Linux/Windows ↔ Windows on the same router, 1 GiB | `fliq_cli serve-send` / `fliq_cli connect` | |
| 2 | Same, 5 GiB, then compare hashes (`certutil -hashfile X SHA256`) | | |
| 3 | Both directions (serve-recv + connect FILE) | | |
| 4 | Pull the network cable / switch off Wi-Fi mid-transfer and leave it off: both sides show "Reconnecting…", then after 60 s "Connection lost", and no `.fliq.part` is left | | |
| 4b | Briefly unplug / switch Wi-Fi off for ~5 s mid-transfer, then reconnect: both sides show "Reconnecting…" and the file still arrives with a matching hash | | |
| 5 | Ctrl+C the receiver mid-transfer, start a new receive into the same folder: old `.fliq.part` is removed | | |
| 6 | Windows Firewall on: first run prompts; deny it and confirm the other side gets "Can't reach the other device" | | |
| 7 | Record MB/s next to iperf3 for the same pair in `docs/BENCHMARKS.md` | `tools/iperf3_compare.*` | |

## M2 (Windows app) — needs owner test
| # | Test | Result |
|---|---|---|
| 1 | Install `FliqSetup-*.exe`; `netsh advfirewall firewall show rule name=Fliq` lists the rule on Private and Public | |
| 2 | Dashboard: four metrics, save folder with free space, Change works | |
| 3 | Send: pick files, Copy/Move, Show code; QR, countdown and addresses appear; minimising hides the QR | |
| 4 | Second PC (CLI `fliq_cli connect "<text>"` until manual mode exists) receives; both show the same verification code | |
| 5 | Receive: accept dialog lists files and sizes; Decline leaves nothing behind | |
| 6 | 5 GiB transfer between two PCs; hash matches (`certutil -hashfile X SHA256`) | |
| 7 | Move: source goes to the Recycle Bin only after "Verified" | |
| 8 | Delete the firewall rule, start a session: warning + Fix appears; Fix shows UAC and the rule returns | |
| 9 | Cancel mid-transfer on either side: other side shows an error at once, no `.fliq.part` left | |
| 10 | Transfer Logs lists the transfers; search and Clear work | |
| 11 | Window cannot be made smaller than 960×640; Tab moves focus with a visible cobalt ring | |
| 12 | Windows display scale 200%: no clipped text | |

## M3–M6
The full matrix from SPEC 13.3 (Windows 10/11, two Wi-Fi chipsets, three Android phones,
hotspots, camera scan, permissions, screen off, battery saver, 5 GiB soak, firewall) is added
as each milestone lands.
