# ---------------------------------------------------------------------------
# S3 object store for the CAS tier.
#
# The sandbox server stores session artifacts (snapshots, logs, shared files)
# in an S3-compatible object store. The bucket is versioned and private.
#
# The systemd unit receives the bucket name and endpoint via
# SANDBOX_CAS_S3_BUCKET / SANDBOX_CAS_S3_ENDPOINT (see bootstrap.sh).
# ---------------------------------------------------------------------------

resource "aws_s3_bucket" "sandbox_cas" {
  count = var.create_object_store ? 1 : 0

  bucket = "${var.environment}-frontal-sandbox-cas-${replace(var.aws_region, "-", "")}"

  tags = {
    Name = "${local.name_prefix}-cas"
  }
}

resource "aws_s3_bucket_versioning" "sandbox_cas" {
  count = var.create_object_store ? 1 : 0

  bucket = aws_s3_bucket.sandbox_cas[0].id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "sandbox_cas" {
  count = var.create_object_store ? 1 : 0

  bucket = aws_s3_bucket.sandbox_cas[0].id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "aws:kms"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "sandbox_cas" {
  count = var.create_object_store ? 1 : 0

  bucket = aws_s3_bucket.sandbox_cas[0].id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
