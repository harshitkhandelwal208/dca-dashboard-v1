#!/bin/bash
# Run every 2 minutes by dca-autoupdate.timer: install the newest build of the rolling release, and go back to the
# previous build if the new one does not come up healthy. Nothing connects to the server; it only looks at GitHub.
set -u
REPO="${DCA_REPO:-harshitkhandelwal208/dca-dashboard-v1}"
# Paths and timings can be overridden so the script can be tested in a sandbox.
ROOT="${DCA_ROOT:-/opt/dca}"
STATE="${DCA_STATE:-/var/lib/dca-autoupdate}"
LOCK="${DCA_LOCK:-/run/dca-autoupdate.lock}"
UPDATE="${DCA_UPDATE_CMD:-/usr/local/bin/dca-update}"
HEALTH="${DCA_HEALTH_URL:-http://127.0.0.1:3000/health}"
TRIES="${DCA_HEALTH_TRIES:-24}"
PAUSE="${DCA_HEALTH_PAUSE:-5}"
PREV="$ROOT/prev"
mkdir -p "$STATE"
exec 9>"$LOCK"
flock -n 9 || exit 0

LATEST="$(curl -fsS -m 20 "${DCA_VERSION_URL:-https://github.com/${REPO}/releases/download/rolling/VERSION}" 2>/dev/null | awk '{print $1; exit}')"
[ -n "$LATEST" ] || exit 0
CURRENT="$(awk '{print $1; exit}' "$ROOT/VERSION" 2>/dev/null || true)"
[ "$LATEST" = "$CURRENT" ] && exit 0
# A build that failed its health check is not tried again; the next commit gets a fresh chance.
[ "$LATEST" = "$(cat "$STATE/failed" 2>/dev/null || true)" ] && exit 0

logger -t dca-autoupdate "installing $LATEST (running ${CURRENT:-none})"
rm -rf "$PREV"; mkdir -p "$PREV"
cp -a "$ROOT/bin/dca-bot" "$ROOT/VERSION" "$PREV/" 2>/dev/null || true
cp -a "$ROOT/dashboard" "$ROOT/fonts" "$ROOT/assets" "$PREV/" 2>/dev/null || true

if ! "$UPDATE" >/dev/null 2>&1; then
  logger -t dca-autoupdate "dca-update failed for $LATEST, keeping the running build"
  exit 1
fi

for _ in $(seq 1 "$TRIES"); do
  sleep "$PAUSE"
  if curl -fsS -m 10 "$HEALTH" 2>/dev/null | grep -q '"discordStatus":"connected"'; then
    logger -t dca-autoupdate "$LATEST is running and connected"
    rm -f "$STATE/failed"
    exit 0
  fi
done

logger -t dca-autoupdate "$LATEST did not come up healthy within 2 minutes, going back to ${CURRENT:-the previous build}"
echo "$LATEST" > "$STATE/failed"
systemctl stop dca-bot
cp -a "$PREV/dca-bot" "$ROOT/bin/dca-bot"
[ -f "$PREV/VERSION" ] && cp -a "$PREV/VERSION" "$ROOT/VERSION"
for d in dashboard fonts assets; do
  [ -d "$PREV/$d" ] && { rm -rf "${ROOT:?}/$d"; cp -a "$PREV/$d" "$ROOT/$d"; }
done
chown -R dca:dca "$ROOT/dashboard" "$ROOT/fonts" "$ROOT/assets" 2>/dev/null || true
systemctl start dca-bot
exit 1
