//! Sentinel Mock Server Agent - CLI Entry Point

use anyhow::Result;
use clap::Parser;
use sentinel_agent_mock_server::{MockServerAgent, MockServerConfig};
use sentinel_agent_sdk::AgentRunner;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(
    name = "sentinel-agent-mock-server",
    about = "Mock server agent for Sentinel proxy - request stubbing and response simulation",
    version
)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "mock-server.yaml")]
    config: PathBuf,

    /// Unix socket path for agent communication
    #[arg(short, long, default_value = "/tmp/sentinel-mock-server.sock")]
    socket: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short = 'L', long, default_value = "info")]
    log_level: Level,

    /// Print default configuration and exit
    #[arg(long)]
    print_config: bool,

    /// Validate configuration and exit
    #[arg(long)]
    validate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(args.log_level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Print default config if requested
    if args.print_config {
        let default_config = include_str!("../examples/default-config.yaml");
        println!("{}", default_config);
        return Ok(());
    }

    // Load configuration
    let config = if args.config.exists() {
        info!(path = ?args.config, "Loading configuration");
        MockServerConfig::from_file(&args.config)?
    } else if args.validate {
        anyhow::bail!("Configuration file not found: {:?}", args.config);
    } else {
        info!("Using default configuration (no stubs)");
        MockServerConfig::default()
    };

    // Validate and exit if requested
    if args.validate {
        config.validate()?;
        println!("Configuration is valid ({} stubs defined)", config.stubs.len());
        return Ok(());
    }

    // Create agent
    let agent = MockServerAgent::new(config);

    info!(
        socket = ?args.socket,
        "Starting mock server agent"
    );

    // Run the agent
    AgentRunner::new(agent)
        .with_name("mock-server")
        .with_socket(&args.socket)
        .run()
        .await?;

    Ok(())
}
