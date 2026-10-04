#!/bin/bash
# Run every 2 minutes by dca-autoupdate.timer: install the newest build of the rolling release, and go back to the
# previous build if the new one does not come up healthy. Nothing connects to the server; it only looks at GitHub.
set -u
REPO="${DCA_REPO:-harshitkhandelwal208/dca-dashboard-v1}"
STATE=/var/lib/dca-autoupdate
PREV=/opt/dca/prev
mkdir -p "$STATE"
exec 9>/run/dca-autoupdate.lock
flock -n 9 || exit 0

LATEST="$(curl -fsS -m 20 "https://github.com/${REPO}/releases/download/rolling/VERSION" 2>/dev/null | awk '{print $1; exit}')"
[ -n "$LATEST" ] || exit 0
CURRENT="$(awk '{print $1; exit}' /opt/dca/VERSION 2>/dev/null || true)"
[ "$LATEST" = "$CURRENT" ] && exit 0
# A build that failed its health check is not tried again; the next commit gets a fresh chance.
[ "$LATEST" = "$(cat "$STATE/failed" 2>/dev/null || true)" ] && exit 0

logger -t dca-autoupdate "installing $LATEST (running ${CURRENT:-none})"
rm -rf "$PREV"; mkdir -p "$PREV"
cp -a /opt/dca/bin/dca-bot /opt/dca/VERSION "$PREV/" 2>/dev/null || true
cp -a /opt/dca/dashboard /opt/dca/fonts /opt/dca/assets "$PREV/" 2>/dev/null || true

if ! /usr/local/bin/dca-update >/dev/null 2>&1; then
  logger -t dca-autoupdate "dca-update failed for $LATEST, keeping the running build"
  exit 1
fi

for _ in $(seq 1 24); do
  sleep 5
  if curl -fsS -m 10 http://127.0.0.1:3000/health 2>/dev/null | grep -q '"discordStatus":"connected"'; then
    logger -t dca-autoupdate "$LATEST is running and connected"
    rm -f "$STATE/failed"
    exit 0
  fi
done

logger -t dca-autoupdate "$LATEST did not come up healthy within 2 minutes, going back to ${CURRENT:-the previous build}"
echo "$LATEST" > "$STATE/failed"
systemctl stop dca-bot
cp -a "$PREV/dca-bot" /opt/dca/bin/dca-bot
[ -f "$PREV/VERSION" ] && cp -a "$PREV/VERSION" /opt/dca/VERSION
for d in dashboard fonts assets; do
  [ -d "$PREV/$d" ] && { rm -rf "/opt/dca/$d"; cp -a "$PREV/$d" "/opt/dca/$d"; }
done
chown -R dca:dca /opt/dca/dashboard /opt/dca/fonts /opt/dca/assets 2>/dev/null || true
systemctl start dca-bot
exit 1
