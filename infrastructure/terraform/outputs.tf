output "tools_domain_url" {
  description = "URL for the tools frontend"
  value       = "https://tools.frontal.dev"
}

output "frontal-code_tools_url" {
  description = "URL for Frontal Code tools"
  value       = "https://tools.frontal.dev/frontal-code"
}

output "frontal-code_server_url" {
  description = "Internal URL for frontal-code-server"
  value       = var.deploy_frontal-code_server ? "http://frontal-code-server:8788" : null
}

output "frontal-code_slack_url" {
  description = "Internal URL for frontal-code-slack"
  value       = var.deploy_frontal-code_slack ? "http://frontal-code-slack:3000" : null
}

output "nginx_ingress_load_balancer" {
  description = "NLB DNS name for the NGINX ingress"
  value       = data.kubernetes_service.nginx_ingress.status.0.load_balancer.0.ingress.0.hostname
}

output "route53_record_name" {
  description = "Route53 record name"
  value       = aws_route53_record.tools_frontal_dev.name
}

output "frontal-code_server_deployment" {
  description = "Frontal Code server deployment status"
  value       = var.deploy_frontal-code_server ? "Deployed with ${length(kubernetes_deployment.frontal-code_server[0].spec.replica)} replicas" : "Not deployed"
}

output "frontal-code_slack_deployment" {
  description = "Frontal Code Slack deployment status"
  value       = var.deploy_frontal-code_slack ? "Deployed with ${length(kubernetes_deployment.frontal-code_slack[0].spec.replica)} replicas" : "Not deployed"
}

output "storage_info" {
  description = "Storage configuration"
  value = var.deploy_frontal-code_server ? {
    workspace_size    = var.workspace_storage_size
    server_state_size = var.server_state_storage_size
    agent_store_size  = var.agent_store_storage_size
    storage_class     = var.storage_class
  } : null
}

output "environment_info" {
  description = "Environment configuration"
  value = {
    environment    = var.environment
    aws_region     = var.aws_region
    aws_account_id = var.aws_account_id
    namespace      = var.frontal-code_service_namespace
  }
}

output "ecr_repositories" {
  description = "ECR repositories created"
  value = {
    frontal-code_server = var.deploy_frontal-code_server && length(regexall("amazonaws\\.com", var.frontal-code_server_image)) > 0 ? aws_ecr_repository.frontal-code_server[0].repository_url : null
    frontal-code_slack  = var.deploy_frontal-code_slack && length(regexall("amazonaws\\.com", var.frontal-code_slack_image)) > 0 ? aws_ecr_repository.frontal-code_slack[0].repository_url : null
  }
}
