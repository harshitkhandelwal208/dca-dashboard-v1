#!/bin/bash
# Run every minute by dca-health.timer: restart the bot when /health has not reported a connected gateway for 5 checks in a row.
STATE=/run/dca-health.fails
if curl -fsS -m 10 http://127.0.0.1:3000/health 2>/dev/null | grep -q '"discordStatus":"connected"'; then
  rm -f "$STATE"
  exit 0
fi
n=$(( $(cat "$STATE" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$STATE"
if [ "$n" -ge 5 ]; then
  logger -t dca-health "bot unhealthy for $n checks, restarting"
  rm -f "$STATE"
  systemctl restart dca-bot
fi
