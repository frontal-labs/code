# Security Guide

This guide covers security aspects of the Frontal Code CLI, including permissions, sandboxing, data protection, and best practices.

## Security Overview

Frontal Code is designed with security as a primary concern, implementing multiple layers of protection:

- **Permission System** - Granular control over tool access
- **Sandboxing** - Isolated execution environments
- **Authentication** - Secure API key management
- **Data Protection** - Encryption and secure storage
- **Audit Logging** - Comprehensive activity tracking

## Server Authentication

`frontal-code-server` exposes a control plane that creates, cancels and completes agent
tasks. Both of its authentication boundaries are closed by default.

### Control plane

Set `FCODE_SERVER_API_KEY` before starting the server. Clients present it as
either `x-api-key: <key>` or `Authorization: Bearer <key>`; the comparison is
constant-time.

If the variable is unset, the server **refuses to start** rather than serving an
open control plane. To run without a key on a trusted network — local
development, or a host reachable only from inside your own perimeter — opt in
explicitly:

```bash
FCODE_SERVER_ALLOW_ANONYMOUS=1 frontal-code-server
```

That path logs a warning naming the bind address on every start.

### Integration webhooks

`POST /v1/webhooks/<source>` sits outside the control-plane auth layer, so its
HMAC signature is the only thing protecting it. Each source needs its own
secret, named `FCODE_<SOURCE>_WEBHOOK_SECRET`:

```bash
export FCODE_GITHUB_WEBHOOK_SECRET="..."
```

A delivery is rejected with `401` when the secret is missing or blank, when the
`x-<source>-signature` header is absent, or when the HMAC does not match. There
is no unsigned fallback. Source names are restricted to letters, digits, `-` and
`_`, so an unrecognised path returns `404` instead of probing your environment.

## Permission System

### Permission Modes

#### danger-full-access
- **Description**: All tools are allowed without confirmation
- **Use Case**: Trusted environments, automation scripts
- **Risk**: High - full system access
- **Configuration**: `--permission-mode danger-full-access`

```bash
# Use with caution in trusted environments
frontal-code --permission-mode danger-full-access prompt "deploy to production"
```

#### safe-mode
- **Description**: Only safe tools allowed, destructive tools require approval
- **Use Case**: Untrusted codebases, learning environments
- **Risk**: Medium - limited destructive capability
- **Configuration**: `--permission-mode safe-mode`

```bash
# Safe mode for untrusted projects
frontal-code --permission-mode safe-mode prompt "analyze this codebase"
```

#### ask-permissions
- **Description**: Prompt for approval on every tool use
- **Use Case**: Maximum security, learning, debugging
- **Risk**: Low - explicit approval required
- **Configuration**: `--permission-mode ask-permissions`

```bash
# Maximum security
frontal-code --permission-mode ask-permissions prompt "list files in /tmp"
```

### Tool Permissions

#### Safe Tools (always allowed)
- `read` - Read file contents
- `grep` - Search file contents
- `glob` - Search file patterns
- `web_search` - Search the web
- `web_fetch` - Fetch web content

#### Restricted Tools (require approval)
- `write` - Write/create files
- `edit` - Edit existing files
- `bash` - Execute shell commands
- `agent` - Launch sub-agents

#### Dangerous Tools (high risk)
- `bash` with system commands
- `write` to system directories
- `edit` configuration files
- `agent` with full access

### Permission Configuration

