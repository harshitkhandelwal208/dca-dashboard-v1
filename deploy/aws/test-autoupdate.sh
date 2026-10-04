#!/bin/bash
# Sandbox test of dca-autoupdate.sh: no network, no systemd, no real server. Run: deploy/aws/test-autoupdate.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

mkdir -p "$T/stubs"
# Stubs for the commands that would touch the real machine; every call is recorded.
printf '#!/bin/bash\necho "$@" >> "%s/systemctl.log"\n' "$T" > "$T/stubs/systemctl"
printf '#!/bin/bash\nexit 0\n' > "$T/stubs/logger"
printf '#!/bin/bash\nexit 0\n' > "$T/stubs/chown"
chmod +x "$T"/stubs/*

fresh_root() {
  rm -rf "$T/root" "$T/state"; : > "$T/systemctl.log"; : > "$T/update.calls"
  mkdir -p "$T/root/bin" "$T/root/dashboard" "$T/root/fonts" "$T/root/assets"
  echo "old-binary" > "$T/root/bin/dca-bot"; echo "oldsha 2026-01-01T00:00Z" > "$T/root/VERSION"; echo "old-ui" > "$T/root/dashboard/index.html"
}
# What the real dca-update does, reduced to the files that matter.
printf '#!/bin/bash\necho x >> "%s/update.calls"\n[ -f "%s/update.fails" ] && exit 1\necho new-binary > "%s/root/bin/dca-bot"\ncat "%s/latest" > "%s/root/VERSION"\necho new-ui > "%s/root/dashboard/index.html"\n' "$T" "$T" "$T" "$T" "$T" "$T" > "$T/update.sh"
chmod +x "$T/update.sh"

run() {
  PATH="$T/stubs:$PATH" DCA_ROOT="$T/root" DCA_STATE="$T/state" DCA_LOCK="$T/lock" DCA_UPDATE_CMD="$T/update.sh" \
  DCA_VERSION_URL="file://$T/latest" DCA_HEALTH_URL="file://$T/health" DCA_HEALTH_TRIES=2 DCA_HEALTH_PAUSE=0 \
  bash "$HERE/dca-autoupdate.sh"
}
calls() { wc -l < "$T/update.calls" | tr -d ' '; }

echo "--- same version: nothing to do"
fresh_root; echo "oldsha 2026-01-01T00:00Z" > "$T/latest"; echo '{"discordStatus":"connected"}' > "$T/health"
run; check "exit 0" "[ $? -eq 0 ]"; check "update not run" "[ \"\$(calls)\" = 0 ]"

echo "--- no release marker reachable: nothing to do"
fresh_root; rm -f "$T/latest"; run; check "update not run" "[ \"\$(calls)\" = 0 ]"

echo "--- newer version, comes up healthy: stays installed"
fresh_root; echo "newsha 2026-02-02T00:00Z" > "$T/latest"; echo '{"discordStatus":"connected"}' > "$T/health"
run; check "exit 0" "[ $? -eq 0 ]"
check "new binary installed" "grep -q new-binary '$T/root/bin/dca-bot'"
check "no failed marker" "[ ! -e '$T/state/failed' ]"

echo "--- newer version, never healthy: rolled back"
fresh_root; echo "badsha 2026-03-03T00:00Z" > "$T/latest"; echo '{"discordStatus":"connecting"}' > "$T/health"
run; check "exit 1" "[ $? -eq 1 ]"
check "old binary restored" "grep -q old-binary '$T/root/bin/dca-bot'"
check "old version restored" "grep -q '^oldsha' '$T/root/VERSION'"
check "old dashboard restored" "grep -q old-ui '$T/root/dashboard/index.html'"
check "bad version remembered" "grep -q badsha '$T/state/failed'"
check "bot stopped and started" "grep -q '^stop dca-bot' '$T/systemctl.log' && grep -q '^start dca-bot' '$T/systemctl.log'"
echo "--- the same bad version is not tried again"
: > "$T/update.calls"; run; check "update not run again" "[ \"\$(calls)\" = 0 ]"
echo "--- a newer commit gets a fresh chance"
echo "fixsha 2026-03-04T00:00Z" > "$T/latest"; echo '{"discordStatus":"connected"}' > "$T/health"; run
check "fixed build installed" "grep -q new-binary '$T/root/bin/dca-bot'"; check "failed marker cleared" "[ ! -e '$T/state/failed' ]"

echo "--- the install itself fails: running build is kept"
fresh_root; echo "newsha 2026-02-02T00:00Z" > "$T/latest"; touch "$T/update.fails"
run; check "exit 1" "[ $? -eq 1 ]"; check "old binary untouched" "grep -q old-binary '$T/root/bin/dca-bot'"; rm -f "$T/update.fails"

if [ "$fail" = 0 ]; then
  echo "all checks passed"
else
  echo "SOME CHECKS FAILED"
  exit 1
fi
