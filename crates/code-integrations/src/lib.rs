//! # `Frontal Code` Integrations
//!
//! This crate provides integration capabilities for the `FrontalCode` system,
//! including Model Context Protocol (MCP) server management and tool bridging.

pub mod ide;
pub mod manifest;
pub mod mcp;
pub mod providers;

// Re-export commonly used MCP types for convenience
pub use mcp::{
    client::McpClientTransport,
    integration::{
        global_integration_registry, IntegrationConfig, IntegrationRegistry, IntegrationTools,
    },
    stdio::{McpServerManager, McpToolDiscoveryReport},
    tool_bridge::{McpConnectionStatus, McpServerState, McpToolRegistry},
    tools::{execute_mcp_tool, mcp_tool_specs, ToolSpec},
};

// Re-export the integration manifest system (the typed plugin contract)
pub use manifest::{
    AuthSpec, CapabilitySpec, ContextFieldSpec, EventSpec, IntegrationManifest,
    ManifestRegistry, SignatureSpec, TransportConfig, ValidationIssue,
};

// Re-export the built-in provider manifests (GitHub, Linear, PostHog, Sentry)
pub use providers::{builtin_providers, BUILTIN_PROVIDER_IDS};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
