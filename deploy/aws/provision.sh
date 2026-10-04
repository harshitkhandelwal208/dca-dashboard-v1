#!/bin/bash
# Create (or find) the EC2 instance for the DCA bot, give it a fixed IP and install the bot on it.
#
#   aws login            # or any other way to get AWS credentials
#   deploy/aws/provision.sh /path/to/production.env
#
# production.env holds the bot's settings (see .env.example). `__DOMAIN__` in it is replaced by the instance's
# address. Safe to run again: existing resources are reused. Costs about $14/month in ap-southeast-2
# (t3.micro + 20 GB disk + public IPv4).
set -euo pipefail

ENV_FILE="${1:?usage: provision.sh /path/to/production.env}"
REGION="${AWS_REGION:-ap-southeast-2}"
NAME="${DCA_NAME:-dca-bot}"
TYPE="${DCA_INSTANCE_TYPE:-t3.micro}"
KEY="$HOME/.ssh/${NAME}.pem"
HERE="$(cd "$(dirname "$0")" && pwd)"
export AWS_REGION="$REGION" AWS_DEFAULT_REGION="$REGION" AWS_PAGER=""

aws sts get-caller-identity --query Arn --output text >/dev/null || { echo "No AWS credentials: run 'aws login' first." >&2; exit 1; }
MYIP="$(curl -fsS https://checkip.amazonaws.com | tr -d '\n')"
VPC="$(aws ec2 describe-vpcs --filters Name=isDefault,Values=true --query 'Vpcs[0].VpcId' --output text)"

# --- key pair --------------------------------------------------------------------------------------------
if ! aws ec2 describe-key-pairs --key-names "$NAME" >/dev/null 2>&1; then
  mkdir -p "$HOME/.ssh"
  aws ec2 create-key-pair --key-name "$NAME" --query KeyMaterial --output text > "$KEY"
  chmod 600 "$KEY"
  echo "created key pair, private key saved to $KEY"
fi
[ -f "$KEY" ] || { echo "Key pair '$NAME' exists in AWS but $KEY is missing; delete the key pair or set DCA_NAME." >&2; exit 1; }

# --- security group: HTTPS/HTTP for everyone, SSH only from this machine ---------------------------------
SG="$(aws ec2 describe-security-groups --filters Name=group-name,Values="$NAME" Name=vpc-id,Values="$VPC" --query 'SecurityGroups[0].GroupId' --output text)"
if [ "$SG" = "None" ]; then
  SG="$(aws ec2 create-security-group --group-name "$NAME" --description "DCA bot" --vpc-id "$VPC" --query GroupId --output text)"
  aws ec2 authorize-security-group-ingress --group-id "$SG" --ip-permissions \
    "IpProtocol=tcp,FromPort=80,ToPort=80,IpRanges=[{CidrIp=0.0.0.0/0}]" \
    "IpProtocol=tcp,FromPort=443,ToPort=443,IpRanges=[{CidrIp=0.0.0.0/0}]" >/dev/null
fi
# (Re)allow SSH from the current address.
aws ec2 authorize-security-group-ingress --group-id "$SG" --protocol tcp --port 22 --cidr "$MYIP/32" >/dev/null 2>&1 || true

# --- instance ----------------------------------------------------------------------------------------------
IID="$(aws ec2 describe-instances --filters Name=tag:Name,Values="$NAME" Name=instance-state-name,Values=pending,running,stopped --query 'Reservations[0].Instances[0].InstanceId' --output text)"
if [ "$IID" = "None" ]; then
  AMI="$(aws ssm get-parameter --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id --query Parameter.Value --output text)"
  IID="$(aws ec2 run-instances --image-id "$AMI" --instance-type "$TYPE" --key-name "$NAME" --security-group-ids "$SG" \
    --block-device-mappings 'DeviceName=/dev/sda1,Ebs={VolumeSize=20,VolumeType=gp3,DeleteOnTermination=true}' \
    --credit-specification CpuCredits=standard --metadata-options HttpTokens=required,HttpEndpoint=enabled \
    --disable-api-termination --user-data "file://$HERE/bootstrap.sh" \
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$NAME}]" "ResourceType=volume,Tags=[{Key=Name,Value=$NAME}]" \
    --query 'Instances[0].InstanceId' --output text)"
  echo "launched $IID"
fi
aws ec2 wait instance-running --instance-ids "$IID"
# Let AWS move the instance to healthy hardware if the host fails (best effort: needs CloudWatch access).
aws cloudwatch put-metric-alarm --alarm-name "$NAME-auto-recover" --namespace AWS/EC2 --metric-name StatusCheckFailed_System \
  --dimensions Name=InstanceId,Value="$IID" --statistic Maximum --period 60 --evaluation-periods 2 --threshold 0 \
  --comparison-operator GreaterThanThreshold --alarm-actions "arn:aws:automate:$REGION:ec2:recover" >/dev/null 2>&1 || true

# --- fixed public address ------------------------------------------------------------------------------------
EIP="$(aws ec2 describe-addresses --filters Name=tag:Name,Values="$NAME" --query 'Addresses[0].AllocationId' --output text)"
if [ "$EIP" = "None" ]; then
  EIP="$(aws ec2 allocate-address --domain vpc --tag-specifications "ResourceType=elastic-ip,Tags=[{Key=Name,Value=$NAME}]" --query AllocationId --output text)"
fi
aws ec2 associate-address --instance-id "$IID" --allocation-id "$EIP" --allow-reassociation >/dev/null
IP="$(aws ec2 describe-addresses --allocation-ids "$EIP" --query 'Addresses[0].PublicIp' --output text)"
DOMAIN="${DCA_DOMAIN:-${IP//./-}.sslip.io}"   # sslip.io resolves a-b-c-d.sslip.io to a.b.c.d: HTTPS without owning a domain
echo "instance $IID  address $IP  dashboard https://$DOMAIN"

# --- install the bot -----------------------------------------------------------------------------------------
SSH=(ssh -i "$KEY" -o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 "ubuntu@$IP")
for _ in $(seq 1 60); do "${SSH[@]}" true 2>/dev/null && break; sleep 5; done
echo "waiting for the first-boot setup..."
for _ in $(seq 1 120); do "${SSH[@]}" test -f /var/log/dca-bootstrap.done 2>/dev/null && break; sleep 5; done
"${SSH[@]}" test -f /var/log/dca-bootstrap.done || { echo "bootstrap did not finish; see /var/log/cloud-init-output.log on the instance" >&2; exit 1; }

TMPENV="$(mktemp)"; trap 'rm -f "$TMPENV"' EXIT
sed "s/__DOMAIN__/$DOMAIN/g" "$ENV_FILE" > "$TMPENV"
scp -i "$KEY" -q "$TMPENV" "ubuntu@$IP:/tmp/dca.env"
"${SSH[@]}" "sudo install -m 640 -o root -g dca /tmp/dca.env /etc/dca/dca.env && rm /tmp/dca.env && sudo sed 's/__DOMAIN__/$DOMAIN/' /etc/dca/Caddyfile.template | sudo tee /etc/caddy/Caddyfile >/dev/null && sudo systemctl enable --now caddy && sudo systemctl reload caddy && sudo dca-update"
echo
echo "Done. Dashboard: https://$DOMAIN/dashboard   health: https://$DOMAIN/health"
echo "Register this redirect URI in the Discord developer portal: https://$DOMAIN/auth/discord/callback"
