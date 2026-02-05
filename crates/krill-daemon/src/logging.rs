// Logging System - Per-service and timeline logging

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

/// Maximum log lines to keep in memory per service
const MAX_LOG_LINES: usize = 5000;

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

/// Thread-safe log storage with file persistence
pub struct LogStore {
    /// In-memory log buffer per service
    logs: RwLock<HashMap<String, VecDeque<String>>>,
    /// Session directory for log files
    session_dir: PathBuf,
    /// Timeline file handle
    timeline_file: RwLock<File>,
}

impl LogStore {
    pub fn new(base_dir: Option<PathBuf>) -> Result<Arc<Self>, LogError> {
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

        Ok(Arc::new(Self {
            logs: RwLock::new(HashMap::new()),
            session_dir,
            timeline_file: RwLock::new(timeline_file),
        }))
    }

    /// Add a log line for a service
    pub async fn add_log(&self, service: &str, line: String) {
        // Add to in-memory buffer
        {
            let mut logs = self.logs.write().await;
            let service_logs = logs
                .entry(service.to_string())
                .or_insert_with(VecDeque::new);
            service_logs.push_back(line.clone());

            // Trim if too many lines
            while service_logs.len() > MAX_LOG_LINES {
                service_logs.pop_front();
            }
        }

        // Write to file
        let log_path = self.session_dir.join(format!("{}.log", service));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] {}", timestamp, line);
        }

        // Write to timeline
        {
            let mut timeline = self.timeline_file.write().await;
            let event = TimelineEvent {
                timestamp: Utc::now(),
                service: service.to_string(),
                level: LogLevel::Info,
                message: line,
            };
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(*timeline, "{}", json);
                let _ = timeline.flush();
            }
        }
    }

    /// Get log history for a service (or all services if None)
    pub async fn get_logs(&self, service: Option<&str>, limit: usize) -> Vec<String> {
        let logs = self.logs.read().await;

        match service {
            Some(svc) => logs
                .get(svc)
                .map(|v| v.iter().rev().take(limit).rev().cloned().collect())
                .unwrap_or_default(),
            None => {
                // Return interleaved logs from all services (simplified: just concatenate)
                let mut all_logs: Vec<String> = Vec::new();
                for (svc, svc_logs) in logs.iter() {
                    for line in svc_logs.iter() {
                        all_logs.push(format!("[{}] {}", svc, line));
                    }
                }
                // Take last N lines
                let skip = all_logs.len().saturating_sub(limit);
                all_logs.into_iter().skip(skip).collect()
            }
        }
    }

    /// Get session directory path
    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    /// Log a daemon event
    pub async fn log_daemon(&self, level: LogLevel, message: &str) {
        let daemon_log_path = self.session_dir.join("krill.log");
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(daemon_log_path)
        {
            let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] {:?} {}", timestamp, level, message);
            let _ = file.flush();
        }

        // Also write to timeline
        {
            let mut timeline = self.timeline_file.write().await;
            let event = TimelineEvent {
                timestamp: Utc::now(),
                service: "krill-daemon".to_string(),
                level,
                message: message.to_string(),
            };
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = writeln!(*timeline, "{}", json);
                let _ = timeline.flush();
            }
        }
    }
}

// Keep the old LogManager for backward compatibility but mark as deprecated
// #[deprecated(note = "Use LogStore instead")]
// pub struct LogManager {
//     session_dir: PathBuf,
//     timeline_file: File,
// }

// impl LogManager {
//     pub fn new(base_dir: Option<PathBuf>) -> Result<Self, LogError> {
//         // Default to ~/.krill/logs if not specified
//         let base_dir = base_dir.unwrap_or_else(|| {
//             let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
//             home.join(".krill").join("logs")
//         });

//         // Create session directory with timestamp
//         let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
//         let session_dir = base_dir.join(format!("session-{}", timestamp));

//         fs::create_dir_all(&session_dir)?;
//         info!("Created log session directory: {:?}", session_dir);

//         // Create timeline.jsonl file
//         let timeline_path = session_dir.join("timeline.jsonl");
//         let timeline_file = OpenOptions::new()
//             .create(true)
//             .append(true)
//             .open(&timeline_path)?;

//         // Create krill.log file for daemon logs
//         let daemon_log_path = session_dir.join("krill.log");
//         File::create(daemon_log_path)?;

//         Ok(Self {
//             session_dir,
//             timeline_file,
//         })
//     }

//     /// Get the path to a service's log file
//     pub fn get_service_log_path(&self, service_name: &str) -> PathBuf {
//         self.session_dir.join(format!("{}.log", service_name))
//     }

//     /// Create a log file for a service
//     pub fn create_service_log(&self, service_name: &str) -> Result<File, LogError> {
//         let log_path = self.get_service_log_path(service_name);
//         let file = OpenOptions::new()
//             .create(true)
//             .append(true)
//             .open(log_path)?;
//         Ok(file)
//     }

//     /// Write to timeline
//     pub fn write_timeline(
//         &mut self,
//         service: &str,
//         level: LogLevel,
//         message: &str,
//     ) -> Result<(), LogError> {
//         let event = TimelineEvent {
//             timestamp: Utc::now(),
//             service: service.to_string(),
//             level,
//             message: message.to_string(),
//         };

//         let json = serde_json::to_string(&event)?;
//         writeln!(self.timeline_file, "{}", json)?;
//         self.timeline_file.flush()?;

//         Ok(())
//     }

//     /// Write to a service log file
//     pub fn write_service_log(&self, service_name: &str, line: &str) -> Result<(), LogError> {
//         let log_path = self.get_service_log_path(service_name);

//         let mut file = OpenOptions::new()
//             .create(true)
//             .append(true)
//             .open(log_path)?;

//         let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
//         writeln!(file, "[{}] {}", timestamp, line)?;
//         file.flush()?;

//         Ok(())
//     }

//     /// Log a daemon event
//     pub fn log_daemon(&mut self, level: LogLevel, message: &str) -> Result<(), LogError> {
//         self.write_timeline("krill-daemon", level.clone(), message)?;

//         let daemon_log_path = self.session_dir.join("krill.log");
//         let mut file = OpenOptions::new()
//             .create(true)
//             .append(true)
//             .open(daemon_log_path)?;

//         let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
//         writeln!(file, "[{}] {:?} {}", timestamp, level, message)?;
//         file.flush()?;

//         Ok(())
//     }

//     /// Get session directory path
//     pub fn session_dir(&self) -> &Path {
//         &self.session_dir
//     }
// }

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
    fn test_log_store_creation() {
        let temp_dir = TempDir::new().unwrap();
        let log_store = LogStore::new(Some(temp_dir.path().to_path_buf())).unwrap();

        assert!(log_store.session_dir().exists());
        assert!(log_store.session_dir().join("timeline.jsonl").exists());
        assert!(log_store.session_dir().join("krill.log").exists());
    }
}
