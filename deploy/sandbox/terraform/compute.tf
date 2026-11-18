# ---------------------------------------------------------------------------
# Sandbox host instance.
#
# KVM (Tier 2 / VM sessions) requires /dev/kvm, which on AWS means a
# bare-metal instance type (e.g. m5.metal) or a provider exposing nested
# virtualization. The default instance_type in variables.tf is m5.metal for
# this reason. Container-only (Tier 1) deployments can override with a
# regular instance type such as m5.large.
# ---------------------------------------------------------------------------

resource "aws_instance" "sandbox" {
  ami                    = var.ami_id
  instance_type          = var.instance_type
  subnet_id              = aws_subnet.private.id
  vpc_security_group_ids = [aws_security_group.sandbox.id]
  key_name               = var.ssh_key_name

  root_block_device {
    volume_type = "gp3"
    volume_size = var.root_volume_size
  }

  metadata_options {
    http_endpoint = "enabled"
    http_tokens   = "required"
  }

  tags = {
    Name = local.name_prefix
  }

  # Attach the data volume after the instance exists.
  lifecycle {
    ignore_changes = [tags]
  }
}

# EBS volume for the sandbox data directory (/var/lib/sandbox).
#
# The volume is created in the instance's availability zone, formatted, and
# mounted by scripts/bootstrap.sh at /var/lib/sandbox.
resource "aws_ebs_volume" "sandbox_data" {
  availability_zone = aws_instance.sandbox.availability_zone
  size              = var.data_volume_size
  type              = "gp3"

  tags = {
    Name = "${local.name_prefix}-data"
  }
}

resource "aws_volume_attachment" "sandbox_data" {
  device_name = "/dev/sdf"
  volume_id   = aws_ebs_volume.sandbox_data.id
  instance_id = aws_instance.sandbox.id

  # Allow replacement of the instance without failing on volume detach.
  force_detach = true
}