```json
{
  "permissions": {
    "mode": "safe-mode",
    "allowed_tools": ["read", "grep", "web_search"],
    "restricted_tools": ["write", "edit"],
    "blocked_tools": ["bash"],
    "tool_restrictions": {
      "bash": {
        "allowed_commands": ["ls", "cat", "grep"],
        "blocked_commands": ["rm", "sudo", "chmod", "chown"],
        "allowed_paths": ["/tmp", "/home/user/projects"],
        "blocked_paths": ["/etc", "/usr/bin", "/bin"]
      },
      "write": {
        "allowed_paths": ["/tmp", "./", "/home/user/projects"],
        "blocked_paths": ["/etc", "/usr", "/bin"],
        "max_file_size": "10MB"
      },
      "edit": {
        "allowed_extensions": [".txt", ".md", ".js", ".py"],
        "blocked_extensions": [".sh", ".conf", ".key"],
        "backup_enabled": true
      }
    },
    "time_restrictions": {
      "allowed_hours": "9-17",
      "allowed_days": "mon-fri",
      "timezone": "UTC"
    }
  }
}
```

## Sandboxing

### Process Sandboxing

Frontal Code isolates tool execution in separate processes:

```bash
# Enable sandbox mode
frontal-code --sandbox enable

# Configure sandbox limits
frontal-code --sandbox --cpu-limit 50% --memory-limit 1GB
```

### Sandbox Configuration

```json
{
  "sandbox": {
    "enabled": true,
    "limits": {
      "cpu": "50%",
      "memory": "1GB",
      "disk": "100MB",
      "network": "restricted",
      "processes": 10
    },
    "filesystem": {
      "read_only": ["/usr", "/lib", "/etc"],
      "read_write": ["/tmp", "./"],
      "hidden": ["/home/user/.ssh", "/etc/ssl"]
    },
    "network": {
      "allowed_hosts": ["api.anthropic.com", "github.com"],
      "blocked_hosts": ["malicious.example.com"],
      "allowed_ports": [443, 80],
      "blocked_ports": [22, 23, 3389]
    }
  }
}
```

### Container Sandboxing

For maximum isolation, use container-based sandboxing:

```bash
# Enable container sandbox
frontal-code --sandbox container

# Use a pinned container image
frontal-code --sandbox container --image frontal-code-sandbox:v0.1.0
```

## Authentication and API Keys

### API Key Management

#### Environment Variables (Recommended)

```bash
# Set API keys in environment
export FRONTAL_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export XAI_API_KEY="xai-..."

# Use with Frontal Code
frontal-code prompt "analyze this code"
```

#### Config File Storage

```json
{
  "providers": {
    "anthropic": {
       "api_key": "${FRONTAL_API_KEY}",
      "base_url": "https://api.anthropic.com"
    }
  }
}
```

#### Key Rotation

```bash
# Rotate API keys
frontal-code config rotate-api-keys anthropic

# Check key expiration
frontal-code config check-api-keys

# Set key expiration reminder
frontal-code config set key-expiry-reminder 7d
```

### API Security

#### Key Validation

```bash
# Validate API key
frontal-code auth validate anthropic

# Test API connectivity
frontal-code auth test anthropic

# Show key info (without exposing key)
frontal-code auth info anthropic
```

#### Rate Limiting

```json
{
  "rate_limiting": {
    "enabled": true,
    "requests_per_minute": 60,
    "tokens_per_minute": 100000,
    "burst_limit": 10,
    "backoff_strategy": "exponential"
  }
}
```

## Data Protection

### Encryption

#### Data at Rest

```json
{
  "encryption": {
    "enabled": true,
    "algorithm": "AES-256-GCM",
    "key_derivation": "PBKDF2",
    "iterations": 100000,
    "encrypt_sessions": true,
    "encrypt_config": false,
    "encrypt_cache": true
  }
}
```

#### Data in Transit

```json
{
  "tls": {
    "enabled": true,
    "version": "1.3",
    "cipher_suites": ["TLS_AES_256_GCM_SHA384"],
    "certificate_verification": true,
    "hsts": true
  }
}
```

### Sensitive Data Handling

#### Data Sanitization

```bash
# Enable data sanitization
frontal-code --sanitize-data prompt "process this file"

# Configure sanitization rules
frontal-code config set sanitize-patterns "password,token,key,secret"
```

#### Data Retention

