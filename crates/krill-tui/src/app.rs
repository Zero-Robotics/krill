// TUI Application State

use crate::richtext::strip_ansi;
use krill_common::{
    ClientMessage, CommandAction, LogLevel, LogStream, ServerMessage, ServiceStatus,
};
use std::collections::HashMap;
use std::io;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    List,
    Logs(String),   // service name
    Detail(String), // service name
}

/// One stored log line: the raw text (may contain ANSI), plus the stream and
/// severity the daemon tagged it with (used for gutter color / stderr flagging).
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub text: String,
    pub level: LogLevel,
    pub stream: LogStream,
}

/// Infer a level from text alone (for history lines, which arrive untagged).
fn infer_level_text(text: &str) -> LogLevel {
    let lower = text.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("panic") || lower.contains("fatal") {
        LogLevel::Error
    } else if lower.contains("warn") {
        LogLevel::Warn
    } else {
        LogLevel::Info
    }
}

#[derive(Debug, Clone)]
pub struct ServiceState {
    pub name: String,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub uid: String,
    pub restart_count: u32,
    pub namespace: String,
    pub executor_type: String,
    pub uptime: Option<std::time::Duration>,
    pub dependencies: Vec<String>,
    pub uses_gpu: bool,
    pub critical: bool,
    pub restart_policy: String,
    pub max_restarts: u32,
    pub last_error: Option<String>,
}

pub struct App {
    pub current_view: View,
    pub services: HashMap<String, ServiceState>,
    pub selected_index: usize,
    pub service_list: Vec<String>,
    pub logs: HashMap<String, Vec<LogEntry>>, // per-service logs
    pub log_scroll: usize, // scroll offset from bottom, in log entries (0 = at bottom)
    pub auto_scroll: bool, // auto-scroll to new logs
    pub wrap_logs: bool,   // soft-wrap long log lines instead of clipping
    pub log_search: String, // active search term (highlighted; empty = none)
    pub log_search_editing: bool, // currently typing into the search box
    pub log_search_buffer: String, // in-progress search text while editing
    pub status_message: Option<String>, // transient status (e.g. export path)
    pub should_quit: bool,
    pub show_confirmation: bool,
    pub confirmation_message: String,
    pub message_tx: mpsc::UnboundedSender<ClientMessage>,
    pub uptime_start: std::time::Instant,
    pub cpu_usage: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_usage_gb: f32,
    pub disk_total_gb: f32,
}

impl App {
    pub fn new(message_tx: mpsc::UnboundedSender<ClientMessage>) -> Self {
        Self {
            current_view: View::List,
            services: HashMap::new(),
            selected_index: 0,
            service_list: Vec::new(),
            logs: HashMap::new(),
            log_scroll: 0,
            auto_scroll: true,
            wrap_logs: true,
            log_search: String::new(),
            log_search_editing: false,
            log_search_buffer: String::new(),
            status_message: None,
            should_quit: false,
            show_confirmation: false,
            confirmation_message: String::new(),
            message_tx,
            uptime_start: std::time::Instant::now(),
            cpu_usage: 0.0,
            memory_used_mb: 0,
            memory_total_mb: 0,
            disk_usage_gb: 0.0,
            disk_total_gb: 0.0,
        }
    }

    pub fn handle_server_message(&mut self, message: ServerMessage) {
        match message {
            ServerMessage::StatusUpdate { service, status } => {
                self.services
                    .entry(service.clone())
                    .and_modify(|s| s.status = status.clone())
                    .or_insert(ServiceState {
                        name: service.clone(),
                        status,
                        pid: None,
                        uid: String::new(),
                        restart_count: 0,
                        namespace: String::new(),
                        executor_type: String::new(),
                        uptime: None,
                        dependencies: Vec::new(),
                        uses_gpu: false,
                        critical: false,
                        restart_policy: String::new(),
                        max_restarts: 0,
                        last_error: None,
                    });

                // Update service list
                self.update_service_list();
            }
            ServerMessage::LogLine {
                service,
                line,
                stream,
                level,
            } => {
                // Store logs per-service
                let service_logs = self.logs.entry(service.clone()).or_default();
                service_logs.push(LogEntry {
                    text: line,
                    level,
                    stream,
                });

                // Keep only last 2000 lines per service
                if service_logs.len() > 2000 {
                    service_logs.drain(0..1000);
                }

                // If viewing this service's logs and auto_scroll is on, stay at bottom
                if let View::Logs(current_service) = &self.current_view {
                    if current_service == &service && self.auto_scroll {
                        self.log_scroll = 0;
                    }
                }
            }
            ServerMessage::Snapshot { services } => {
                for (name, snapshot) in services {
                    self.services.insert(
                        name.clone(),
                        ServiceState {
                            name: name.clone(),
                            status: snapshot.status,
                            pid: snapshot.pid,
                            uid: snapshot.uid,
                            restart_count: snapshot.restart_count,
                            namespace: snapshot.namespace,
                            executor_type: snapshot.executor_type,
                            uptime: snapshot.uptime,
                            dependencies: snapshot.dependencies,
                            uses_gpu: snapshot.uses_gpu,
                            critical: snapshot.critical,
                            restart_policy: snapshot.restart_policy,
                            max_restarts: snapshot.max_restarts,
                            last_error: snapshot.last_error,
                        },
                    );
                }
                self.update_service_list();
            }
            ServerMessage::SystemStats {
                cpu_usage,
                memory_used_mb,
                memory_total_mb,
                disk_usage_gb,
                disk_total_gb,
            } => {
                self.cpu_usage = cpu_usage;
                self.memory_used_mb = memory_used_mb;
                self.memory_total_mb = memory_total_mb;
                self.disk_usage_gb = disk_usage_gb;
                self.disk_total_gb = disk_total_gb;
            }
            ServerMessage::LogHistory { service, lines } => {
                // History arrives as plain strings (no per-line tags); infer level
                // from the text and default the stream to stdout.
                let to_entries = |lines: Vec<String>| -> Vec<LogEntry> {
                    lines
                        .into_iter()
                        .map(|text| LogEntry {
                            level: infer_level_text(&text),
                            stream: LogStream::Stdout,
                            text,
                        })
                        .collect()
                };
                if let Some(svc) = service {
                    let service_logs = self.logs.entry(svc).or_default();
                    // Insert history at the beginning
                    let mut new_logs = to_entries(lines);
                    new_logs.append(service_logs);
                    *service_logs = new_logs;
                } else {
                    // All logs - store under special key
                    self.logs.insert("__all__".to_string(), to_entries(lines));
                }
            }
            _ => {}
        }
    }

