# Optional: Build and push Docker images using Terraform
# This is useful for CI/CD pipelines

resource "terraform_data" "build_frontal-code_server_image" {
  count = var.deploy_frontal-code_server ? 1 : 0

  triggers_replace = [
    var.frontal-code_server_image
  ]

  provisioner "local-exec" {
    command = <<-EOT
      echo "Building frontal-code-server image: ${var.frontal-code_server_image}"
      
      # Build the image
      docker build -f infrastructure/docker/frontal-code-server.Dockerfile -t ${var.frontal-code_server_image} .
      
      # Tag for ECR if needed
      if [[ "${var.frontal-code_server_image}" == *"amazonaws.com"* ]]; then
        # Extract ECR details
        ECR_REGISTRY=$(echo "${var.frontal-code_server_image}" | cut -d'/' -f1)
        REPOSITORY_NAME=$(echo "${var.frontal-code_server_image}" | cut -d'/' -f2 | cut -d':' -f1)
        
        # Login to ECR
        aws ecr get-login-password --region ${var.aws_region} | docker login --username AWS --password-stdin $ECR_REGISTRY
        
        # Create repository if it doesn't exist
        aws ecr describe-repositories --repository-names $REPOSITORY_NAME --region ${var.aws_region} || \
          aws ecr create-repository --repository-name $REPOSITORY_NAME --region ${var.aws_region}
        
        # Push the image
        docker push ${var.frontal-code_server_image}
      fi
    EOT

    working_dir = path.module
  }
}

resource "terraform_data" "build_frontal-code_slack_image" {
  count = var.deploy_frontal-code_slack ? 1 : 0

  triggers_replace = [
    var.frontal-code_slack_image
  ]

  provisioner "local-exec" {
    command = <<-EOT
      echo "Building frontal-code-slack image: ${var.frontal-code_slack_image}"
      
      # Build the image
      docker build -f infrastructure/docker/frontal-code-slack-app.Dockerfile -t ${var.frontal-code_slack_image} .
      
      # Tag for ECR if needed
      if [[ "${var.frontal-code_slack_image}" == *"amazonaws.com"* ]]; then
        # Extract ECR details
        ECR_REGISTRY=$(echo "${var.frontal-code_slack_image}" | cut -d'/' -f1)
        REPOSITORY_NAME=$(echo "${var.frontal-code_slack_image}" | cut -d'/' -f2 | cut -d':' -f1)
        
        # Login to ECR
        aws ecr get-login-password --region ${var.aws_region} | docker login --username AWS --password-stdin $ECR_REGISTRY
        
        # Create repository if it doesn't exist
        aws ecr describe-repositories --repository-names $REPOSITORY_NAME --region ${var.aws_region} || \
          aws ecr create-repository --repository-name $REPOSITORY_NAME --region ${var.aws_region}
        
        # Push the image
        docker push ${var.frontal-code_slack_image}
      fi
    EOT

    working_dir = path.module
  }
}

# ECR Repository resources (optional - for managed repositories)
resource "aws_ecr_repository" "frontal-code_server" {
  count                = var.deploy_frontal-code_server && length(regexall("amazonaws\\.com", var.frontal-code_server_image)) > 0 ? 1 : 0
  name                 = "frontal-code-server"
  image_tag_mutability = "IMMUTABLE"

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = {
    Environment = var.environment
    Project     = "frontal-code-tools"
  }
}

resource "aws_ecr_repository" "frontal-code_slack" {
  count                = var.deploy_frontal-code_slack && length(regexall("amazonaws\\.com", var.frontal-code_slack_image)) > 0 ? 1 : 0
  name                 = "frontal-code-slack"
  image_tag_mutability = "IMMUTABLE"

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = {
    Environment = var.environment
    Project     = "frontal-code-tools"
  }
}

# ECR Lifecycle policies
resource "aws_ecr_lifecycle_policy" "frontal-code_server" {
  count      = var.deploy_frontal-code_server && length(regexall("amazonaws\\.com", var.frontal-code_server_image)) > 0 ? 1 : 0
  repository = aws_ecr_repository.frontal-code_server[0].name

  policy = jsonencode({
    rules = [
      {
        rulePriority = 1
        description  = "Keep last 30 images"
        selection = {
          tagStatus     = "tagged"
          tagPrefixList = ["v"]
          countType     = "imageCountMoreThan"
          countNumber   = 30
        }
        action = {
          type = "expire"
        }
      },
      {
        rulePriority = 2
        description  = "Keep last 10 untagged images"
        selection = {
          tagStatus   = "untagged"
          countType   = "imageCountMoreThan"
          countNumber = 10
        }
        action = {
          type = "expire"
        }
      }
    ]
  })
}

resource "aws_ecr_lifecycle_policy" "frontal-code_slack" {
  count      = var.deploy_frontal-code_slack && length(regexall("amazonaws\\.com", var.frontal-code_slack_image)) > 0 ? 1 : 0
  repository = aws_ecr_repository.frontal-code_slack[0].name

  policy = jsonencode({
    rules = [
      {
        rulePriority = 1
        description  = "Keep last 30 images"
        selection = {
          tagStatus     = "tagged"
          tagPrefixList = ["v"]
          countType     = "imageCountMoreThan"
          countNumber   = 30
        }
        action = {
          type = "expire"
        }
      },
      {
        rulePriority = 2
        description  = "Keep last 10 untagged images"
        selection = {
          tagStatus   = "untagged"
          countType   = "imageCountMoreThan"
          countNumber = 10
        }
        action = {
          type = "expire"
        }
      }
    ]
  })
}
