#!/bin/sh
printf 'signaling_addr = "http://101.43.41.195:8080"\nrelay_addr = "101.43.41.195:8081"\npassword = "REDACTED"\n' > /frpsh.toml
cat /frpsh.toml
pkill frp-sh 2>/dev/null
sleep 1
nohup /frp-sh --config /frpsh.toml lan join 5439 --relay --verbose > /join.log 2>&1 &
sleep 22
grep -E 'Joined|TURN relay link|trying relay|relay connected|heartbeat' /join.log | head -8
echo '--- recv count ---'
grep -c 'recv kind' /join.log
echo '--- ping 10.66.0.1 ---'
ping -c 3 -W 2 10.66.0.1 2>&1 | tail -2