    fn update_service_list(&mut self) {
        self.service_list = self.services.keys().cloned().collect();
        self.service_list.sort();
    }

    pub fn selected_service(&self) -> Option<&str> {
        self.service_list
            .get(self.selected_index)
            .map(|s| s.as_str())
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected_index < self.service_list.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    pub fn enter_logs(&mut self) {
        if let Some(service) = self.selected_service() {
            let service_name = service.to_string();
            self.current_view = View::Logs(service_name.clone());
            self.log_scroll = 0;
            self.auto_scroll = true;
            self.log_search.clear();
            self.log_search_editing = false;
            self.status_message = None;

            // Request log history first
            let get_logs_msg = ClientMessage::GetLogs {
                service: Some(service_name.clone()),
            };
            let _ = self.message_tx.send(get_logs_msg);

            // Subscribe to this service's logs
            let subscribe_msg = ClientMessage::Subscribe {
                events: true,
                logs: Some(service_name),
            };
            let _ = self.message_tx.send(subscribe_msg);
        }
    }

    pub fn enter_detail(&mut self) {
        if let Some(service) = self.selected_service() {
            self.current_view = View::Detail(service.to_string());
        }
    }

    pub fn back_to_list(&mut self) {
        self.current_view = View::List;
        // Re-subscribe to all logs
        let subscribe_msg = ClientMessage::Subscribe {
            events: true,
            logs: None,
        };
        let _ = self.message_tx.send(subscribe_msg);
    }

    /// Get logs for the current service being viewed
    pub fn current_logs(&self) -> &[LogEntry] {
        if let View::Logs(service) = &self.current_view {
            self.logs.get(service).map(|v| v.as_slice()).unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Number of log entries for the current service.
    pub fn current_log_len(&self) -> usize {
        self.current_logs().len()
    }

    /// Scroll logs up (older)
    pub fn scroll_logs_up(&mut self, amount: usize) {
        if let View::Logs(_) = &self.current_view {
            let total_logs = self.current_log_len();
            self.log_scroll = self
                .log_scroll
                .saturating_add(amount)
                .min(total_logs.saturating_sub(1));
            self.auto_scroll = false;
        }
    }

    /// Scroll logs down (newer)
    pub fn scroll_logs_down(&mut self, amount: usize) {
        self.log_scroll = self.log_scroll.saturating_sub(amount);
        if self.log_scroll == 0 {
            self.auto_scroll = true;
        }
    }

    /// Scroll to top (oldest logs)
    pub fn scroll_logs_to_top(&mut self) {
        if let View::Logs(_) = &self.current_view {
            let total_logs = self.current_log_len();
            self.log_scroll = total_logs.saturating_sub(1);
            self.auto_scroll = false;
        }
    }

    /// Scroll to bottom (newest logs)
    pub fn scroll_logs_to_bottom(&mut self) {
        self.log_scroll = 0;
        self.auto_scroll = true;
    }

    /// Toggle soft-wrapping of long log lines.
    pub fn toggle_wrap(&mut self) {
        self.wrap_logs = !self.wrap_logs;
    }

    /// The entry index currently anchored at the bottom of the view.
    fn anchor_index(&self) -> usize {
        self.current_log_len()
            .saturating_sub(1)
            .saturating_sub(self.log_scroll)
    }

    /// Anchor the view so that entry `idx` sits at the bottom.
    fn scroll_to_index(&mut self, idx: usize) {
        let total = self.current_log_len();
        self.log_scroll = total.saturating_sub(idx + 1);
        self.auto_scroll = self.log_scroll == 0;
    }

    /// Enter search-input mode, seeded with the current term.
    pub fn begin_search(&mut self) {
        self.log_search_editing = true;
        self.log_search_buffer = self.log_search.clone();
    }

    pub fn search_push(&mut self, c: char) {
        self.log_search_buffer.push(c);
    }

    pub fn search_backspace(&mut self) {
        self.log_search_buffer.pop();
    }

    /// Apply the typed text as the active search term (highlighted in-place) and
    /// jump to the newest matching line, if any.
    pub fn search_commit(&mut self) {
        self.log_search = self.log_search_buffer.trim().to_string();
        self.log_search_editing = false;
        if self.log_search.is_empty() {
            return;
        }
        let needle = self.log_search.to_ascii_lowercase();
        let newest = self
            .current_logs()
            .iter()
            .rposition(|e| e.text.to_ascii_lowercase().contains(&needle));
        if let Some(idx) = newest {
            self.scroll_to_index(idx);
        }
    }

    /// Leave search-input mode without changing the active term.
    pub fn search_cancel(&mut self) {
        self.log_search_editing = false;
    }

    /// Clear the active search term (removes highlight).
    pub fn clear_search(&mut self) {
        self.log_search.clear();
    }

    /// Jump to the previous (`older`) or next (newer) line matching the search
    /// term, moving the view to bring it to the bottom.
    pub fn jump_match(&mut self, older: bool) {
        if self.log_search.is_empty() {
            return;
        }
        let needle = self.log_search.to_ascii_lowercase();
        let total = self.current_log_len();
        if total == 0 {
            return;
        }
        let anchor = self.anchor_index();
        let found = {
            let entries = self.current_logs();
            let hit = |i: usize| entries[i].text.to_ascii_lowercase().contains(&needle);
            if older {
                (0..anchor).rev().find(|&i| hit(i))
            } else {
                ((anchor + 1)..total).find(|&i| hit(i))
            }
        };
        if let Some(idx) = found {
            self.scroll_to_index(idx);
        }
    }

    /// Write the current service's log lines (ANSI stripped) to a temp file so
    /// the user can open/grep/copy them outside the TUI. Sets `status_message`.
    pub fn export_current_logs(&mut self) {
        let service = match &self.current_view {
            View::Logs(s) => s.clone(),
            _ => return,
        };
        let mut body = String::new();
        let mut count = 0;
        if let Some(entries) = self.logs.get(&service) {
            for e in entries {
                body.push_str(&strip_ansi(&e.text));
                body.push('\n');
                count += 1;
            }
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("krill-{}-{}.log", service, ts));
        self.status_message = Some(match std::fs::write(&path, body) {
            Ok(()) => format!("exported {} lines → {}", count, path.display()),
            Err(e) => format!("export failed: {}", e),
        });
    }

    /// Toggle auto-scroll mode
    pub fn toggle_auto_scroll(&mut self) {
        self.auto_scroll = !self.auto_scroll;
        if self.auto_scroll {
            self.log_scroll = 0; // Jump to bottom when enabling
        }
    }

    pub fn restart_selected(&mut self) -> io::Result<()> {
        if let Some(service) = self.selected_service() {
            let msg = ClientMessage::Command {
                action: CommandAction::Restart,
                target: Some(service.to_string()),
            };
            self.message_tx
                .send(msg)
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(())
    }

    pub fn stop_selected(&mut self) -> io::Result<()> {
        if let Some(service) = self.selected_service() {
            let msg = ClientMessage::Command {
                action: CommandAction::Stop,
                target: Some(service.to_string()),
            };
            self.message_tx
                .send(msg)
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        Ok(())
    }

    pub fn show_stop_daemon_confirmation(&mut self) {
        self.show_confirmation = true;
        self.confirmation_message = "Stop daemon? All services will be stopped. (Y/N)".to_string();
    }

    pub fn confirm_stop_daemon(&mut self) -> io::Result<()> {
        let msg = ClientMessage::Command {
            action: CommandAction::StopDaemon,
            target: None,
        };
        self.message_tx
            .send(msg)
            .map_err(|e| io::Error::other(e.to_string()))?;
        self.should_quit = true;
        Ok(())
    }

    pub fn cancel_confirmation(&mut self) {
        self.show_confirmation = false;
        self.confirmation_message.clear();
    }

    pub fn request_snapshot(&mut self) -> io::Result<()> {
        let msg = ClientMessage::GetSnapshot;
        self.message_tx
            .send(msg)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }

    pub fn uptime(&self) -> String {
        let elapsed = self.uptime_start.elapsed();
        let hours = elapsed.as_secs() / 3600;
        let minutes = (elapsed.as_secs() % 3600) / 60;
        format!("{}h {}m", hours, minutes)
    }
}
