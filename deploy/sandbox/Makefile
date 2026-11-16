# Frontal Sandbox deployment.
#
# Uses OpenTofu (`tofu`). If you only have Terraform, set `TF := terraform`.
#
# smoke    requires: SANDBOX_BASE_URL, SANDBOX_API_KEY
# bootstrap requires: BOOTSTRAP_HOST, SANDBOX_BINARY_URL, SANDBOX_API_KEY_HASH
#   (BOOTSTRAP_USER defaults to ubuntu; SANDBOX_CAS_S3_BUCKET /
#    SANDBOX_CAS_S3_ENDPOINT are optional)

TF     ?= tofu
TF_DIR := terraform

.PHONY: init apply destroy smoke bootstrap plan validate fmt

## Terraform lifecycle

init:
	$(TF) -chdir=$(TF_DIR) init

plan:
	$(TF) -chdir=$(TF_DIR) plan

apply:
	$(TF) -chdir=$(TF_DIR) apply

destroy:
	$(TF) -chdir=$(TF_DIR) destroy

validate:
	$(TF) -chdir=$(TF_DIR) validate

fmt:
	$(TF) -chdir=$(TF_DIR) fmt -recursive

## Outputs

host_ip:
	$(TF) -chdir=$(TF_DIR) output -raw sandbox_host_ip

host_dns:
	$(TF) -chdir=$(TF_DIR) output -raw sandbox_host_dns

base_url:
	$(TF) -chdir=$(TF_DIR) output -raw sandbox_base_url

bucket:
	$(TF) -chdir=$(TF_DIR) output -raw s3_bucket_name

## Host bootstrap
## Piped over SSH: SANDBOX_BINARY_URL, SANDBOX_API_KEY_HASH required;
## SANDBOX_CAS_S3_BUCKET / SANDBOX_CAS_S3_ENDPOINT optional.

bootstrap:
	@test -n "$${BOOTSTRAP_HOST:?BOOTSTRAP_HOST is required (see `make host_ip`)}"
	@test -n "$${SANDBOX_BINARY_URL:?SANDBOX_BINARY_URL is required}"
	@test -n "$${SANDBOX_API_KEY_HASH:?SANDBOX_API_KEY_HASH is required}"
	ssh "$${BOOTSTRAP_USER:-ubuntu}@$${BOOTSTRAP_HOST}" \
		'SANDBOX_BINARY_URL="$$SANDBOX_BINARY_URL" \
		 SANDBOX_API_KEY_HASH="$$SANDBOX_API_KEY_HASH" \
		 SANDBOX_CAS_S3_BUCKET="$$SANDBOX_CAS_S3_BUCKET" \
		 SANDBOX_CAS_S3_ENDPOINT="$$SANDBOX_CAS_S3_ENDPOINT" \
		 bash -s' < scripts/bootstrap.sh

## Smoke test

smoke:
	@scripts/smoke.sh