```json
{
  "data_retention": {
    "sessions": "30d",
    "cache": "7d",
    "logs": "90d",
    "telemetry": "30d",
    "auto_cleanup": true
  }
}
```

### Privacy Settings

```json
{
  "privacy": {
    "disable_telemetry": true,
    "disable_usage_stats": true,
    "disable_error_reporting": false,
    "anonymize_data": true,
    "data_minimization": true
  }
}
```

## Audit and Logging

### Activity Logging

```json
{
  "logging": {
    "level": "info",
    "audit_log": {
      "enabled": true,
      "file": "~/.frontal-code/logs/audit.log",
      "format": "json",
      "rotation": "daily",
      "retention": "90d"
    },
    "events": [
      "tool_execution",
      "file_access",
      "network_request",
      "authentication",
      "permission_change",
      "config_change"
    ]
  }
}
```

### Security Events

```bash
# View security events
frontal-code audit security

# Show recent activity
frontal-code audit recent --hours 24

# Filter by event type
frontal-code audit filter --event tool_execution

# Export audit log
frontal-code audit export --format csv --output audit.csv
```

### Incident Response

```bash
# Lock down system on security event
frontal-code security lock

# Revoke all sessions
frontal-code security revoke-sessions

# Reset permissions to safe mode
frontal-code security reset-permissions

# Generate security report
frontal-code security report
```

## Network Security

### Network Restrictions

```json
{
  "network": {
    "allowed_hosts": [
      "api.anthropic.com",
      "api.openai.com",
      "api.x.ai",
      "github.com"
    ],
    "blocked_hosts": [
      "*.malicious.com",
      "phishing.example.com"
    ],
    "allowed_ports": [443, 80],
    "blocked_ports": [22, 23, 3389, 5432],
    "dns_servers": ["8.8.8.8", "1.1.1.1"],
    "proxy": {
      "enabled": false,
      "host": "",
      "port": 0,
      "auth": {
        "username": "",
        "password": ""
      }
    }
  }
}
```

### Certificate Validation

```json
{
  "certificates": {
    "validation": true,
    "custom_ca": [],
    "client_cert": {
      "enabled": false,
      "path": "",
      "key_path": ""
    },
    "ocsp_stapling": true,
    "certificate_pinning": false
  }
}
```

## Plugin Security

### Plugin Permissions

```json
{
  "plugins": {
    "permissions": {
      "default": "restricted",
      "require_explicit_approval": true,
      "sandbox_plugins": true,
      "signature_verification": true
    },
    "allowed_sources": [
      "https://github.com",
      "https://github.com/frontal-labs/frontal-code"
    ],
    "blocked_sources": [
      "*.malicious.com"
    ]
  }
}
```

### Plugin Sandboxing

```bash
# Enable plugin sandboxing
frontal-code config set plugin-sandbox true

# Configure plugin limits
frontal-code config set plugin-cpu-limit 25%
frontal-code config set plugin-memory-limit 512MB
```

### Plugin Verification

```bash
# Verify plugin signature
frontal-code plugin verify my-plugin

# Check plugin security
frontal-code plugin security-check my-plugin

# List trusted plugins
frontal-code plugin trusted
```

## Security Best Practices

### General Guidelines

1. **Use least privilege** - Grant minimum necessary permissions
2. **Regular updates** - Keep Frontal Code and dependencies updated
3. **Monitor activity** - Review audit logs regularly
4. **Secure storage** - Protect API keys and sensitive data
5. **Network security** - Restrict network access when possible

### Environment Security

```bash
# Use dedicated user account
useradd -m frontal-code
su - frontal-code

# Set restrictive file permissions
chmod 700 ~/.frontal-code
chmod 600 ~/.frontal-code/config.json

# Use secure shell
ssh -i ~/.ssh/frontal-code_key user@server
```

### Development Security

```bash
# Use safe mode for development
frontal-code --permission-mode safe-mode

# Enable audit logging
frontal-code --audit-log enable

# Use containerized development
docker run -it --rm frontal-code/cli:v0.1.0
```

