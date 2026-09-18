# Containers Guide

This guide covers containerization and deployment of Frontal Code CLI using Docker, Kubernetes, and other container technologies.

## Table of Contents

- [Overview](#overview)
- [Docker Images](#docker-images)
- [Docker Compose](#docker-compose)
- [Kubernetes Deployment](#kubernetes-deployment)
- [Container Security](#container-security)
- [Performance Optimization](#performance-optimization)
- [Monitoring and Logging](#monitoring-and-logging)
- [CI/CD Integration](#cicd-integration)
- [Troubleshooting](#troubleshooting)

## Overview

Frontal Code CLI provides official container images for easy deployment and scaling. Containerization offers:

- **Consistency**: Same environment across development, staging, and production
- **Portability**: Run on any platform supporting containers
- **Scalability**: Easy horizontal scaling with orchestration
- **Isolation**: Security and dependency isolation
- **Versioning**: Immutable deployments with version control

### Container Benefits

- **Rapid Deployment**: Spin up new instances in seconds
- **Resource Efficiency**: Shared kernel and optimized resource usage
- **DevOps Integration**: Fits into modern CI/CD pipelines
- **Rollback Capability**: Easy rollback to previous versions
- **Environment Parity**: Eliminate "it works on my machine" issues

## Docker Images

### Official Images

Frontal Code provides official Docker images on multiple registries:

Pin an explicit release tag or digest in production. Avoid mutable aliases like `latest`.

#### Docker Hub

```bash
# Pinned release
docker pull frontal-code/cli:v0.1.0

# Specific version
docker pull frontal-code/cli:v0.1.0

# Alpine variant (smaller size)
docker pull frontal-code/cli:alpine

# Development version
docker pull frontal-code/cli:dev
```

#### GitHub Container Registry

```bash
# Pinned release
docker pull ghcr.io/frontal-code-org/cli:v0.1.0

# Specific version
docker pull ghcr.io/frontal-code-org/cli:v0.1.0
```

#### Amazon ECR

```bash
# Public ECR
docker pull public.ecr.aws/frontal-code/cli:v0.1.0
```

### Image Variants

| Variant | Description | Size | Use Case |
|---------|-------------|--------|----------|
| `vX.Y.Z` | Pinned release tag | General purpose |
| `alpine` | Alpine Linux-based | Minimal size, security-focused |
| `slim` | Slimmed-down image | Reduced attack surface |
| `dev` | Development build with tools | Development and debugging |

### Image Layers

```
frontal-code/cli:v0.1.0
├── rust:1.75-alpine          # Base runtime
├── ca-certificates             # SSL certificates
├── frontal-code-cli-binary           # Compiled Frontal Code binary
├── configuration-templates     # Default config files
├── plugins                   # Built-in plugins
└── entrypoint-scripts        # Startup and health scripts
```

### Building Custom Images

#### Dockerfile

```dockerfile
# Multi-stage build for smaller final image
FROM rust:1.75-alpine AS builder

# Install dependencies
RUN apk add --no-cache musl-dev

# Build Frontal Code
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

# Final runtime image
FROM alpine:3.20

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    curl \
    bash

# Create non-root user
RUN addgroup -g 1000 frontal-code && \
    adduser -D -s /bin/sh -u 1000 -G frontal-code frontal-code

# Copy binary and set permissions
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/frontal-code /usr/local/bin/
RUN chmod +x /usr/local/bin/frontal-code

# Set up directories
RUN mkdir -p /home/frontal-code/.frontal-code && \
    chown -R frontal-code:frontal-code /home/frontal-code

# Switch to non-root user
USER frontal-code

# Set environment
ENV FCODE_DATA_DIR=/home/frontal-code/.frontal-code
ENV PATH=/usr/local/bin:$PATH

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD frontal-code status || exit 1

# Entry point
ENTRYPOINT ["frontal-code"]
CMD ["--help"]
```

#### Build and Push

```bash
# Build image
docker build -t frontal-code/cli:custom .

# Tag for registry
docker tag frontal-code/cli:custom ghcr.io/frontal-code-org/cli:custom

# Push to registry
docker push ghcr.io/frontal-code-org/cli:custom
```

### Multi-Architecture Builds

```dockerfile
# Build for multiple architectures
FROM --platform=linux/amd64 rust:1.75-alpine AS builder-amd64
FROM --platform=linux/arm64 rust:1.75-alpine AS builder-arm64

# Build steps...

# Final image with multiple architectures
FROM --platform=linux/amd64 alpine:3.20
# ... copy from builder-amd64

# Create manifest
docker manifest create frontal-code/cli:multiarch \
    frontal-code/cli:amd64 \
    frontal-code/cli:arm64

docker manifest push frontal-code/cli:multiarch
```

## Docker Compose

### Development Environment

```yaml
# docker-compose.dev.yml
version: '3.8'

services:
  frontal-code:
    build:
      context: .
      dockerfile: infrastructure/docker/frontal-code-dev.Dockerfile
    ports:
      - "8080:8080"
    volumes:
      - ./:/app
      - frontal-code-data:/home/frontal-code/.frontal-code
    environment:
      - FCODE_LOG_LEVEL=debug
      - FRONTAL_API_KEY=${FRONTAL_API_KEY}
      - FCODE_DEFAULT_MODEL=claude-sonnet-4-6
    working_dir: /app
    command: repl
    restart: unless-stopped

volumes:
  frontal-code-data:
    driver: local
```

Only mount `/var/run/docker.sock` when the container must launch sibling Docker workers.
Keep it off the default development path and add it explicitly for trusted local-docker scenarios.

### Production Environment

```yaml
# docker-compose.prod.yml
version: '3.8'

services:
  frontal-code:
    image: frontal-code/cli:v0.1.0
    deploy:
      replicas: 3
      resources:
        limits:
          cpus: '1.0'
          memory: 1G
        reservations:
          cpus: '0.5'
          memory: 512M
      restart_policy:
        condition: on-failure
        delay: 5s
        max_attempts: 3
    environment:
      - FCODE_LOG_LEVEL=info
      - FRONTAL_API_KEY=${FRONTAL_API_KEY}
      - FCODE_PERMISSION_MODE=safe-mode
    volumes:
      - frontal-code-config:/home/frontal-code/.frontal-code
      - frontal-code-sessions:/home/frontal-code/.frontal-code/sessions
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
    healthcheck:
      test: ["CMD", "frontal-code", "status"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  redis:
    image: redis:7-alpine
    deploy:
      resources:
        limits:
          cpus: '0.5'
          memory: 256M
    volumes:
      - redis-data:/data
    command: redis-server --appendonly yes

volumes:
  frontal-code-config:
    driver: local
  frontal-code-sessions:
    driver: local
  redis-data:
    driver: local
```

### Monitoring Stack

```yaml
# docker-compose.monitoring.yml
version: '3.8'

services:
  frontal-code:
    image: frontal-code/cli:v0.1.0
    environment:
      - FCODE_TELEMETRY_ENABLED=true
      - FCODE_TELEMETRY_ENDPOINT=http://prometheus:9090/metrics
    depends_on:
      - prometheus
      - grafana

  prometheus:
    image: prom/prometheus:vX.Y.Z
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.console.libraries=/etc/prometheus/console_libraries'
      - '--web.console.templates=/etc/prometheus/consoles'

  grafana:
    image: grafana/grafana:X.Y.Z
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - grafana-data:/var/lib/grafana
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards

volumes:
  prometheus-data:
  grafana-data:
```

## Kubernetes Deployment

### Namespace and RBAC

```yaml
# namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: frontal-code
  labels:
    name: frontal-code

---
# rbac.yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: frontal-code-sa
  namespace: frontal-code

---
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: frontal-code-role
  namespace: frontal-code
rules:
- apiGroups: [""]
  resources: ["pods", "services", "configmaps"]
  verbs: ["get", "list", "watch"]

---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: frontal-code-rolebinding
  namespace: frontal-code
subjects:
- kind: ServiceAccount
  name: frontal-code-sa
  namespace: frontal-code
roleRef:
  kind: Role
  name: frontal-code-role
```

### Deployment Configuration

```yaml
# deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cli
  namespace: frontal-code
  labels:
    app: cli
    version: v1.0.0
spec:
  replicas: 3
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app: cli
  template:
    metadata:
      labels:
        app: cli
        version: v1.0.0
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "8080"
        prometheus.io/path: "/metrics"
    spec:
      serviceAccountName: frontal-code-sa
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        runAsGroup: 1000
        fsGroup: 1000
      containers:
      - name: cli
        image: frontal-code/cli:v0.1.0
        imagePullPolicy: Always
        ports:
        - name: http
          containerPort: 8080
          protocol: TCP
        env:
        - name: FCODE_LOG_LEVEL
          value: "info"
        - name: FRONTAL_API_KEY
          valueFrom:
            secretKeyRef:
              name: frontal-code-secrets
              key: anthropic-api-key
        - name: FCODE_DEFAULT_MODEL
          value: "claude-sonnet-4-6"
        - name: FCODE_PERMISSION_MODE
          value: "safe-mode"
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "1Gi"
            cpu: "1000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 3
        volumeMounts:
        - name: frontal-code-config
          mountPath: /home/frontal-code/.frontal-code
          readOnly: false
        - name: frontal-code-sessions
          mountPath: /home/frontal-code/.frontal-code/sessions
          readOnly: false
      volumes:
      - name: frontal-code-config
        persistentVolumeClaim:
          claimName: frontal-code-config-pvc
      - name: frontal-code-sessions
        persistentVolumeClaim:
          claimName: frontal-code-sessions-pvc
      restartPolicy: Always
      terminationGracePeriodSeconds: 30
```

### Service Configuration

```yaml
# service.yaml
apiVersion: v1
kind: Service
metadata:
  name: frontal-code-service
  namespace: frontal-code
  labels:
    app: cli
spec:
  type: ClusterIP
  ports:
  - name: http
    port: 80
    targetPort: 8080
    protocol: TCP
  selector:
    app: cli

---
# ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: frontal-code-ingress
  namespace: frontal-code
  annotations:
    kubernetes.io/ingress.class: nginx
    cert-manager.io/cluster-issuer: letsencrypt-prod
    nginx.ingress.kubernetes.io/rewrite-target: /
spec:
  tls:
  - hosts:
    - frontal-code.example.com
    secretName: frontal-code-tls
  rules:
  - host: frontal-code.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: frontal-code-service
            port:
              number: 80
```

### Persistent Storage

```yaml
# pvc.yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: frontal-code-config-pvc
  namespace: frontal-code
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 1Gi
  storageClassName: fast-ssd

---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: frontal-code-sessions-pvc
  namespace: frontal-code
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 5Gi
  storageClassName: fast-ssd
```

### Horizontal Pod Autoscaler

```yaml
# hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: frontal-code-hpa
  namespace: frontal-code
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: cli
  minReplicas: 2
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
  behavior:
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
      - type: Percent
        value: 10
        periodSeconds: 60
    scaleUp:
      stabilizationWindowSeconds: 60
      policies:
      - type: Percent
        value: 50
        periodSeconds: 60
```

## Container Security

### Security Best Practices

#### Non-Root User

```dockerfile
# Create non-root user
RUN addgroup -g 1000 frontal-code && \
    adduser -D -s /bin/sh -u 1000 -G frontal-code frontal-code

# Use non-root user
USER frontal-code
```

#### Read-Only Filesystem

```dockerfile
# Copy as read-only
COPY --chown=frontal-code:frontal-code . /app
RUN chmod -R 755 /app

# Mount read-only where possible
VOLUME ["/home/frontal-code/.frontal-code:rw"]
```

#### Minimal Attack Surface

```dockerfile
# Use Alpine for minimal base
FROM alpine:3.20

# Install only required packages
RUN apk add --no-cache \
    ca-certificates \
    curl \
    && rm -rf /var/cache/apk/*

# Remove unnecessary tools
RUN rm -rf /usr/bin/wget \
    /usr/bin/curl \
    /bin/sh
```

### Security Context

```yaml
securityContext:
  runAsNonRoot: true
  runAsUser: 1000
  runAsGroup: 1000
  fsGroup: 1000
  capabilities:
    drop:
    - ALL
  readOnlyRootFilesystem: false
  allowPrivilegeEscalation: false
```

### Pod Security Policies

```yaml
# pod-security-policy.yaml
apiVersion: policy/v1beta1
kind: PodSecurityPolicy
metadata:
  name: frontal-code-psp
spec:
  privileged: false
  allowPrivilegeEscalation: false
  requiredDropCapabilities:
    - ALL
  volumes:
    - 'configMap'
    - 'emptyDir'
    - 'projected'
    - 'secret'
    - 'downwardAPI'
    - 'persistentVolumeClaim'
  runAsUser:
    rule: 'MustRunAsNonRoot'
  seLinux:
    rule: 'RunAsAny'
  fsGroup:
    rule: 'RunAsAny'
```

### Secrets Management

```yaml
# secrets.yaml
apiVersion: v1
kind: Secret
metadata:
  name: frontal-code-secrets
  namespace: frontal-code
type: Opaque
data:
  anthropic-api-key: <base64-encoded-key>
  openai-api-key: <base64-encoded-key>
  xai-api-key: <base64-encoded-key>

---
# sealed-secrets.yaml (using Sealed Secrets)
apiVersion: bitnami.com/v1alpha1
kind: SealedSecret
metadata:
  name: frontal-code-secrets
  namespace: frontal-code
spec:
  encryptedData:
    anthropic-api-key: <encrypted-key>
    openai-api-key: <encrypted-key>
    xai-api-key: <encrypted-key>
```

## Performance Optimization

### Resource Limits

```yaml
resources:
  requests:
    memory: "512Mi"
    cpu: "500m"
  limits:
    memory: "1Gi"
    cpu: "1000m"
```

### Caching Strategy

```yaml
# Redis cache sidecar
- name: redis-cache
  image: redis:7-alpine
  resources:
    limits:
      memory: "256Mi"
      cpu: "250m"
  env:
    - name: REDIS_MAXMEMORY
      value: "200mb"
```

### Connection Pooling

```yaml
env:
  - name: FCODE_CONNECTION_POOL_SIZE
    value: "10"
  - name: FCODE_CONNECTION_TIMEOUT
    value: "30"
  - name: FCODE_KEEP_ALIVE
    value: "true"
```

### Image Optimization

```dockerfile
# Multi-stage build for smaller images
FROM rust:1.75-alpine AS builder
# ... build steps ...

FROM scratch
COPY --from=builder /app/target/release/frontal-code /frontal-code
# No additional layers for minimal size
```

## Monitoring and Logging

### Health Checks

```dockerfile
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD frontal-code status --json || exit 1
```

### Metrics Collection

```yaml
# prometheus-config.yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'frontal-code'
    static_configs:
      - targets: ['frontal-code:8080']
    metrics_path: /metrics
    scrape_interval: 5s
```

### Log Aggregation

```yaml
# fluentd-config.yaml
<source>
  @type tail
  path /var/log/containers/*.log
  pos_file /var/log/fluentd-containers.log.pos
  tag kubernetes.*
  read_from_head true
  <parse>
    @type json
    time_format %Y-%m-%dT%H:%M:%S.%NZ
  </parse>
</source>

<match kubernetes.**>
  @type elasticsearch
  host elasticsearch
  port 9200
  index_name frontal-code-logs
  type_name _doc
</match>
```

### Distributed Tracing

```yaml
# jaeger-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: jaeger
spec:
  template:
    spec:
      containers:
      - name: jaeger
        image: jaegertracing/all-in-one:X.Y.Z
        ports:
        - containerPort: 16686
          name: ui
        - containerPort: 14268
          name: collector
        env:
        - name: COLLECTOR_ZIPKIN_HTTP_PORT
          value: "9411"
        - name: SPAN_STORAGE_TYPE
          value: "elasticsearch"
        - name: ES_SERVER_URLS
          value: "http://elasticsearch:9200"
```

## CI/CD Integration

### GitHub Actions

```yaml
# .github/workflows/docker.yml
name: Build and Deploy Docker

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
    - uses: actions/checkout@v3
    - name: Set up Docker Buildx
      uses: docker/setup-buildx-action@v2
    - name: Build Docker image
      run: |
        docker buildx build \
          --platform linux/amd64,linux/arm64 \
          --tag frontal-code/cli:${{ github.sha }} \
          --load .
    - name: Test Docker image
      run: |
        docker run --rm frontal-code/cli:${{ github.sha }} --version

  build-and-push:
    needs: test
    runs-on: ubuntu-24.04
    if: github.ref == 'refs/heads/main'
    steps:
    - uses: actions/checkout@v3
    - name: Set up Docker Buildx
      uses: docker/setup-buildx-action@v2
    - name: Login to Container Registry
      uses: docker/login-action@v2
      with:
        registry: ghcr.io
        username: ${{ github.actor }}
        password: ${{ secrets.GITHUB_TOKEN }}
    - name: Build and push Docker image
      run: |
        docker buildx build \
          --platform linux/amd64,linux/arm64 \
          --tag ghcr.io/frontal-code-org/cli:${{ github.sha }} \
          --push .
```

### GitLab CI

```yaml
# .gitlab-ci.yml
stages:
  - test
  - build
  - deploy

variables:
  DOCKER_DRIVER: overlay2
  DOCKER_TLS_CERTDIR: "/certs"

services:
  - docker:dind

test:
  stage: test
  script:
    - docker build -t frontal-code/cli:test .
    - docker run --rm frontal-code/cli:test --version

build:
  stage: build
  script:
    - docker build -t $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA .
    - docker push $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA
  only:
    - main

deploy:
  stage: deploy
  script:
    - kubectl set image deployment/cli frontal-code=$CI_REGISTRY_IMAGE:$CI_COMMIT_SHA
    - kubectl rollout status deployment/cli
  only:
    - main
```

### ArgoCD

```yaml
# argocd-application.yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: cli
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/frontal-code-org/frontal-code-k8s.git
    targetRevision: HEAD
    path: manifests
  destination:
    server: https://kubernetes.default.svc
    namespace: frontal-code
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
    syncOptions:
    - CreateNamespace=true
  retry:
    limit: 5
    backoff:
      duration: 5s
      factor: 2
      maxDuration: 3m
```

## Troubleshooting

### Common Issues

#### Container Won't Start

```bash
# Check logs
docker logs frontal-code-container

# Check health status
docker inspect frontal-code-container --format='{{.State.Health.Status}}'

# Debug with interactive shell
docker run -it --entrypoint /bin/sh frontal-code/cli:v0.1.0
```

#### Permission Issues

```bash
# Check user permissions
docker run frontal-code/cli:v0.1.0 id

# Fix volume permissions
docker run --user 1000:1000 -v $(pwd):/app frontal-code/cli:v0.1.0

# Use security context
docker run --security-opt no-new-privileges frontal-code/cli:v0.1.0
```

#### Resource Issues

```bash
# Monitor resource usage
docker stats frontal-code-container

# Check limits
docker inspect frontal-code-container --format='{{.HostConfig.Resources}}'

# Adjust limits
docker update --memory=2g --cpus=2 frontal-code-container
```

### Debugging Tools

```bash
# Enter running container
docker exec -it frontal-code-container /bin/sh

# Monitor network traffic
docker run --network container:frontal-code-container nicolaka/netshoot

# Check filesystem
docker run --volumes-from frontal-code-container busybox ls -la /home/frontal-code/.frontal-code
```

### Performance Debugging

```bash
# Profile with perf
docker run --privileged -v /usr/local/bin/perf:/usr/local/bin/perf frontal-code/cli:v0.1.0

# Memory profiling
docker run --memory=512m --memory-swap=512m frontal-code/cli:v0.1.0

# CPU profiling
docker run --cpus=0.5 frontal-code/cli:v0.1.0
```

This containers guide provides comprehensive coverage of deploying and managing Frontal Code CLI in containerized environments.
