variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "us-east-1"
}

variable "environment" {
  description = "Environment name"
  type        = string
  default     = "development"
}

variable "aws_account_id" {
  description = "AWS account ID for deployment"
  type        = string
}

variable "eks_cluster_name" {
  description = "EKS cluster name"
  type        = string
}

variable "eks_cluster_auth_name" {
  description = "EKS cluster auth name"
  type        = string
}

variable "certificate_arn" {
  description = "ACM certificate ARN for tools.frontal.dev"
  type        = string
}

variable "vpc_id" {
  description = "VPC ID for the EKS cluster"
  type        = string
}

variable "frontal-code_service_namespace" {
  description = "Kubernetes namespace where frontal-code-server is deployed"
  type        = string
  default     = "frontal-code"
}

variable "deploy_frontal-code_server" {
  description = "Whether to deploy the frontal-code-server"
  type        = bool
  default     = true
}

variable "deploy_frontal-code_slack" {
  description = "Whether to deploy the frontal-code-slack extension"
  type        = bool
  default     = true
}

variable "frontal-code_server_image" {
  description = "Docker image for frontal-code-server"
  type        = string
  default     = "frontal-code-server:v0.1.0"
}

variable "frontal-code_slack_image" {
  description = "Docker image for frontal-code-slack"
  type        = string
  default     = "frontal-code-slack:v0.1.0"
}

variable "frontal-code_server_api_key" {
  description = "Shared API key used by frontal-code-server control-plane routes and frontal-code-slack"
  type        = string
  sensitive   = true
}

variable "slack_bot_token" {
  description = "Slack bot token"
  type        = string
  sensitive   = true
}

variable "slack_app_token" {
  description = "Slack app token"
  type        = string
  sensitive   = true
}

variable "slack_signing_secret" {
  description = "Slack signing secret"
  type        = string
  sensitive   = true
}

variable "github_token" {
  description = "GitHub token for Slack integration"
  type        = string
  sensitive   = true
  default     = ""
}

variable "sentry_dsn" {
  description = "Sentry DSN for error tracking"
  type        = string
  sensitive   = true
  default     = ""
}

variable "api_keys" {
  description = "API keys for various providers"
  type = object({
    anthropic = optional(string, "")
    openai    = optional(string, "")
    xai       = optional(string, "")
    azure     = optional(string, "")
    bedrock   = optional(string, "")
    ollama    = optional(string, "")
  })
  sensitive = true
  default   = {}
}

variable "storage_class" {
  description = "Storage class for persistent volumes"
  type        = string
  default     = "gp3"
}

variable "workspace_storage_size" {
  description = "Size of workspace storage"
  type        = string
  default     = "100Gi"
}

variable "server_state_storage_size" {
  description = "Size of server state storage"
  type        = string
  default     = "10Gi"
}

variable "agent_store_storage_size" {
  description = "Size of agent store storage"
  type        = string
  default     = "50Gi"
}
