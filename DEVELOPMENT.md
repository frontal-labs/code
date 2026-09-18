# Development Environment Setup

## Quick Start

### Using Docker Compose
```bash
# Start all services
docker compose -f infrastructure/compose/docker-compose.yml up -d

# View logs
docker compose -f infrastructure/compose/docker-compose.yml logs -f frontal-code-server

# Stop services
docker compose -f infrastructure/compose/docker-compose.yml down
```

### Development with hot reload
```bash
# Start development environment with hot reload
cargo build --workspace
cargo run -p cli -- --help
```

## Configuration

### Development Configuration

For development, you can create a local configuration file at `config/project.json.dev` and symlink it:

```bash
# Create development configuration
cp config/project.json config/project.json.dev

# Edit for development needs
# vim config/project.json.dev

# Use for development
ln -sf config/project.json.dev config/project.json
```

### Environment Variables

Create a `.env` file for local development:

```bash
  # AI Provider
  FRONTAL_API_KEY=sk-ant-...

# Database (for local development)
DATABASE_URL=sqlite:///tmp/frontal-code_dev.db

# Optional: PostgreSQL for full server mode
DATABASE_URL=postgresql://frontal-code:frontal-code_password@localhost:5432/frontal-code_db

# Webhook secret for server mode
WEBHOOK_SECRET=your_webhook_secret_here

# Configuration overrides
FCODE_LOG_LEVEL=debug
FCODE_PERMISSION_MODE=permissive
FCODE_CONFIG_HOME=./dev-config
```

### Core Configuration Development

When developing with the core configuration system:

```rust
use frontal-code_core::config::ProjectConfig;
use frontal-code_runtime::ConfigurationManager;

// In development, you can load configuration directly
let config = ProjectConfig::load_or_default();

// Or use the configuration manager for bridge functionality
let manager = ConfigurationManager::load_with_cwd(".")?;

// Enable development-specific features
if std::env::var("FCODE_DEV_MODE").is_ok() {
    println!("Development mode enabled");
    println!("Telemetry: {}", config.features.enable_telemetry);
    println!("Plugins: {}", config.features.enable_plugins);
}
```

### Testing Configuration

Test configuration changes without affecting your main setup:

```bash
# Test with custom config directory
export FCODE_CONFIG_HOME=./test-config
frontal-code /doctor

# Test with specific config file
cp config/project.json config/test-project.json
# Edit test-project.json
FCODE_CONFIG_HOME=./test-config frontal-code /doctor
```

## Pre-commit Hooks

Install pre-commit hooks:

```bash
# Install pre-commit
pip install pre-commit

# Install hooks
pre-commit install

# Run hooks manually
pre-commit run --all-files
```

## Development Commands

```bash
# Build
cargo build --workspace

# Build release
cargo build --release --workspace

# Run tests
cargo test --workspace

# Run with logging
RUST_LOG=debug cargo run --bin frontal-code

# Check code
cargo check --workspace

# Format code
cargo fmt --all

# Lint code
cargo clippy --workspace -- -D warnings

# Run doctor check
cargo run --bin frontal-code -- doctor

# Generate documentation
cargo doc --workspace --no-deps --document-private-items

# Audit dependencies
cargo audit
```

## Services Overview

### Core Services
- **frontal-code**: Main Frontal Code service
- **postgres**: PostgreSQL database for structured memory
- **redis**: Redis for caching and session management
- **pinecone**: Managed vector database for semantic search
- **webhook-server**: Webhook receiver for external events

### Development Services
- **frontal-code-dev**: Development environment with hot reload

## Database Setup

### PostgreSQL
```bash
# Connect to database
docker-compose exec postgres psql -U frontal-code -d frontal-code_db

# View tables
\dt

# View schema
\d+ tasks
```

### Redis
```bash
# Connect to Redis
docker-compose exec redis redis-cli -a redis_password

# View keys
KEYS *
```

### Pinecone
```bash
# Export Pinecone settings before running memory-backed flows
export FCODE_MEMORY_PINECONE_URL=https://YOUR_INDEX_HOST
export FCODE_MEMORY_PINECONE_API_KEY=your_pinecone_api_key_here
export FCODE_MEMORY_PINECONE_NAMESPACE=dev

# Run the focused tool integration test against a configured backend
cargo test -p frontal-code-tools env_backed_memory_tools_route_requests_to_pinecone_and_neo4j -- --nocapture
```

## Testing

```bash
# Run all tests
cargo test --workspace

# Run specific test
cargo test --package cli test_name

# Run tests with output
cargo test --workspace -- --nocapture

# Run mock parity harness
./scripts/run_mock_parity_harness.sh
```

## Troubleshooting

### Common Issues

1. **Permission denied errors**: Make sure your user has permissions for the workspace directory
2. **Database connection errors**: Check that database services are running and accessible
3. **Build failures**: Ensure all dependencies are installed and Rust toolchain is up to date

### Health Checks

```bash
# Check Frontal Code health
docker-compose exec frontal-code frontal-code doctor

# Check service status
docker-compose ps

# View logs
docker-compose logs [service_name]
```

### Reset Environment

```bash
# Stop all services
docker-compose down

# Remove volumes (this will delete all data!)
docker-compose down -v

# Rebuild and start
docker-compose up --build
```
