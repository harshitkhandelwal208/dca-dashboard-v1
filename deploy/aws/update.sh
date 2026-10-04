#!/bin/bash
# Install the latest build (GitHub release "rolling") and restart the bot. Run as root: `sudo dca-update`.
# The OCR models are downloaded by the bot itself on first start (into /opt/dca/models).
set -euo pipefail
REPO="${DCA_REPO:-harshitkhandelwal208/dca-dashboard-v1}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
BRANCH="${DCA_BRANCH:-main}"
# Refresh the service unit, the health check and this script itself from the repository.
RAW="https://raw.githubusercontent.com/${REPO}/${BRANCH}/deploy/aws"
for f in dca-bot.service dca-health.service dca-health.timer; do curl -fsSL "$RAW/$f" -o "/etc/systemd/system/$f"; done
curl -fsSL "$RAW/dca-health.sh" -o /usr/local/bin/dca-health && chmod 755 /usr/local/bin/dca-health
curl -fsSL "$RAW/update.sh" -o /usr/local/bin/dca-update.new && chmod 755 /usr/local/bin/dca-update.new && mv /usr/local/bin/dca-update.new /usr/local/bin/dca-update
systemctl daemon-reload
systemctl enable --now dca-health.timer >/dev/null 2>&1 || true
curl -fsSL "https://github.com/${REPO}/releases/download/rolling/dca-bot-linux-x86_64.tar.gz" -o "$TMP/dca.tar.gz"
tar -xzf "$TMP/dca.tar.gz" -C "$TMP"
install -d -o dca -g dca /opt/dca/bin
install -m 755 -o dca -g dca "$TMP/dca-bot/dca-bot" /opt/dca/bin/dca-bot.new
rm -rf /opt/dca/dashboard /opt/dca/fonts /opt/dca/assets
cp -r "$TMP/dca-bot/dashboard" "$TMP/dca-bot/fonts" "$TMP/dca-bot/assets" /opt/dca/
chown -R dca:dca /opt/dca/dashboard /opt/dca/fonts /opt/dca/assets
mv /opt/dca/bin/dca-bot.new /opt/dca/bin/dca-bot
cp "$TMP/dca-bot/VERSION" /opt/dca/VERSION 2>/dev/null || true
systemctl enable dca-bot >/dev/null 2>&1 || true
systemctl restart dca-bot
sleep 5
systemctl --no-pager --lines=15 status dca-bot || true
echo "installed: $(cat /opt/dca/VERSION 2>/dev/null || echo unknown)"
