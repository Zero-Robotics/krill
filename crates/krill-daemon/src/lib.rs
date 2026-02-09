// Krill Daemon - Process orchestrator for robotics systems

pub mod ipc_server;
pub mod logging;
pub mod orchestrator;
pub mod runner;

use krill_common::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
// use thiserror::Error;

pub use ipc_server::IpcServer;
#[allow(deprecated)]
pub use logging::LogStore;
pub use orchestrator::Orchestrator;
pub use runner::ServiceRunner;

#[derive(Serialize, Deserialize)]
pub enum StartupMessage {
    Success,
    Error(StartupError),
}

impl fmt::Display for StartupMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartupMessage::Success => write!(f, "Startup successful"),
            StartupMessage::Error(err) => write!(f, "Startup error: {}", err),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct StartupError {
    pub category: ErrorCategory,
    pub message: String,
    pub path: Option<PathBuf>,
    pub hint: String,
}
impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;

        if let Some(path) = &self.path {
            write!(f, " (path: {})", path.display())?;
        }

        if !self.hint.is_empty() {
            write!(f, "\nHint: {}", self.hint)?;
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub enum ErrorCategory {
    Config,
    LogStore,
    Orchestrator,
    IpcServer,
}
