provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      environment = var.environment
      component   = "sandbox"
      managed_by  = "tofu"
    }
  }
}
