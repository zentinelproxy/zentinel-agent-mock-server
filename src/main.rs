//! Zentinel Mock Server Agent - CLI Entry Point

use anyhow::Result;
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use zentinel_agent_mock_server::{MockServerAgent, MockServerConfig};
use zentinel_agent_sdk::v2::{AgentRunnerV2, TransportConfig};

const DEFAULT_CONFIG: &str = "mock-server.yaml";
const DEFAULT_SOCKET: &str = "/tmp/zentinel-mock-server.sock";

#[derive(Parser, Debug)]
#[command(
    name = "zentinel-mock-server-agent",
    about = "Mock server agent for Zentinel proxy - request stubbing and response simulation",
    version
)]
struct Args {
    /// Path to configuration file. Without it, `mock-server.yaml` in the
    /// working directory is used if it exists, otherwise the defaults (no
    /// stubs).
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Unix socket path (UDS transport, agent protocol v2).
    /// Default: /tmp/zentinel-mock-server.sock. With --grpc-address the
    /// socket is served too only when given explicitly.
    #[arg(short, long)]
    socket: Option<PathBuf>,

    /// gRPC listen address (e.g. "127.0.0.1:50051")
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
    // An explicit --config must exist; the implicit mock-server.yaml is
    // optional.
    let config_path = args
        .config
        .clone()
        .or_else(|| Some(PathBuf::from(DEFAULT_CONFIG)).filter(|p| p.exists()));
    let config = if let Some(path) = config_path {
        if !path.exists() {
            anyhow::bail!("Configuration file not found: {}", path.display());
        }
        info!(path = ?path, "Loading configuration");
        MockServerConfig::from_file(&path)?
    } else if args.validate {
        anyhow::bail!("Configuration file not found: {}", DEFAULT_CONFIG);
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
    let transport = match (args.grpc_address, args.socket) {
        (Some(grpc_addr), Some(socket)) => {
            info!(
                grpc_address = %grpc_addr,
                socket = %socket.display(),
                "Starting mock server agent with gRPC and UDS (v2 protocol)"
            );
            TransportConfig::Both {
                grpc_address: grpc_addr,
                uds_path: socket,
            }
        }
        (Some(grpc_addr), None) => {
            info!(grpc_address = %grpc_addr, "Starting mock server agent with gRPC (v2 protocol)");
            TransportConfig::Grpc { address: grpc_addr }
        }
        (None, socket) => {
            let socket = socket.unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
            info!(socket = %socket.display(), "Starting mock server agent with UDS (v2 protocol)");
            TransportConfig::Uds { path: socket }
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
