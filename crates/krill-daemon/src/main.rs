use clap::Parser;
use krill_common::schema::load_config_from_file;
use std::path::PathBuf;
use tokio::signal;
use tracing::{Level, error, info};

mod daemon;
mod dag;
mod health;
mod ipc;
mod process;
mod safety;
mod supervisor;

use daemon::Daemon;

/// Mission Control Supervisor for robot services
///
/// This daemon orchestrates the lifecycle of every program on the robot,
/// providing process supervision, DAG-based orchestration, and safety
/// interception for critical failures.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the service configuration YAML file
    #[arg(short, long, default_value = "/etc/krill/services.yaml")]
    config: PathBuf,

    /// Path to the PID file
    #[arg(short, long, default_value = "/var/run/krill.pid")]
    pid_file: PathBuf,

    /// Path to the Unix domain socket for IPC
    #[arg(short, long, default_value = "/tmp/krill.sock")]
    socket: PathBuf,

    /// Log directory for service outputs
    #[arg(short, long, default_value = "~/.krill/logs")]
    log_dir: PathBuf,

    /// Enable debug logging
    #[arg(short, long, default_value_t = false)]
    debug: bool,

    /// Dry run: parse configuration but don't start services
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.debug {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_ansi(false)
        .init();

    info!(
        "Starting Krill Mission Control Supervisor v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("Configuration file: {}", args.config.display());
    info!("Socket path: {}", args.socket.display());

    // Load and validate configuration
    let config = match load_config_from_file(&args.config) {
        Ok(config) => {
            info!("Configuration loaded successfully");
            info!("Services defined: {}", config.services.len());
            for service_name in config.services.keys() {
                info!("  - {}", service_name);
            }
            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    if args.dry_run {
        info!("Dry run completed successfully");
        return Ok(());
    }

    // Create daemon instance
    let socket_path = args.socket.clone();
    let daemon = match Daemon::new(config, args.pid_file, socket_path, args.log_dir).await {
        Ok(daemon) => daemon,
        Err(e) => {
            error!("Failed to initialize daemon: {}", e);
            std::process::exit(1);
        }
    };

    // Wrap in Arc for sharing between components
    let daemon = std::sync::Arc::new(daemon);

    // Create supervisor with Arc clone
    // Note: supervisor::Supervisor::new needs to be updated to accept Arc<Daemon>
    let daemon_for_supervisor = std::sync::Arc::clone(&daemon);
    let mut supervisor = match supervisor::Supervisor::new(daemon_for_supervisor).await {
        Ok(supervisor) => supervisor,
        Err(e) => {
            error!("Failed to initialize supervisor: {}", e);
            std::process::exit(1);
        }
    };

    // Start supervisor
    if let Err(e) = supervisor.start().await {
        error!("Failed to start supervisor: {}", e);
        std::process::exit(1);
    }

    // Start IPC server with shared daemon reference
    let ipc_handle = match ipc::start_ipc_server(daemon).await {
        Ok(handle) => {
            info!("IPC server started on {}", args.socket.display());
            handle
        }
        Err(e) => {
            error!("Failed to start IPC server: {}", e);
            std::process::exit(1);
        }
    };

    // Wait for shutdown signals
    match signal::ctrl_c().await {
        Ok(_) => {
            info!("Received shutdown signal");
        }
        Err(e) => {
            error!("Failed to install Ctrl+C handler: {}", e);
            std::process::exit(1);
        }
    }

    // Graceful shutdown
    info!("Initiating graceful shutdown...");

    // Stop IPC server
    if let Err(e) = ipc_handle.shutdown().await {
        error!("Error shutting down IPC server: {}", e);
    }

    // Stop supervisor and all services
    if let Err(e) = supervisor.shutdown().await {
        error!("Error during supervisor shutdown: {}", e);
    }

    info!("Krill Mission Control Supervisor stopped");
    Ok(())
}

/// Handle OS signals for graceful shutdown
async fn handle_signals() -> Result<(), std::io::Error> {
    let mut term = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    let mut int = signal::unix::signal(signal::unix::SignalKind::interrupt())?;

    tokio::select! {
        _ = term.recv() => {
            info!("Received SIGTERM");
        }
        _ = int.recv() => {
            info!("Received SIGINT");
        }
    }

    Ok(())
}
