# ---------------------------------------------------------------------------
# Security group for the sandbox host.
#
# Ingress:
#   - SSH (22)       from allow_cidr_blocks
#   - API  (8080)    from allow_cidr_blocks
#   - Scheduler gRPC (50051) from allow_cidr_blocks (only needed if code-server
#     talks to the scheduler directly; kept for parity with the upstream plan)
#
# Egress: all outbound (instance needs internet for package install and
# S3 access).
# ---------------------------------------------------------------------------

resource "aws_security_group" "sandbox" {
  name_prefix = "${local.name_prefix}-"
  description = "Frontal Sandbox server"
  vpc_id      = aws_vpc.sandbox.id

  ingress {
    description = "SSH"
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = var.allow_cidr_blocks
  }

  ingress {
    description = "Sandbox API (HTTP)"
    from_port   = 8080
    to_port     = 8080
    protocol    = "tcp"
    cidr_blocks = var.allow_cidr_blocks
  }

  ingress {
    description = "Sandbox scheduler gRPC"
    from_port   = 50051
    to_port     = 50051
    protocol    = "tcp"
    cidr_blocks = var.allow_cidr_blocks
  }

  egress {
    description = "All outbound"
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = {
    Name = "${local.name_prefix}-sg"
  }
}
