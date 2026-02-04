// Krill Daemon - Main entry point

use anyhow::{Context, Result};
use clap::Parser;
use krill_common::KrillConfig;
use krill_daemon::{IpcServer, LogManager, Orchestrator};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "krill-daemon")]
#[command(about = "Krill process orchestrator daemon", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    config: PathBuf,

    /// Log directory (overrides config)
    #[arg(long, value_name = "DIR")]
    log_dir: Option<PathBuf>,

    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    socket: PathBuf,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = if args.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    fmt().with_env_filter(filter).with_target(false).init();

    info!("Starting krill-daemon");

    // Load configuration
    info!("Loading configuration from {:?}", args.config);
    let config = KrillConfig::from_file(&args.config).context("Failed to load configuration")?;

    info!("Loaded workspace: {}", config.name);
    info!("Services: {}", config.services.len());

    // Initialize logging system
    let log_dir = args.log_dir.or(config.log_dir.clone());
    let log_manager = LogManager::new(log_dir).context("Failed to initialize log manager")?;

    info!("Logs directory: {:?}", log_manager.session_dir());

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Create command channel for IPC
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();

    // Create orchestrator
    let orchestrator = Arc::new(
        Orchestrator::new(config, event_tx.clone()).context("Failed to create orchestrator")?,
    );

    // Create IPC server
    let ipc_server = Arc::new(
        IpcServer::new(args.socket.clone(), command_tx).context("Failed to create IPC server")?,
    );

    // Spawn IPC server task
    let ipc_server_clone = Arc::clone(&ipc_server);
    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = ipc_server_clone.start().await {
            error!("IPC server error: {}", e);
        }
    });

    // Spawn event forwarding task
    let ipc_server_clone = Arc::clone(&ipc_server);
    let event_handle = tokio::spawn(async move {
        while let Some((service, status)) = event_rx.recv().await {
            info!("Event: {} -> {:?}", service, status);
            ipc_server_clone.broadcast_event(service, status);
        }
    });

    // Spawn command handling task
    let orchestrator_clone = Arc::clone(&orchestrator);
    let command_handle = tokio::spawn(async move {
        while let Some((action, target)) = command_rx.recv().await {
            info!("Command: {:?} for {:?}", action, target);

            use krill_common::CommandAction;
            match action {
                CommandAction::StopDaemon => {
                    info!("Received stop daemon command");
                    if let Err(e) = orchestrator_clone.shutdown().await {
                        error!("Shutdown error: {}", e);
                    }
                    break;
                }
                _ => {
                    warn!("Command handling not yet implemented: {:?}", action);
                }
            }
        }
    });

    // Start all services
    info!("Starting all services...");
    if let Err(e) = orchestrator.start_all().await {
        error!("Failed to start services: {}", e);
        return Err(e.into());
    }

    info!("All services started successfully");
    info!("Daemon running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    let shutdown_signal = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("Received Ctrl+C, initiating graceful shutdown");
    };

    shutdown_signal.await;

    // Shutdown
    info!("Shutting down daemon...");

    if let Err(e) = orchestrator.shutdown().await {
        error!("Error during shutdown: {}", e);
    }

    ipc_server.shutdown().await;

    // Wait for tasks to complete
    let _ = tokio::join!(ipc_handle, event_handle, command_handle);

    info!("Daemon stopped");
    Ok(())
}
