#!/bin/bash
# First-boot setup of the EC2 instance (Ubuntu 24.04), run as root by cloud-init (user-data) or by hand.
# Installs the runtime packages, a swap file, the service user, Caddy (HTTPS reverse proxy) and the update script.
set -euxo pipefail
export DEBIAN_FRONTEND=noninteractive
REPO="${DCA_REPO:-harshitkhandelwal208/dca-dashboard-v1}"
BRANCH="${DCA_BRANCH:-main}"

apt-get update -y
apt-get install -y ca-certificates curl fontconfig fonts-noto-core fonts-noto-cjk unzip debian-keyring debian-archive-keyring apt-transport-https gpg libstdc++6
# Caddy from its official repository.
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' > /etc/apt/sources.list.d/caddy-stable.list
apt-get update -y && apt-get install -y caddy

# 2 GB of swap as a safety net on a 1 GB machine.
if [ ! -f /swapfile ]; then
  fallocate -l 2G /swapfile && chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile
  echo '/swapfile none swap sw 0 0' >> /etc/fstab
  echo 'vm.swappiness=20' > /etc/sysctl.d/99-dca.conf && sysctl -p /etc/sysctl.d/99-dca.conf
fi

id dca >/dev/null 2>&1 || useradd --system --home /opt/dca --shell /usr/sbin/nologin dca
mkdir -p /opt/dca/{bin,data,models,fonts,assets,dashboard} /etc/dca
chown -R dca:dca /opt/dca
chmod 750 /etc/dca; chown root:dca /etc/dca

RAW="https://raw.githubusercontent.com/${REPO}/${BRANCH}/deploy/aws"
curl -fsSL "$RAW/dca-bot.service" -o /etc/systemd/system/dca-bot.service
curl -fsSL "$RAW/Caddyfile.template" -o /etc/dca/Caddyfile.template
curl -fsSL "$RAW/update.sh" -o /usr/local/bin/dca-update && chmod 755 /usr/local/bin/dca-update
for f in dca-health.service dca-health.timer dca-autoupdate.service dca-autoupdate.timer; do curl -fsSL "$RAW/$f" -o "/etc/systemd/system/$f"; done
curl -fsSL "$RAW/dca-health.sh" -o /usr/local/bin/dca-health && chmod 755 /usr/local/bin/dca-health
curl -fsSL "$RAW/dca-autoupdate.sh" -o /usr/local/bin/dca-autoupdate && chmod 755 /usr/local/bin/dca-autoupdate
systemctl daemon-reload
systemctl enable --now dca-health.timer dca-autoupdate.timer
echo "bootstrap done" > /var/log/dca-bootstrap.done
