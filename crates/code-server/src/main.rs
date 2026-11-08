use code_server::{LaneTransportKind, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = ServerConfig::from_env()?;
    // The in-memory transport records a fake worker and never runs an agent, so
    // tasks reach `completed` without doing any work. Say so at startup.
    if matches!(config.lane_transport_kind, LaneTransportKind::InMemory) {
        eprintln!(
            "warning: lane transport is in-memory: hosted tasks are simulated and no agent runs. \
             Set FRONTAL_CODE_SERVER_LANE_TRANSPORT to `tools-agent` (agents run inside this process) \
             or `local-docker` (agents run in isolated worker containers)."
        );
    }
    code_server::serve(config).await
}
