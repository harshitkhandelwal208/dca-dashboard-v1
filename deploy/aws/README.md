# Deploying on AWS EC2

One small EC2 instance runs the bot and the dashboard (a single Rust process) behind Caddy, which serves HTTPS with an automatic certificate. State stays in Firebase, so the instance holds nothing that cannot be rebuilt.

| | |
| --- | --- |
| Region / instance | us-east-1 (closest to Discord's gateway), `t3.micro` (2 vCPU burst, 1 GB) + 2 GB swap, Ubuntu 24.04, 20 GB gp3, standard CPU credits (never billed for bursting) |
| Cost | about $13 per month (instance $7.6, disk $1.9, public IPv4 $3.7): roughly $80 for six months |
| Address | an Elastic IP; the dashboard uses `https://<ip-with-dashes>.sslip.io` (sslip.io resolves it to the IP, so no domain is needed). Set `DCA_DOMAIN=your.domain` and point an A record at the IP to use your own |
| Builds | GitHub Actions (`.github/workflows/release.yml`) builds the binary and the dashboard on every push to `main` and publishes them as the `rolling` release; the instance only downloads and restarts, it never compiles |

## First deployment

```bash
aws login                               # opens the browser; any other way of getting credentials works too
cp deploy/aws/.env.example ~/dca.env    # fill it in (or reuse the production settings); keep it private
deploy/aws/provision.sh ~/dca.env
```

`provision.sh` creates the key pair (`~/.ssh/dca-bot.pem`), a security group (80/443 open, SSH only from your current IP), the instance, the Elastic IP, installs the service and Caddy, uploads the settings and starts the bot. It is safe to run again.

Afterwards:

1. Register `https://<address>/auth/discord/callback` as an OAuth2 redirect in the Discord developer portal (the script prints it).
2. Stop the old host (Render) before or right after the first start: two processes with the same token answer every command twice.
3. `https://<address>/health` should report `"discordStatus":"connected"`. The OCR models (about 100 MB) download in the background on the first start, `"ocr"` turns `ready` after a minute or two.

## Updating

Push to `main`; when the "Release build" workflow is green:

```bash
ssh -i ~/.ssh/dca-bot.pem ubuntu@<ip> sudo dca-update
```

## Operating

```bash
ssh -i ~/.ssh/dca-bot.pem ubuntu@<ip>
sudo journalctl -u dca-bot -f          # logs
sudo systemctl restart dca-bot
sudoedit /etc/dca/dca.env && sudo systemctl restart dca-bot    # change settings
```

If your IP changes, allow SSH again with `aws ec2 authorize-security-group-ingress --group-id <sg> --protocol tcp --port 22 --cidr <new-ip>/32` (or run `provision.sh` again).

The instance has API termination protection on; to remove everything: disable it, terminate the instance, release the Elastic IP and delete the security group and key pair.
