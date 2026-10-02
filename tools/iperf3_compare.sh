#!/usr/bin/env bash
# Compare Fliq against iperf3 on the same pair of machines (Linux/macOS client side).
# Usage: iperf3_compare.sh <server-ip> <fliq-uri> <out-dir>
#   On the other machine first run:  iperf3 -s   and   fliq_cli serve-send <file>
set -euo pipefail
ip="$1"; uri="$2"; out="$3"
iperf=$(iperf3 -c "$ip" -t 15 -P 4 -R -J | python3 -c 'import json,sys;print(json.load(sys.stdin)["end"]["sum_received"]["bits_per_second"]/8e6)')
fliq=$(fliq_cli connect "$uri" --out "$out" --yes 2>/dev/null | sed -n 's/.*= \([0-9.]*\) MB\/s.*/\1/p')
python3 - "$iperf" "$fliq" <<'PY'
import sys
i, f = map(float, sys.argv[1:])
print(f"iperf3 {i:.1f} MB/s   fliq {f:.1f} MB/s   ratio {100*f/i:.1f} %  (target >= 90 %)")
PY
