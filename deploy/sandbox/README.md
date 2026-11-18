# Frontal Sandbox — deployment wrapper

A turnkey deployment for the [Frontal Sandbox server](https://github.com/frontal-labs/sandbox):
it provisions a KVM-capable host plus its object store (CAS tier) with OpenTofu,
then installs and runs the sandbox server that code-server's new
`SandboxLaneWorkerTransport` talks to over HTTP (port 8080).

The sandbox server listens on port 8080 (API + `/metrics`) and needs `/dev/kvm`
for Tier 2 (VM) sessions. It is configured via a TOML file at
`/etc/sandbox/config.toml` with env var overrides (`SANDBOX_<SECTION>_<KEY>`).

The upstream sandbox repo also ships its own `infrastructure/terraform/`
module (AWS/GCP/Azure) and a clustered deployment. This directory is a
**self-contained single-node deployment** — see
[terraform/README.md](terraform/README.md) for the relationship.

## Layout

```
deploy/sandbox/
├── README.md
├── Makefile
├── config/
│   └── production.toml          # sandbox server TOML (installed to /etc/sandbox/config.toml)
├── scripts/
│   ├── bootstrap.sh             # prerequisites, KVM, binary, config, systemd unit
│   ├── deploy.sh                # restart + verify on an already-bootstrapped host
│   └── smoke.sh                 # API smoke test (openapi → create → exec → destroy)
└── terraform/
    ├── main.tf                  # terraform block, required_providers, locals
    ├── providers.tf             # AWS provider
    ├── variables.tf             # inputs
    ├── outputs.tf               # outputs
    ├── networking.tf            # VPC, public + private subnets, IGW, NAT
    ├── compute.tf               # EC2 instance + EBS data volume
    ├── firewall.tf              # security group (SSH 22, API 8080, scheduler 50051)
    └── storage.tf               # S3 CAS bucket (versioned, encrypted, private)
```

## Prerequisites

- [OpenTofu](https://opentofu.org) (`tofu`) on PATH — Terraform ≥ 1.5 works too
  (the Makefile uses `tofu`; set `TF=terraform` to switch).
- Cloud provider credentials (AWS: `AWS_PROFILE` or `AWS_ACCESS_KEY_ID` /
  `AWS_SECRET_ACCESS_KEY` env vars) with EC2, VPC, EBS, and S3 permissions.
- An SSH key pair registered in EC2 (pass its name via `-var ssh_key_name=...`).
- An AMI ID for the host (Ubuntu 22.04/24.04):

  ```sh
  aws ec2 describe-images --owners 099720109477 \
    --filters Name=name,values='ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*' \
    Name=root-device-type,values=ebs --query 'Images[0].ImageId' --output text
  ```

## Quick start

From `deploy/sandbox/`, five steps to a running sandbox server:

```sh
# 1. Initialize (installs the AWS provider)
cd deploy/sandbox && tofu -chdir=terraform init

# 2. Provision the KVM host + object store + firewall.
#    Provide at least: ami_id, ssh_key_name. Tighten allow_cidr_blocks.
tofu -chdir=terraform apply -auto-approve \
  -var ami_id=ami-0... \
  -var ssh_key_name=my-key \
  -var 'allow_cidr_blocks=["203.0.113.0/24"]'

# 3. Get onto the host
ssh ubuntu@$(tofu -chdir=terraform output -raw sandbox_host_ip) sudo -i

# 4. Install the binary, config, and systemd unit, then start
export SANDBOX_BINARY_URL="https://.../sandbox-x86_64-unknown-linux-gnu"
# optional: export SANDBOX_API_KEY_HASH=$(printf '%s' "$FRONTAL_SANDBOX_API_KEY" | shasum -a 256)
./scripts/bootstrap.sh && ./scripts/deploy.sh

# 5. Verify the server is healthy
./scripts/smoke.sh
```

> Note: the instance lives in a **private subnet**. `sandbox_host_ip` may be
> empty; reach the host over your VPN/bastion, or add an Elastic IP to make
> it reachable. The security group allows inbound 8080 only from
> `allow_cidr_blocks`.

## Pointing code-server at the sandbox server

In code-server's environment (e.g. `.env`):

```sh
FRONTAL_SANDBOX_BASE_URL=http://<host>:8080   # `tofu -chdir=terraform output -raw sandbox_base_url`
FRONTAL_SANDBOX_API_KEY=sbx_...               # your sandbox API key
FRONTAL_CODE_SERVER_LANE_TRANSPORT=sandbox
```

The server authenticates with the SHA-256 hex hash of the API key. Pass the
hash (never the raw key) to `bootstrap.sh` as `SANDBOX_API_KEY_HASH`; it is
injected into the systemd unit as `SANDBOX_AUTH_API_KEY_HASH`.

## Destroy

```sh
cd deploy/sandbox
tofu -chdir=terraform destroy -auto-approve
```

Removes the EC2 instance, EBS volumes, S3 bucket, and all networking.
Anything under `/var/lib/sandbox` is gone with the instance; CAS objects are
gone with the bucket. Back up anything you need first.

## Configuration reference

The sandbox server reads `/etc/sandbox/config.toml`. The production defaults
live in [`config/production.toml`](config/production.toml); every key can be
overridden with a `SANDBOX_<SECTION>_<KEY>` env var (e.g.
`SANDBOX_AUTH_RATE_LIMIT_RPS`).

| Section | Key | Description |
| --- | --- | --- |
| `server` | `addr` | API listen address (`0.0.0.0:8080`) |
| `server` | `data_dir` | State / CAS data directory (`/var/lib/sandbox`) |
| `auth` | `api_key_hash` | SHA-256 hex of the API key. Keep in `SANDBOX_AUTH_API_KEY_HASH` env var, never in the TOML. |
| `auth` | `rate_limit_rps` | API rate limit (requests/sec) |
| `tls` | `enabled` | TLS on the sandbox server. Not supported yet (v0.x) — terminate TLS at a reverse proxy. |
| `tls` | `cert_file` / `key_file` | Certificate / key paths (when `enabled = true`) |
| `telemetry` | `metrics_addr` | Prometheus metrics endpoint (`0.0.0.0:9090`) |
| `telemetry` | `otlp_endpoint` | Optional OTLP endpoint |
| `telemetry` | `service_name` | Service name reported by telemetry |
| `scheduler` | `address` | Scheduler gRPC listen address |

## Troubleshooting

- **KVM not available** (`/dev/kvm` missing): the instance type must support
  KVM — on AWS that means bare metal (e.g. `m5.metal`) or a provider exposing
  nested virtualization. Check with `ls -l /dev/kvm` and
  `dmesg | grep -i kvm`. Tier 1 (container) sessions work without KVM; only
  Tier 2 (VM) sessions need it. `bootstrap.sh` warns but continues when
  `/dev/kvm` is absent.
- **Port 8080 blocked**: the security group only allows 8080 from
  `allow_cidr_blocks`. Make sure the code-server's egress CIDR is in the list
  and that no corporate firewall in between drops it. Test from the
  code-server host: `curl -v http://<host>:8080/v1/openapi.json`.
- **SSH from anywhere (port 22)**: the default `allow_cidr_blocks` is
  `0.0.0.0/0`, which opens SSH to the world. Replace it with your operator
  CIDR before applying anywhere real.
- **`smoke.sh` fails to create a session**: verify
  `SANDBOX_API_KEY` (the key `smoke.sh` sends) matches the hash installed on the server
  (`SANDBOX_AUTH_API_KEY_HASH` in `/etc/systemd/system/sandbox-server.service`). It must
  equal the `FRONTAL_SANDBOX_API_KEY` that code-server uses as its caller key.
  See `journalctl -u sandbox-server -n 100` on the host.
- **Service not active**: `systemctl status sandbox-server` and
  `journalctl -u sandbox-server -n 100`. The most common causes are a bad
  `SANDBOX_BINARY_URL` download (check `/usr/local/bin/sandbox` runs
  `--version`) and a stale config (compare `/etc/sandbox/config.toml` with
  `config/production.toml`).
- **Instance unreachable**: it lives in a private subnet with NAT-only
  egress; inbound traffic requires your VPN/bastion or an added Elastic IP.
