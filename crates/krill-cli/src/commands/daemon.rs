// krill daemon - Run the daemon directly (used internally)

use anyhow::{Context, Result};
use krill_common::KrillConfig;
use krill_daemon::{
    ErrorCategory, IpcServer, LogStore, Orchestrator, StartupError, StartupMessage,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(clap::Args, Debug)]
pub struct DaemonArgs {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: PathBuf,

    /// Log directory (overrides config)
    #[arg(long, value_name = "DIR")]
    pub log_dir: Option<PathBuf>,

    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    pub socket: PathBuf,

    // File descriptor for startup error communication
    #[arg(long, hide = true)]
    pub startup_pipe_fd: Option<i32>,
}

pub async fn execute(args: DaemonArgs) -> Result<()> {
    info!("Pre-flight krill-daemon checks");
    use std::io::Write;
    use std::os::fd::FromRawFd;

    // Open startup pipe if provided
    let mut startup_pipe = args
        .startup_pipe_fd
        .map(|fd| unsafe { std::fs::File::from_raw_fd(fd) });

    // Helper to send error and exit
    let send_error = |pipe: &mut Option<std::fs::File>, category, message, path, hint| {
        let error = StartupError {
            category,
            message,
            path,
            hint,
        };
        let msg = StartupMessage::Error(error);
        if let Some(pipe) = pipe {
            let _ = writeln!(pipe, "{}", serde_json::to_string(&msg).unwrap());
        } else {
            eprintln!("Startup error: {}", msg);
        }
        std::process::exit(1);
    };

    // Load configuration
    info!("Loading configuration from {:?}", args.config);
    let config = match KrillConfig::from_file(&args.config) {
        Ok(c) => c,
        Err(e) => {
            send_error(
                &mut startup_pipe,
                ErrorCategory::Config,
                format!("Failed to load configuration: {}", e),
                Some(args.config.clone()),
                "Check that the file exists, is valid Yaml, and you have read permissions"
                    .to_string(),
            );
            unreachable!();
        }
    };

    info!("Loaded workspace: {}", config.name);
    info!("Services: {}", config.services.len());

    // Initialize log store
    let log_dir = args.log_dir.or(config.log_dir.clone());
    let log_store = match LogStore::new(log_dir.clone()) {
        Ok(ls) => ls,
        Err(e) => {
            send_error(
                &mut startup_pipe,
                ErrorCategory::LogStore,
                format!("Failed to initialize log store: {}", e),
                log_dir.clone(),
                "Check that the directory exists and you have write permissions".to_string(),
            );
            unreachable!();
        }
    };

    info!("Logs directory: {:?}", log_store.session_dir());

    init_daemon_tracing(&log_store);
    info!("Krill daemon starting");
    info!("Workspace: {}", config.name);

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Create log channel for service output
    let (log_tx, mut log_rx) = mpsc::unbounded_channel();

    // Create command channel for IPC
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();

    // Create snapshot request channel
    let (snapshot_req_tx, mut snapshot_req_rx) = mpsc::unbounded_channel::<
        mpsc::UnboundedSender<HashMap<String, krill_common::ServiceSnapshot>>,
    >();

    // Create heartbeat channel
    let (heartbeat_tx, mut heartbeat_rx) = mpsc::unbounded_channel();

    // Create orchestrator with log channel
    let orchestrator = Arc::new(
        match Orchestrator::with_log_tx(config, event_tx.clone(), Some(log_tx)) {
            Ok(o) => o,
            Err(e) => {
                send_error(
                    &mut startup_pipe,
                    ErrorCategory::Orchestrator,
                    format!("Failed to initialize orchestrator: {}", e),
                    None,
                    "Check if you have permission to create channels".to_string(),
                );
                unreachable!();
            }
        },
    );

    // Create IPC server with heartbeat channel and log store
    let ipc_server = Arc::new(
        match IpcServer::with_heartbeat_tx(
            args.socket.clone(),
            command_tx,
            snapshot_req_tx,
            Some(heartbeat_tx),
            Some(Arc::clone(&log_store)),
        ) {
            Ok(is) => is,
            Err(e) => {
                send_error(
                    &mut startup_pipe,
                    ErrorCategory::IpcServer,
                    format!("Failed to initialize IPC server: {}", e),
                    None,
                    "Check if you have permission to create IPC Server".to_string(),
                );
                unreachable!();
            }
        },
    );

    // Send success messages to close pipe
    if let Some(mut pipe) = startup_pipe {
        let msg = StartupMessage::Success;
        let _ = writeln!(pipe, "{}", serde_json::to_string(&msg).unwrap());
        drop(pipe);
    }

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

    // Spawn log forwarding task - writes to log store and broadcasts to clients
    let ipc_server_clone = Arc::clone(&ipc_server);
    let log_store_clone = Arc::clone(&log_store);
    let log_handle = tokio::spawn(async move {
        while let Some((service, line)) = log_rx.recv().await {
            // Write to log store (file + memory)
            log_store_clone.add_log(&service, line.clone()).await;
            // Broadcast to connected clients
            ipc_server_clone.broadcast_log(service, line);
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
                CommandAction::Stop => {
                    if let Some(service) = target {
                        if let Err(e) = orchestrator_clone.stop_service(&service).await {
                            error!("Failed to stop service '{}': {}", service, e);
                        }
                    } else {
                        warn!("Stop command requires a target service");
                    }
                }
                CommandAction::Restart => {
                    if let Some(service) = target {
                        if let Err(e) = orchestrator_clone.restart_service(&service).await {
                            error!("Failed to restart service '{}': {}", service, e);
                        }
                    } else {
                        warn!("Restart command requires a target service");
                    }
                }
                CommandAction::Start => {
                    warn!("Start command not implemented - services start automatically");
                }
                CommandAction::Kill => {
                    warn!("Kill command not implemented - use Stop instead");
                }
            }
        }
    });

    // Spawn snapshot request handling task
    let orchestrator_clone = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        while let Some(response_tx) = snapshot_req_rx.recv().await {
            let snapshot = orchestrator_clone.get_snapshot().await;
            let _ = response_tx.send(snapshot);
        }
    });

    // Spawn heartbeat handling task
    let orchestrator_clone = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        while let Some((service, status, metadata)) = heartbeat_rx.recv().await {
            if let Err(e) = orchestrator_clone
                .process_heartbeat(&service, status, metadata)
                .await
            {
                error!("Failed to process heartbeat for '{}': {}", service, e);
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
    tokio::select! {
        result = signal::ctrl_c() => {
            match result {
                Ok(()) => info!("Received Ctrl+C signal, initiating graceful shutdown"),
                Err(e) => error!("Failed to listen for Ctrl+C: {}", e),
            }
        }
        _ = command_handle => {
            info!("Command handler stopped, initiating shutdown");
        }
        _ = ipc_handle => {
            error!("IPC handler stopped unexpectedly");
        }
    }

    // Shutdown
    info!("Shutting down daemon...");

    if let Err(e) = orchestrator.shutdown().await {
        error!("Error during shutdown: {}", e);
    }

    ipc_server.shutdown().await;

    // Cancel event and log forwarding tasks
    event_handle.abort();
    log_handle.abort();

    info!("Daemon stopped");
    Ok(())
}

fn init_daemon_tracing(log_store: &LogStore) -> Result<()> {
    use tracing_subscriber::{fmt, EnvFilter};

    let daemon_log_path = log_store.get_daemon_log_path();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(daemon_log_path)?;

    let subscriber = fmt()
        .with_writer(file)
        .with_target(false)
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}
