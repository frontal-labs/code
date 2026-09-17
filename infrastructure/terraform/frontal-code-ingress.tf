# Create Ingress for tools.frontal.dev/frontal-code
resource "kubernetes_ingress_v1" "frontal-code_tools" {
  metadata {
    name      = "frontal-code-tools-ingress"
    namespace = var.frontal-code_service_namespace
    annotations = {
      "kubernetes.io/ingress.class"                       = "nginx"
      "cert-manager.io/cluster-issuer"                    = "letsencrypt-prod"
      "nginx.ingress.kubernetes.io/ssl-redirect"          = "true"
      "nginx.ingress.kubernetes.io/use-regex"             = "true"
      "nginx.ingress.kubernetes.io/rewrite-target"        = "/$2"
      "nginx.ingress.kubernetes.io/proxy-body-size"       = "10m"
      "nginx.ingress.kubernetes.io/proxy-read-timeout"    = "300"
      "nginx.ingress.kubernetes.io/proxy-send-timeout"    = "300"
      "nginx.ingress.kubernetes.io/configuration-snippet" = <<-EOT
        more_set_headers "X-Forwarded-Proto: https";
        more_set_headers "X-Forwarded-Host: tools.frontal.dev";
      EOT
    }
  }

  spec {
    tls {
      hosts       = ["tools.frontal.dev"]
      secret_name = "tools-frontal-dev-tls"
    }

    rule {
      host = "tools.frontal.dev"
      http {
        path {
          path     = "/frontal-code(/|$)(.*)"
          pathType = "Prefix"
          backend {
            service {
              name = "frontal-code-server"
              port {
                number = 8788
              }
            }
          }
        }
      }
    }
  }

  depends_on = [helm_release.nginx_ingress]
}

# Create SSL certificate using cert-manager
resource "kubernetes_manifest" "frontal-code_tools_certificate" {
  manifest = {
    apiVersion = "cert-manager.io/v1"
    kind       = "Certificate"
    metadata = {
      name      = "tools-frontal-dev"
      namespace = var.frontal-code_service_namespace
    }
    spec = {
      secretName = "tools-frontal-dev-tls"
      dnsNames   = ["tools.frontal.dev"]
      issuerRef = {
        name = "letsencrypt-prod"
        kind = "ClusterIssuer"
      }
    }
  }

  depends_on = [kubernetes_ingress_v1.frontal-code_tools]
}
