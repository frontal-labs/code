# deploy/sandbox/terraform

Self-contained OpenTofu/Terraform module that provisions a KVM-capable AWS
host for the [Frontal Sandbox](https://github.com/frontal-labs/sandbox)
server.

## Relationship to the upstream sandbox repo

The upstream sandbox repo ships its own `infrastructure/terraform/` module
(`main.tf`, `providers.tf`, `versions.tf`, `variables.tf`, `outputs.tf`,
`compute.tf`, `networking.tf`, `storage.tf`, `firewall.tf`). This wrapper is
**intentionally independent** of it: it creates every resource directly so an
operator can `tofu init && tofu apply` in isolation, without having to
reconcile the upstream module's provider and variable surface.

If you need multi-cloud support or the upstream module's exact resource
layout, use `frontal-labs/sandbox`'s `infrastructure/terraform/` directly
instead of this wrapper.

## Layout

```
terraform/
├── main.tf         # terraform block, required_providers, locals
├── providers.tf    # AWS provider + default tags
├── variables.tf    # inputs
├── outputs.tf      # outputs
├── networking.tf   # VPC, public + private subnets, IGW, NAT, route tables
├── compute.tf      # EC2 instance + EBS data volume + attachment
├── firewall.tf     # security group (SSH 22, API 8080, scheduler 50051)
└── storage.tf      # S3 CAS bucket (versioned, KMS-encrypted, private)
```

The instance lives in a **private subnet** with a NAT gateway for outbound
egress. If you need inbound reachability on port 8080 from the internet,
either add an elastic IP / NAT for inbound, or place the instance in the
public subnet (the security group already allows 8080 from
`allow_cidr_blocks`).

## Variables

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `aws_region` | string | `us-east-1` | AWS region to deploy into. |
| `environment` | string | `dev` | Prefix for all resource names. |
| `instance_type` | string | `m5.metal` | EC2 type. **KVM requires bare metal** (or nested virt support) for Tier 2 VM sessions; use `m5.large` or similar for container-only (Tier 1) deploys. |
| `ami_id` | string | *(required)* | Ubuntu 22.04/24.04 AMI in the target region. |
| `ssh_key_name` | string | *(required)* | EC2 key pair name for operator SSH access. |
| `vpc_cidr` | string | `10.200.0.0/16` | VPC CIDR. |
| `public_subnet_cidr` | string | `10.200.0.0/24` | Public subnet CIDR (NAT lives here). |
| `private_subnet_cidr` | string | `10.200.1.0/24` | Private subnet CIDR (instance lives here). |
| `allow_cidr_blocks` | list(string) | `["0.0.0.0/0"]` | CIDRs allowed to reach the host (SSH + API + scheduler). **Tighten this before production.** |
| `data_volume_size` | number | `100` | GiB of the EBS volume mounted at `/var/lib/sandbox`. |
| `root_volume_size` | number | `40` | GiB of the instance root volume. |
| `create_object_store` | bool | `true` | Create an S3 bucket for the CAS tier. |

## Outputs

| Output | Description |
| --- | --- |
| `sandbox_host_ip` | Public IP of the sandbox host (empty when the instance is in a private subnet without an EIP). |
| `sandbox_host_dns` | Public DNS name of the sandbox host (empty when there is no public IP). |
| `s3_bucket_name` | S3 bucket name for the CAS tier (empty when `create_object_store = false`). |
| `sandbox_base_url` | `http://<public_ip>:8080` — use as `FRONTAL_SANDBOX_BASE_URL`. |

## Usage

```bash
cd deploy/sandbox
tofu -chdir=terraform init
tofu -chdir=terraform apply -var ami_id=ami-... -var ssh_key_name=my-key
tofu -chdir=terraform output -raw sandbox_host_dns
```

`terraform` (HashiCorp) works identically in place of `tofu`
([OpenTofu](https://opentofu.org) is Terraform-compatible).
