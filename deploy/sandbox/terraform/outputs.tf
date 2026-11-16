output "sandbox_host_ip" {
  description = "Public IP address of the sandbox host (empty when the instance is in a private subnet without an EIP; SSH via a bastion or use sandbox_host_dns)."
  value       = aws_instance.sandbox.public_ip
}

output "sandbox_host_dns" {
  description = "Public DNS name of the sandbox host (empty when the instance has no public IP)."
  value       = aws_instance.sandbox.public_dns
}

output "s3_bucket_name" {
  description = "Name of the S3 bucket used for the CAS tier (empty when create_object_store = false)."
  value       = var.create_object_store ? aws_s3_bucket.sandbox_cas[0].bucket : ""
}

output "sandbox_base_url" {
  description = "Base URL of the sandbox API (http://<host>:8080)."
  value       = "http://${aws_instance.sandbox.public_ip}:8080"
}
