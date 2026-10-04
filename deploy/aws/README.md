# Deploying on AWS EC2

One small EC2 instance runs the bot and the dashboard (a single Rust process) behind Caddy, which serves HTTPS with an automatic certificate. State stays in Firebase, so the instance holds nothing that cannot be rebuilt.

| | |
| --- | --- |
| Region / instance | ap-southeast-2 (Sydney; override with `AWS_REGION`, e.g. us-east-1 is closest to Discord's gateway), `t3.micro` (2 vCPU burst, 1 GB) + 2 GB swap, Ubuntu 24.04, 20 GB gp3, standard CPU credits (never billed for bursting) |
| Cost | about $15.2 per month (instance $9.64 at $0.0132/h in Sydney, disk $1.90, public IPv4 $3.65; traffic and the CloudWatch alarm are inside the free allowance): about $91 for six months, which leaves about $29 of a $120 credit as margin. A $20 monthly budget (`dca-bot-monthly`, gross cost, credits not subtracted) shows overspending early |
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

## Staying up

`systemd` restarts the bot whenever it exits; `dca-autoupdate.timer` installs new builds (see Updating); `dca-health.timer` checks `/health` every minute and restarts the bot when the Discord gateway has not been connected for 5 checks in a row; a CloudWatch alarm (`dca-bot-auto-recover`) lets AWS move the instance to healthy hardware if the host fails; Caddy renews the certificate by itself. State lives in Firebase, so a restart or a rebuilt instance loses nothing.

## Updating

Nothing to do: push to `main`. CI runs the tests; when they pass, the "Release build" workflow publishes the build as the `rolling` release (a failed CI run publishes nothing, and a newer commit replaces a build that is still running). The server looks at that release every 2 minutes (`dca-autoupdate.timer`), installs a newer build and checks that the bot comes up connected within 2 minutes; if it does not, the previous build is put back and that version is not tried again until the next commit. A push is live about 8 minutes later.

Nothing connects to the server and GitHub holds no credentials for it: the server only reads the public release.

```bash
sudo journalctl -t dca-autoupdate -n 20      # what the updater did
sudo systemctl start dca-autoupdate          # check right now instead of waiting
sudo dca-update                              # install the latest build by hand
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
