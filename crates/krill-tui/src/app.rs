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
    pub restart_count: u32,
}

pub struct App {
    pub current_view: View,
    pub services: HashMap<String, ServiceState>,
    pub selected_index: usize,
    pub service_list: Vec<String>,
    pub logs: Vec<String>,
    pub should_quit: bool,
    pub show_confirmation: bool,
    pub confirmation_message: String,
    pub message_tx: mpsc::UnboundedSender<ClientMessage>,
    pub uptime_start: std::time::Instant,
}

impl App {
    pub fn new(message_tx: mpsc::UnboundedSender<ClientMessage>) -> Self {
        Self {
            current_view: View::List,
            services: HashMap::new(),
            selected_index: 0,
            service_list: Vec::new(),
            logs: Vec::new(),
            should_quit: false,
            show_confirmation: false,
            confirmation_message: String::new(),
            message_tx,
            uptime_start: std::time::Instant::now(),
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
                        restart_count: 0,
                    });

                // Update service list
                self.update_service_list();
            }
            ServerMessage::LogLine { service, line } => {
                if let View::Logs(current_service) = &self.current_view {
                    if current_service == &service {
                        self.logs.push(line);
                        // Keep only last 1000 lines
                        if self.logs.len() > 1000 {
                            self.logs.drain(0..500);
                        }
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
                            restart_count: snapshot.restart_count,
                        },
                    );
                }
                self.update_service_list();
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
            self.current_view = View::Logs(service.to_string());
            self.logs.clear();
        }
    }

    pub fn enter_detail(&mut self) {
        if let Some(service) = self.selected_service() {
            self.current_view = View::Detail(service.to_string());
        }
    }

    pub fn back_to_list(&mut self) {
        self.current_view = View::List;
    }

    pub fn restart_selected(&mut self) -> io::Result<()> {
        if let Some(service) = self.selected_service() {
            let msg = ClientMessage::Command {
                action: CommandAction::Restart,
                target: Some(service.to_string()),
            };
            self.message_tx
                .send(msg)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
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
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
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
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
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
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        Ok(())
    }

    pub fn uptime(&self) -> String {
        let elapsed = self.uptime_start.elapsed();
        let hours = elapsed.as_secs() / 3600;
        let minutes = (elapsed.as_secs() % 3600) / 60;
        format!("{}h {}m", hours, minutes)
    }
}
