# Frontal Sandbox — self-contained AWS deployment.
#
# This directory intentionally does not depend on the upstream sandbox repo's
# `infrastructure/terraform/` modules. It creates every resource directly so
# an operator can `tofu init && tofu apply` in isolation, with no network
# fetch beyond provider installs.

terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

# Local variables derived from inputs.

locals {
  name_prefix = "${var.environment}-sandbox"
}
