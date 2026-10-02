# M0: measure your real link ceiling (owner)

Goal: know the fastest any app could go between **your** phone and **your** laptop, per link
type. Fliq's target is ≥ 90 % of these numbers.

## 1. Install iperf3
- **Windows:** download the iperf3 Windows build (for example from iperf.fr), unzip, open a
  terminal in that folder. Allow it through Windows Firewall (Private **and** Public) when asked.
- **Android:** install an iperf3 app (for example "Magic iPerf" or "PingTools"), or Termux and
  `pkg install iperf3`.

## 2. Find the laptop's IP on the link you are testing
Windows: `ipconfig` → the IPv4 address of the Wi-Fi (or hotspot / USB) adapter.

## 3. Run (laptop is the server)
```
laptop> iperf3 -s
phone>  iperf3 -c <laptop-ip> -t 20 -P 4          # phone -> laptop
phone>  iperf3 -c <laptop-ip> -t 20 -P 4 -R       # laptop -> phone
```
Read the `[SUM] ... receiver` line. Mbits/sec ÷ 8 = MB/s.

## 4. Link types to test
| # | Setup | How |
|---|---|---|
| A | Both on your home router, 5 GHz | Connect both to the 5 GHz SSID. Note router model and Wi-Fi standard. |
| B | Laptop hotspot, no router | Windows Settings → Network → Mobile hotspot, band 5 GHz if offered; phone joins it. If it refuses without internet, write that down. |
| C | Phone hotspot | Phone hotspot on, laptop joins. Note the band shown on the phone. |
| D | USB tethering (reference only) | Phone USB cable, Settings → Hotspot → USB tethering; laptop gets an IP on a new adapter. |

Also note the phone's storage write speed if you can (for example with "A1 SD Bench" or
"AndroBench"): a phone that writes at 80 MB/s cannot receive faster than that.

## 5. Report back
| Link | Phone → laptop MB/s | Laptop → phone MB/s | Band / channel width | Devices |
|---|---|---|---|---|
| A | | | | |
| B | | | | |
| C | | | | |
| D | | | | |

M0 exit: this table filled in. Then run `tools/iperf3_compare` against `fliq_cli` on the same
pair to get the Fliq/iperf3 percentage.
