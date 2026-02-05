// krill logs - View logs

use crate::daemon_manager;
use anyhow::{anyhow, Result};
use krill_common::{ClientMessage, ServerMessage};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(clap::Args, Debug)]
pub struct LogsArgs {
    /// Service name (omit for daemon logs)
    pub service: Option<String>,

    /// Follow log output
    #[arg(short, long)]
    pub follow: bool,

    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    pub socket: PathBuf,
}

pub async fn execute(args: LogsArgs) -> Result<()> {
    // Check if daemon is running
    if !daemon_manager::is_daemon_running(&args.socket).await {
        return Err(anyhow!("Daemon is not running. Start it with 'krill up'"));
    }

    // Connect to daemon
    let stream = UnixStream::connect(&args.socket).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    // Subscribe to logs
    let subscribe_msg = ClientMessage::Subscribe {
        events: false,
        logs: args.service.clone(),
    };

    let json = serde_json::to_string(&subscribe_msg)?;
    writer.write_all(format!("{}\n", json).as_bytes()).await?;

    // If not following, request log history first
    if !args.follow {
        let get_logs_msg = ClientMessage::GetLogs {
            service: args.service.clone(),
        };
        let json = serde_json::to_string(&get_logs_msg)?;
        writer.write_all(format!("{}\n", json).as_bytes()).await?;
    }

    // Print header
    if let Some(ref service) = args.service {
        println!("=== Logs for service: {} ===", service);
    } else {
        println!("=== Daemon logs ===");
    }
    println!();

    // Read and print logs
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                break;
            }
            Ok(_) => {
                if let Ok(msg) = serde_json::from_str::<ServerMessage>(line.trim()) {
                    match msg {
                        ServerMessage::LogLine { service, line } => {
                            if args.service.is_none() || args.service.as_ref() == Some(&service) {
                                println!("[{}] {}", service, line);
                            }
                        }
                        ServerMessage::LogHistory { lines, .. } => {
                            for log_line in lines {
                                println!("{}", log_line);
                            }
                            if !args.follow {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading logs: {}", e);
                break;
            }
        }

        // If not following and we got some output, exit
        if !args.follow {
            break;
        }
    }

    Ok(())
}