### Production Security

```bash
# Use container sandbox
frontal-code --sandbox container

# Enable all security features
frontal-code --permission-mode ask-permissions --audit-log enable

# Monitor security events
frontal-code security monitor
```

## Security Configuration

### Security Hardening

```json
{
  "security": {
    "hardening": {
      "disable_debug_features": true,
      "disable_dev_tools": true,
      "enable_aslr": true,
      "enable_stack_protection": true,
      "disable_core_dumps": true
    },
    "intrusion_detection": {
      "enabled": true,
      "alert_threshold": 5,
      "block_threshold": 10,
      "alert_methods": ["email", "slack"]
    }
  }
}
```

### Compliance Settings

```json
{
  "compliance": {
    "standards": ["SOC2", "ISO27001", "GDPR"],
    "data_classification": "confidential",
    "audit_frequency": "daily",
    "retention_policy": "7y",
    "encryption_required": true
  }
}
```

## Threat Modeling

### Common Threats

1. **API Key Exposure** - Compromised authentication credentials
2. **Code Injection** - Malicious code execution through tools
3. **Data Exfiltration** - Unauthorized data access
4. **Privilege Escalation** - Gaining elevated system access
5. **Denial of Service** - Resource exhaustion attacks

### Mitigation Strategies

```json
{
  "threat_mitigation": {
    "api_key_exposure": {
      "rotation": "weekly",
      "encryption": true,
      "access_logging": true
    },
    "code_injection": {
      "input_validation": true,
      "sandboxing": true,
      "code_scanning": true
    },
    "data_exfiltration": {
      "egress_filtering": true,
      "data_loss_prevention": true,
      "access_controls": true
    }
  }
}
```

## Security Tools

### Built-in Security Tools

```bash
# Security scan
frontal-code security scan

# Vulnerability check
frontal-code security check-vulnerabilities

# Permission audit
frontal-code security audit-permissions

# Configuration security
frontal-code security check-config
```

### External Security Tools

```bash
# Integrate with security scanners
frontal-code security integrate --tool semgrep
frontal-code security integrate --tool trivy
frontal-code security integrate --tool bandit
```

## Incident Response

### Security Incident Types

1. **Unauthorized Access** - Suspicious login attempts
2. **Data Breach** - Unauthorized data access
3. **Malware Detection** - Suspicious code execution
4. **System Compromise** - System integrity issues

### Response Procedures

```bash
# Immediate response
frontal-code security incident --type unauthorized_access --action lock

# Investigation
frontal-code security investigate --incident-id 12345

# Recovery
frontal-code security recover --backup-id latest

# Post-incident review
frontal-code security review --incident-id 12345
```

## Security Updates

### Update Management

```bash
# Check for security updates
frontal-code security check-updates

# Apply security patches
frontal-code security update

# Verify update integrity
frontal-code security verify-update
```

### Security Advisories

```bash
# List security advisories
frontal-code security advisories

# Check specific vulnerability
frontal-code security advisory CVE-2024-12345

# Subscribe to security alerts
frontal-code security subscribe --email security@example.com
```

## Compliance and Auditing

### Compliance Reports

```bash
# Generate compliance report
frontal-code compliance report --standard SOC2

# Audit trail
frontal-code compliance audit-trail --start-date 2024-01-01

# Evidence collection
frontal-code compliance evidence --framework ISO27001
```

### Regulatory Compliance

```json
{
  "compliance": {
    "gdpr": {
      "data_processing": true,
      "consent_management": true,
      "data_subject_rights": true,
      "breach_notification": true
    },
    "soc2": {
      "security": true,
      "availability": true,
      "processing_integrity": true,
      "confidentiality": true,
      "privacy": true
    }
  }
}
```

This security guide provides comprehensive coverage of security features and best practices for using Frontal Code CLI safely in various environments.
