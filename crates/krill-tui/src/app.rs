// TUI Application State

use krill_common::{ClientMessage, CommandAction, ServerMessage, ServiceStatus};
use std::collections::HashMap;
use std::io;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    List,
    Logs(String),   // service name
    Detail(String), // service name
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
}

pub struct App {
    pub current_view: View,
    pub services: HashMap<String, ServiceState>,
    pub selected_index: usize,
    pub service_list: Vec<String>,
    pub logs: HashMap<String, Vec<String>>, // per-service logs
    pub log_scroll: usize,                  // scroll offset from bottom (0 = at bottom)
    pub auto_scroll: bool,                  // auto-scroll to new logs
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
                    });

                // Update service list
                self.update_service_list();
            }
            ServerMessage::LogLine { service, line } => {
                // Store logs per-service
                let service_logs = self.logs.entry(service.clone()).or_default();
                service_logs.push(line);

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
                // Prepend history to existing logs
                if let Some(svc) = service {
                    let service_logs = self.logs.entry(svc).or_default();
                    // Insert history at the beginning
                    let mut new_logs = lines;
                    new_logs.append(service_logs);
                    *service_logs = new_logs;
                } else {
                    // All logs - store under special key
                    self.logs.insert("__all__".to_string(), lines);
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
    pub fn current_logs(&self) -> &[String] {
        if let View::Logs(service) = &self.current_view {
            self.logs.get(service).map(|v| v.as_slice()).unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Scroll logs up (older)
    pub fn scroll_logs_up(&mut self, amount: usize) {
        if let View::Logs(service) = &self.current_view {
            let total_logs = self.logs.get(service).map(|v| v.len()).unwrap_or(0);
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
        if let View::Logs(service) = &self.current_view {
            let total_logs = self.logs.get(service).map(|v| v.len()).unwrap_or(0);
            self.log_scroll = total_logs.saturating_sub(1);
            self.auto_scroll = false;
        }
    }

    /// Scroll to bottom (newest logs)
    pub fn scroll_logs_to_bottom(&mut self) {
        self.log_scroll = 0;
        self.auto_scroll = true;
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
