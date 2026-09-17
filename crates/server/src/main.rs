use frontal_code_server::ServerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = ServerConfig::from_env()?;
    frontal_code_server::serve(config).await
}
