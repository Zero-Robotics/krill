// Logging System - Per-service and timeline logging

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum LogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub timestamp: DateTime<Utc>,
    pub service: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub struct LogManager {
    session_dir: PathBuf,
    timeline_file: File,
}

impl LogManager {
    pub fn new(base_dir: Option<PathBuf>) -> Result<Self, LogError> {
        // Default to ~/.krill/logs if not specified
        let base_dir = base_dir.unwrap_or_else(|| {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.join(".krill").join("logs")
        });

        // Create session directory with timestamp
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let session_dir = base_dir.join(format!("session-{}", timestamp));

        fs::create_dir_all(&session_dir)?;
        info!("Created log session directory: {:?}", session_dir);

        // Create timeline.jsonl file
        let timeline_path = session_dir.join("timeline.jsonl");
        let timeline_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&timeline_path)?;

        // Create krill.log file for daemon logs
        let daemon_log_path = session_dir.join("krill.log");
        File::create(daemon_log_path)?;

        Ok(Self {
            session_dir,
            timeline_file,
        })
    }

    /// Get the path to a service's log file
    pub fn get_service_log_path(&self, service_name: &str) -> PathBuf {
        self.session_dir.join(format!("{}.log", service_name))
    }

    /// Create a log file for a service
    pub fn create_service_log(&self, service_name: &str) -> Result<File, LogError> {
        let log_path = self.get_service_log_path(service_name);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        Ok(file)
    }

    /// Write to timeline
    pub fn write_timeline(
        &mut self,
        service: &str,
        level: LogLevel,
        message: &str,
    ) -> Result<(), LogError> {
        let event = TimelineEvent {
            timestamp: Utc::now(),
            service: service.to_string(),
            level,
            message: message.to_string(),
        };

        let json = serde_json::to_string(&event)?;
        writeln!(self.timeline_file, "{}", json)?;
        self.timeline_file.flush()?;

        Ok(())
    }

    /// Write to a service log file
    pub fn write_service_log(&self, service_name: &str, line: &str) -> Result<(), LogError> {
        let log_path = self.get_service_log_path(service_name);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        writeln!(file, "[{}] {}", timestamp, line)?;
        file.flush()?;

        Ok(())
    }

    /// Log a daemon event
    pub fn log_daemon(&mut self, level: LogLevel, message: &str) -> Result<(), LogError> {
        self.write_timeline("krill-daemon", level.clone(), message)?;

        let daemon_log_path = self.session_dir.join("krill.log");
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(daemon_log_path)?;

        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        writeln!(file, "[{}] {:?} {}", timestamp, level, message)?;
        file.flush()?;

        Ok(())
    }

    /// Get session directory path
    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }
}

/// Service log writer - handles stdout/stderr from a service process
pub struct ServiceLogWriter {
    service_name: String,
    log_file: File,
    timeline: Option<tokio::sync::mpsc::UnboundedSender<TimelineEvent>>,
}

impl ServiceLogWriter {
    pub fn new(
        service_name: String,
        log_file: File,
        timeline: Option<tokio::sync::mpsc::UnboundedSender<TimelineEvent>>,
    ) -> Self {
        Self {
            service_name,
            log_file,
            timeline,
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), LogError> {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        writeln!(self.log_file, "[{}] {}", timestamp, line)?;
        self.log_file.flush()?;

        // Also write to timeline
        if let Some(timeline_tx) = &self.timeline {
            let event = TimelineEvent {
                timestamp: Utc::now(),
                service: self.service_name.clone(),
                level: LogLevel::Info,
                message: line.to_string(),
            };
            let _ = timeline_tx.send(event);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_log_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let log_manager = LogManager::new(Some(temp_dir.path().to_path_buf())).unwrap();

        assert!(log_manager.session_dir().exists());
        assert!(log_manager.session_dir().join("timeline.jsonl").exists());
        assert!(log_manager.session_dir().join("krill.log").exists());
    }

    #[test]
    fn test_service_log_creation() {
        let temp_dir = TempDir::new().unwrap();
        let log_manager = LogManager::new(Some(temp_dir.path().to_path_buf())).unwrap();

        let _log_file = log_manager.create_service_log("test-service").unwrap();
        assert!(log_manager.get_service_log_path("test-service").exists());
    }

    #[test]
    fn test_write_timeline() {
        let temp_dir = TempDir::new().unwrap();
        let mut log_manager = LogManager::new(Some(temp_dir.path().to_path_buf())).unwrap();

        log_manager
            .write_timeline("test-service", LogLevel::Info, "Test message")
            .unwrap();

        let timeline_path = log_manager.session_dir().join("timeline.jsonl");
        let contents = fs::read_to_string(timeline_path).unwrap();
        assert!(contents.contains("test-service"));
        assert!(contents.contains("Test message"));
    }
}
