use frontal_code_api::service::ApiServiceConfig;
use frontal_code_core::config::ProjectConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load core configuration
    let core_config = ProjectConfig::load_or_default();
    println!("Starting FrontalCode API with core configuration:");
    println!(
        "  Project: {} v{}",
        core_config.project.name, core_config.project.version
    );
    println!(
        "  Default provider: {}",
        core_config.runtime.default_provider
    );
    println!(
        "  Max concurrent requests: {}",
        core_config.runtime.max_concurrent_requests
    );
    println!(
        "  Telemetry enabled: {}",
        core_config.features.enable_telemetry
    );

    let config = ApiServiceConfig::from_env()?;
    frontal_code_api::service::serve(config).await
}
