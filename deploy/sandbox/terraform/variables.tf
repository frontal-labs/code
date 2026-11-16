variable "aws_region" {
  description = "AWS region to deploy into."
  type        = string
  default     = "us-east-1"
}

variable "environment" {
  description = "Environment name used as a prefix for all resources."
  type        = string
  default     = "dev"
}

variable "instance_type" {
  description = <<-EOT
    EC2 instance type for the sandbox host.

    Tier 2 (VM) sessions require /dev/kvm, which on AWS means a bare-metal
    instance type such as "m5.metal". Container-only (Tier 1) deployments
    can use a regular type like "m5.large". See README.md for details.
  EOT
  type        = string
  default     = "m5.metal"
}

variable "ami_id" {
  description = "AMI ID for the sandbox host (Ubuntu 24.04 LTS recommended)."
  type        = string
}

variable "ssh_key_name" {
  description = "Name of the EC2 SSH key pair to attach to the instance."
  type        = string
}

variable "vpc_cidr" {
  description = "CIDR block for the VPC."
  type        = string
  default     = "10.200.0.0/16"
}

variable "public_subnet_cidr" {
  description = "CIDR block for the public subnet (NAT gateway + egress)."
  type        = string
  default     = "10.200.0.0/24"
}

variable "private_subnet_cidr" {
  description = "CIDR block for the private subnet (sandbox instance lives here)."
  type        = string
  default     = "10.200.1.0/24"
}

variable "allow_cidr_blocks" {
  description = <<-EOT
    List of CIDR blocks allowed to reach the sandbox host (SSH 22 and
    API 8080). Restrict to your operator / code-server CIDRs.
  EOT
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "data_volume_size" {
  description = "Size in GiB of the EBS volume mounted at /var/lib/sandbox."
  type        = number
  default     = 100
}

variable "root_volume_size" {
  description = "Size in GiB of the instance root volume."
  type        = number
  default     = 40
}

variable "create_object_store" {
  description = "Whether to create an S3 bucket for the CAS tier."
  type        = bool
  default     = true
}
