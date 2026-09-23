# Deploy frontal-code-server if enabled
resource "kubernetes_namespace" "frontal-code" {
  count = var.deploy_frontal-code_server || var.deploy_frontal-code_slack ? 1 : 0
  metadata {
    name = var.frontal-code_service_namespace
    labels = {
      name        = var.frontal-code_service_namespace
      environment = var.environment
    }
  }
}

# Persistent Volume Claims for frontal-code-server
resource "kubernetes_persistent_volume_claim" "frontal-code_workspace" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-workspace"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  spec {
    access_modes       = ["ReadWriteOnce"]
    storage_class_name = var.storage_class
    resources {
      requests = {
        storage = var.workspace_storage_size
      }
    }
  }

  depends_on = [kubernetes_namespace.frontal-code]
}

resource "kubernetes_persistent_volume_claim" "frontal-code_server_state" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-server-state"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  spec {
    access_modes       = ["ReadWriteOnce"]
    storage_class_name = var.storage_class
    resources {
      requests = {
        storage = var.server_state_storage_size
      }
    }
  }

  depends_on = [kubernetes_namespace.frontal-code]
}

resource "kubernetes_persistent_volume_claim" "frontal-code_agent_store" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-agent-store"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  spec {
    access_modes       = ["ReadWriteOnce"]
    storage_class_name = var.storage_class
    resources {
      requests = {
        storage = var.agent_store_storage_size
      }
    }
  }

  depends_on = [kubernetes_namespace.frontal-code]
}

# ConfigMap for frontal-code-server configuration
resource "kubernetes_config_map" "frontal-code_server_config" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-server-config"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  data = {
    "FRONTAL_CODE_SERVER_HOST"                       = "0.0.0.0"
    "FRONTAL_CODE_SERVER_PORT"                       = "8788"
    "FRONTAL_CODE_SERVER_LANE_TRANSPORT"             = "tools-agent"
    "FRONTAL_CODE_SERVER_RECONCILE_INTERVAL_SECS"    = "15"
    "FRONTAL_CODE_SERVER_ORPHAN_APPROVAL_DELAY_SECS" = "0"
    "FRONTAL_CODE_SERVER_ORPHAN_AUTO_RETRY_SECS"     = "0"
    "FRONTAL_CODE_SERVER_ORPHAN_AUTO_CANCEL_SECS"    = "0"
    "FRONTAL_CODE_SERVER_ORPHAN_POLICY_RULES"        = "[]"
    "FRONTAL_CODE_SERVER_STATE_FILE"                 = "/var/lib/frontal-code/server/state.json"
    "FRONTAL_CODE_AGENT_STORE"                       = "/var/lib/frontal-code/agents"
    "RUST_LOG"                                = "info"
  }

  depends_on = [kubernetes_namespace.frontal-code]
}

# Secrets for frontal-code-server
resource "kubernetes_secret" "frontal-code_server_secrets" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-server-secrets"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  data = {
    "FRONTAL_CODE_SERVER_API_KEY"  = var.frontal-code_server_api_key
    "FRONTAL_API_KEY"       = var.api_keys.anthropic
    "FRONTAL_BASE_URL"      = "https://tools.frontal.dev/frontal-code"
    "OPENAI_API_KEY"        = var.api_keys.openai
    "OPENAI_BASE_URL"       = ""
    "XAI_API_KEY"           = var.api_keys.xai
    "XAI_BASE_URL"          = ""
    "AZURE_OPENAI_API_KEY"  = var.api_keys.azure
    "AZURE_OPENAI_BASE_URL" = ""
    "BEDROCK_API_KEY"       = var.api_keys.bedrock
    "BEDROCK_BASE_URL"      = ""
    "OLLAMA_BASE_URL"       = var.api_keys.ollama
    "OLLAMA_MODEL"          = ""
  }

  type = "Opaque"

  depends_on = [kubernetes_namespace.frontal-code]
}

# Deployment for frontal-code-server
resource "kubernetes_deployment" "frontal-code_server" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-server"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  spec {
    replicas = 2

    selector {
      match_labels = {
        app = "frontal-code-server"
      }
    }

    template {
      metadata {
        labels = {
          app         = "frontal-code-server"
          environment = var.environment
        }
      }

      spec {
        automount_service_account_token = false

        security_context {
          fs_group        = 1000
          run_as_group    = 1000
          run_as_non_root = true
          run_as_user     = 1000
        }

        container {
          name              = "frontal-code-server"
          image             = var.frontal-code_server_image
          image_pull_policy = "IfNotPresent"

          security_context {
            allow_privilege_escalation = false
            run_as_group               = 1000
            run_as_non_root            = true
            run_as_user                = 1000
          }

          port {
            container_port = 8788
            name           = "http"
          }

          env_from {
            config_map_ref {
              name = "frontal-code-server-config"
            }
          }

          env_from {
            secret_ref {
              name = "frontal-code-server-secrets"
            }
          }

          resources {
            limits = {
              cpu    = "2000m"
              memory = "4Gi"
            }
            requests = {
              cpu    = "1000m"
              memory = "2Gi"
            }
          }

          liveness_probe {
            http_get {
              path = "/health"
              port = 8788
            }
            initial_delay_seconds = 30
            period_seconds        = 10
            timeout_seconds       = 5
            failure_threshold     = 3
          }

          readiness_probe {
            http_get {
              path = "/health"
              port = 8788
            }
            initial_delay_seconds = 10
            period_seconds        = 5
            timeout_seconds       = 3
            failure_threshold     = 3
          }

          volume_mount {
            name       = "frontal-code-workspace"
            mount_path = "/workspace"
          }

          volume_mount {
            name       = "frontal-code-server-state"
            mount_path = "/var/lib/frontal-code/server"
          }

          volume_mount {
            name       = "frontal-code-agent-store"
            mount_path = "/var/lib/frontal-code/agents"
          }
        }

        volume {
          name = "frontal-code-workspace"
          persistent_volume_claim {
            claim_name = "frontal-code-workspace"
          }
        }

        volume {
          name = "frontal-code-server-state"
          persistent_volume_claim {
            claim_name = "frontal-code-server-state"
          }
        }

        volume {
          name = "frontal-code-agent-store"
          persistent_volume_claim {
            claim_name = "frontal-code-agent-store"
          }
        }

        affinity {
          pod_anti_affinity {
            preferred_during_scheduling_ignored_during_execution {
              weight = 100
              pod_affinity_term {
                label_selector {
                  match_labels = {
                    app = "frontal-code-server"
                  }
                }
                topology_key = "kubernetes.io/hostname"
              }
            }
          }
        }
      }
    }
  }

  depends_on = [
    kubernetes_namespace.frontal-code,
    kubernetes_config_map.frontal-code_server_config,
    kubernetes_secret.frontal-code_server_secrets,
    kubernetes_persistent_volume_claim.frontal-code_workspace,
    kubernetes_persistent_volume_claim.frontal-code_server_state,
    kubernetes_persistent_volume_claim.frontal-code_agent_store
  ]
}

# Service for frontal-code-server
resource "kubernetes_service" "frontal-code_server" {
  count = var.deploy_frontal-code_server ? 1 : 0
  metadata {
    name      = "frontal-code-server"
    namespace = var.frontal-code_service_namespace
    labels = {
      app         = "frontal-code-server"
      environment = var.environment
    }
  }

  spec {
    selector = {
      app = "frontal-code-server"
    }

    port {
      name        = "http"
      port        = 8788
      target_port = 8788
    }

    type = "ClusterIP"
  }

  depends_on = [kubernetes_deployment.frontal-code_server]
}
