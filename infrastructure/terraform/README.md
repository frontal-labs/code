# Terraform Infrastructure for tools.frontal.dev

This Terraform configuration sets up the complete AWS infrastructure for hosting Frontal Code CLI, Slack extension, and other internal tools at `tools.frontal.dev`. Designed specifically for deployment in the development account to avoid conflicts with production.

## Architecture

- **NLB**: Network Load Balancer for high-performance HTTP/HTTPS traffic
- **NGINX Ingress Controller**: Path-based routing to different services
- **Cert-Manager**: Automatic SSL certificate management with Let's Encrypt
- **Route53**: DNS management for `tools.frontal.dev`
- **Kubernetes**: Complete service deployment (frontal-code-server, frontal-code-slack, proxy)
- **ECR**: Optional container registry for custom images
- **EBS**: Persistent storage for workspace, server state, and agent store

## Deployed Components

### Core Services
- **frontal-code-server**: Rust-based control plane (2 replicas, 2Gi RAM each)
- **frontal-code-slack**: Node.js Slack integration (2 replicas, 512Mi RAM each)
- **tools-proxy**: NGINX reverse proxy with SSL termination

### Storage
- **Workspace**: 100Gi for code repositories and workspace
- **Server State**: 10Gi for frontal-code-server state
- **Agent Store**: 50Gi for hosted agent artifacts

### Networking
- **tools.frontal.dev**: Main domain with SSL
- **/frontal-code**: Frontal Code API endpoints
- **/frontal-code/webhooks**: Slack and other webhooks

## Prerequisites

1. **AWS CLI** configured with development account permissions
2. **Terraform** >= 1.0
3. **kubectl** configured to access the development EKS cluster
4. **jq** for JSON parsing in deployment scripts
5. **Docker** (if building custom images)
6. **Development EKS cluster** 
7. **Route53 hosted zone** for `frontal.dev`
8. **ACM certificate** for `tools.frontal.dev` in the development account

## Setup Instructions

### 1. Configure Variables

Copy the example variables file:

```bash
cp terraform.tfvars.example terraform.tfvars
```

Edit `terraform.tfvars` with your development account values:

```hcl
# AWS Configuration
aws_region = "us-east-1"
environment = "development"
aws_account_id = "123456789012"  # Development account ID

# EKS Cluster
eks_cluster_name = "frontal-code-dev-cluster"
eks_cluster_auth_name = "frontal-code-dev-cluster"
vpc_id = "vpc-xxxxxxxxx"

# SSL Certificate
certificate_arn = "arn:aws:acm:us-east-1:123456789012:certificate/xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"

# Deployment Options
deploy_frontal-code_server = true
deploy_frontal-code_slack = true

# Container Images
frontal-code_server_image = "frontal-code-server:v0.1.0"
frontal-code_slack_image = "frontal-code-slack:v0.1.0"
frontal-code_server_api_key = "replace-with-a-long-random-api-key"

# Slack Configuration
slack_bot_token = "xoxb-xxxxxxxxxxxx"
slack_app_token = "xapp-xxxxxxxxxxxx"
slack_signing_secret = "xxxxxxxxxxxxxxxx"

# API Keys (optional)
api_keys = {
  anthropic = "sk-ant-xxxxxxxxxxxx"
  openai = "sk-xxxxxxxxxxxx"
  # ... other providers
}
```

### 2. Deploy Using Script (Recommended)

Use the automated deployment script:

```bash
cd infrastructure
./deploy.sh
```

The script will:
- Check prerequisites
- Initialize Terraform
- Show the deployment plan
- Apply changes with verification
- Display deployment results

### 3. Manual Deployment

If you prefer manual deployment:

```bash
cd infrastructure/terraform

# Initialize Terraform
terraform init

# Review the plan
terraform plan

# Apply the changes
terraform apply
```

### 4. Verify Deployment

After deployment, verify the complete stack:

```bash
# Check all services in frontal-code namespace
kubectl get pods -n frontal-code
kubectl get services -n frontal-code
kubectl get ingress -n frontal-code

# Check the NLB
kubectl get svc -n ingress-nginx

# Check storage
kubectl get pvc -n frontal-code

# Check logs
kubectl logs -n frontal-code -l app=frontal-code-server
kubectl logs -n frontal-code -l app=frontal-code-slack
kubectl logs -n frontal-code -l app=tools-proxy
```

## URL Structure

Once deployed, the following URLs will be available:

- **Tools Landing**: `https://tools.frontal.dev`
- **Frontal Code API**: `https://tools.frontal.dev/frontal-code/`
- **Frontal Code Tasks**: `https://tools.frontal.dev/frontal-code/api/v1/tasks/`
- **Webhooks**: `https://tools.frontal.dev/frontal-code/webhooks/`

## Environment Variables for Frontal Code CLI

Update your Frontal Code CLI configuration to use the new URLs:

```bash
export FRONTAL_BASE_URL="https://tools.frontal.dev/frontal-code"
export FRONTAL_CODE_HOSTED_CALLBACK_URL="https://tools.frontal.dev/frontal-code/webhooks/tasks"
```

## Security Features

- **HTTPS enforced** with automatic redirects
- **Rate limiting** on API endpoints
- **Security headers** (HSTS, XSS protection, etc.)
- **SSL certificates** auto-renewed by cert-manager
- **Network isolation** within EKS VPC

## Monitoring and Logging

- NGINX access and error logs are collected
- Health checks on all services
- Prometheus metrics available from NGINX ingress

## Troubleshooting

### SSL Certificate Issues

```bash
# Check certificate status
kubectl describe certificate tools-frontal-dev -n frontal-code

# Check cert-manager logs
kubectl logs -n cert-manager deployment/cert-manager
```

### Ingress Issues

```bash
# Check ingress status
kubectl describe ingress frontal-code-tools-ingress -n frontal-code

# Check NGINX ingress logs
kubectl logs -n ingress-nginx deployment/nginx-ingress-controller
```

### DNS Issues

```bash
# Check Route53 record
nslookup tools.frontal.dev

# Check NLB DNS name
kubectl get svc -n ingress-nginx nginx-ingress-controller-controller
```

## Maintenance

### Updating SSL Certificates

Certificates are automatically renewed by cert-manager. No manual intervention required.

### Scaling NGINX Ingress

Modify the replica count in `nginx-ingress.tf`:

```hcl
controller = {
  replicaCount = 4  # Increase from 2
}
```

### Adding New Tools

To add new tools under `tools.frontal.dev`:

1. Deploy the service in Kubernetes
2. Add a new location block in `proxy/nginx.conf`
3. Update the Terraform configuration
4. Apply changes

## Cleanup

To destroy all created resources:

```bash
terraform destroy
```

**Note**: This will remove the NLB, Route53 records, and all Kubernetes resources created by this configuration.
