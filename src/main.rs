//! Zentinel Mock Server Agent - CLI Entry Point

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use zentinel_agent_mock_server::{MockServerAgent, MockServerConfig};
use zentinel_agent_sdk::v2::{AgentRunnerV2, TransportConfig};

#[derive(Parser, Debug)]
#[command(
    name = "zentinel-agent-mock-server",
    about = "Mock server agent for Zentinel proxy - request stubbing and response simulation",
    version
)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "mock-server.yaml")]
    config: PathBuf,

    /// Unix socket path for agent communication
    #[arg(short, long, default_value = "/tmp/zentinel-mock-server.sock")]
    socket: PathBuf,

    /// gRPC server address (e.g., "0.0.0.0:50051")
    #[arg(long, value_name = "ADDR")]
    grpc_address: Option<SocketAddr>,

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
        println!(
            "Configuration is valid ({} stubs defined)",
            config.stubs.len()
        );
        return Ok(());
    }

    // Create agent
    let agent = MockServerAgent::new(config);

    // Configure transport based on CLI options
    let transport = match args.grpc_address {
        Some(grpc_addr) => {
            info!(
                grpc_address = %grpc_addr,
                socket = %args.socket.display(),
                "Starting mock server agent with gRPC and UDS (v2 protocol)"
            );
            TransportConfig::Both {
                grpc_address: grpc_addr,
                uds_path: args.socket,
            }
        }
        None => {
            info!(socket = %args.socket.display(), "Starting mock server agent with UDS (v2 protocol)");
            TransportConfig::Uds { path: args.socket }
        }
    };

    // Run agent with v2 runner
    let mut runner = AgentRunnerV2::new(agent).with_name("mock-server");

    runner = match transport {
        TransportConfig::Grpc { address } => runner.with_grpc(address),
        TransportConfig::Uds { path } => runner.with_uds(path),
        TransportConfig::Both {
            grpc_address,
            uds_path,
        } => runner.with_both(grpc_address, uds_path),
    };

    runner.run().await?;

    Ok(())
}
