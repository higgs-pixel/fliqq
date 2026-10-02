# Compare Fliq against iperf3 on the same pair (Windows client side).
# Usage: .\iperf3_compare.ps1 -ServerIp 192.168.1.20 -Uri "fliq://v1?d=..." -OutDir C:\fliq-in
#   On the other machine first run:  iperf3 -s   and   fliq_cli serve-send <file>
param([string]$ServerIp, [string]$Uri, [string]$OutDir)
$j = iperf3 -c $ServerIp -t 15 -P 4 -R -J | ConvertFrom-Json
$iperf = $j.end.sum_received.bits_per_second / 8e6
$line = (fliq_cli connect $Uri --out $OutDir --yes 2>$null | Select-String "MB/s").Line
$fliq = [double]([regex]::Match($line, '= ([0-9.]+) MB/s').Groups[1].Value)
"iperf3 {0:N1} MB/s   fliq {1:N1} MB/s   ratio {2:N1} %  (target >= 90 %)" -f $iperf, $fliq, (100 * $fliq / $iperf)
