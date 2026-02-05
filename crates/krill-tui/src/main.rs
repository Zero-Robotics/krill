// Krill TUI - Terminal UI for krill daemon

use anyhow::Result;
use std::io;
use std::path::PathBuf;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(clap::Parser, Debug)]
#[command(name = "krill-tui")]
#[command(about = "Krill Terminal UI", long_about = None)]
struct Args {
    /// IPC socket path
    #[arg(long, default_value = "/tmp/krill.sock")]
    socket: PathBuf,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    use clap::Parser;
    let args = Args::parse();

    // Initialize tracing
    let filter = if args.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("error")
    };

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(io::stderr)
        .init();

    // Run TUI
    let config = krill_tui::TuiConfig {
        socket: args.socket,
    };

    krill_tui::run(config).await
}
