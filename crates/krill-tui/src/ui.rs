// TUI Rendering - K9s-inspired design

use crate::app::{App, View};
use krill_common::ServiceStatus;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

// K9s-inspired color scheme
const HEADER_BG: Color = Color::Rgb(30, 40, 60);
const HEADER_FG: Color = Color::White;
const SELECTED_BG: Color = Color::Rgb(50, 60, 80);
const SELECTED_FG: Color = Color::White;
const TABLE_HEADER_FG: Color = Color::Rgb(100, 150, 200);
const BORDER_COLOR: Color = Color::Rgb(60, 70, 90);
const DIM_FG: Color = Color::Rgb(120, 120, 120);

// Status colors
const STATUS_HEALTHY: Color = Color::Rgb(80, 200, 120);
const STATUS_RUNNING: Color = Color::Rgb(220, 180, 50);
const STATUS_STARTING: Color = Color::Rgb(100, 180, 220);
const STATUS_DEGRADED: Color = Color::Rgb(200, 100, 200);
const STATUS_STOPPED: Color = Color::Rgb(100, 100, 100);
const STATUS_FAILED: Color = Color::Rgb(220, 80, 80);

pub fn render(frame: &mut Frame, app: &App) {
    match &app.current_view {
        View::List => render_list_view(frame, app),
        View::Logs(service) => render_logs_view(frame, app, service),
        View::Detail(service) => render_detail_view(frame, app, service),
    }

    // Render confirmation dialog if shown
    if app.show_confirmation {
        render_confirmation(frame, app);
    }
}

fn render_list_view(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Header (multi-line)
            Constraint::Min(0),    // Service list
            Constraint::Length(1), // Footer (single line)
        ])
        .split(frame.area());

    // K9s-style header bar
    render_header(frame, app, chunks[0]);

    // Service list
    render_service_list(frame, app, chunks[1]);

    // Footer with keybindings (compact)
    render_footer(frame, chunks[2]);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    // Count services by status
    let healthy_count = app
        .services
        .values()
        .filter(|s| s.status == ServiceStatus::Healthy)
        .count();
    let other_count = app.services.len() - healthy_count;

    // Get workspace name from first service
    let workspace = app
        .services
        .values()
        .next()
        .map(|s| s.namespace.as_str())
        .unwrap_or("krill");

    // CPU and Memory usage colors
    let cpu_color = if app.cpu_usage > 80.0 {
        STATUS_FAILED
    } else if app.cpu_usage > 50.0 {
        STATUS_RUNNING
    } else {
        STATUS_HEALTHY
    };

    let memory_percent = if app.memory_total_mb > 0 {
        (app.memory_used_mb as f32 / app.memory_total_mb as f32) * 100.0
    } else {
        0.0
    };
    let mem_color = if memory_percent > 80.0 {
        STATUS_FAILED
    } else if memory_percent > 50.0 {
        STATUS_RUNNING
    } else {
        STATUS_HEALTHY
    };

    let disk_percent = if app.disk_total_gb > 0.0 {
        (app.disk_usage_gb / app.disk_total_gb) * 100.0
    } else {
        0.0
    };

    let disk_color = if disk_percent > 95.0 {
        STATUS_FAILED
    } else if disk_percent > 50.0 {
        STATUS_RUNNING
    } else {
        STATUS_HEALTHY
    };

    // Line 1: Krill branding
    let line1 = Line::from(vec![Span::styled(
        " Krill",
        Style::default()
            .fg(Color::Black)
            .bg(STATUS_HEALTHY)
            .add_modifier(Modifier::BOLD),
    )]);

    // Line 2: Recipe and services
    let line2 = Line::from(vec![
        Span::raw(" "),
        Span::styled("recipe: ", Style::default().fg(DIM_FG)),
        Span::styled(
            workspace,
            Style::default().fg(HEADER_FG).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(DIM_FG)),
        Span::styled(
            format!("{} services", app.services.len()),
            Style::default().fg(HEADER_FG),
        ),
        Span::styled(" │ ", Style::default().fg(DIM_FG)),
        Span::styled(
            format!("{}", healthy_count),
            Style::default().fg(STATUS_HEALTHY),
        ),
        Span::styled(" ", Style::default()),
        Span::styled(
            format!("{}", other_count),
            Style::default().fg(STATUS_STOPPED),
        ),
    ]);

    // Line 3: CPU, Memory, Disk
    let line3 = Line::from(vec![
        Span::raw(" "),
        Span::styled("CPU: ", Style::default().fg(DIM_FG)),
        Span::styled(
            format!("{:.1}%", app.cpu_usage),
            Style::default().fg(cpu_color),
        ),
        Span::raw("  "),
        Span::styled("MEM: ", Style::default().fg(DIM_FG)),
        Span::styled(
            format!("{}MB/{}MB", app.memory_used_mb, app.memory_total_mb),
            Style::default().fg(mem_color),
        ),
        Span::raw("  "),
        Span::styled("DISK: ", Style::default().fg(DIM_FG)),
        Span::styled(
            format!("{:.2}GB/{:.2}GB", app.disk_usage_gb, app.disk_total_gb),
            Style::default().fg(disk_color),
        ),
    ]);

    let header = Paragraph::new(vec![line1, line2, line3])
        .style(Style::default().bg(HEADER_BG))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER_COLOR)),
        );
    frame.render_widget(header, area);
}

fn render_service_list(frame: &mut Frame, app: &App, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    // Table header
    let header_style = Style::default()
        .fg(TABLE_HEADER_FG)
        .add_modifier(Modifier::BOLD);
    let header = Line::from(vec![
        Span::styled(format!(" {:<20}", "NAME"), header_style),
        Span::styled(format!("{:<12}", "STATUS"), header_style),
        Span::styled(format!("{:<22}", "NAMESPACE (UID)"), header_style),
        Span::styled(format!("{:<10}", "UPTIME"), header_style),
        Span::styled(format!("{:<10}", "EXECUTOR"), header_style),
        Span::styled(format!("{:<8}", "RESTARTS"), header_style),
    ]);
    items.push(ListItem::new(header));

    // Services
    for (i, name) in app.service_list.iter().enumerate() {
        let service = app.services.get(name).unwrap();

        let (status_symbol, status_color) = match service.status {
            ServiceStatus::Healthy => ("●", STATUS_HEALTHY),
            ServiceStatus::Running => ("●", STATUS_RUNNING),
            ServiceStatus::Degraded => ("◐", STATUS_DEGRADED),
            ServiceStatus::Starting => ("◐", STATUS_STARTING),
            ServiceStatus::Stopping => ("◌", STATUS_STOPPED),
            ServiceStatus::Stopped => ("○", STATUS_STOPPED),
            ServiceStatus::Failed => ("✗", STATUS_FAILED),
        };

        let is_selected = i == app.selected_index;
        let row_style = if is_selected {
            Style::default().bg(SELECTED_BG).fg(SELECTED_FG)
        } else {
            Style::default()
        };

        let status_text = format!("{:?}", service.status);

        // Format namespace with UID
        let namespace_uid = format!("{} ({})", service.namespace, service.uid);

        // Format uptime
        let uptime_str = if let Some(uptime) = service.uptime {
            let secs = uptime.as_secs();
            if secs < 60 {
                format!("{}s", secs)
            } else if secs < 3600 {
                format!("{}m", secs / 60)
            } else if secs < 86400 {
                format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
            } else {
                format!("{}d{}h", secs / 86400, (secs % 86400) / 3600)
            }
        } else {
            "-".to_string()
        };

        let line = Line::from(vec![
            Span::styled(
                format!(" {:<20}", name),
                row_style.add_modifier(if is_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(format!("{} ", status_symbol), row_style.fg(status_color)),
            Span::styled(format!("{:<10}", status_text), row_style.fg(status_color)),
            Span::styled(
                format!("{:<22}", namespace_uid),
                row_style.fg(if is_selected { SELECTED_FG } else { DIM_FG }),
            ),
            Span::styled(
                format!("{:<10}", uptime_str),
                row_style.fg(if is_selected {
                    SELECTED_FG
                } else {
                    Color::LightBlue
                }),
            ),
            Span::styled(
                format!("{:<10}", service.executor_type),
                row_style.fg(if is_selected {
                    Color::LightCyan
                } else {
                    Color::Cyan
                }),
            ),
            Span::styled(
                format!("{:<8}", service.restart_count),
                row_style.fg(if is_selected { SELECTED_FG } else { DIM_FG }),
            ),
        ]);

        items.push(ListItem::new(line).style(row_style));
    }

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default()),
    );
    frame.render_widget(list, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" <↑↓>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Navigate ", Style::default().fg(DIM_FG)),
        Span::styled("<enter>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Logs ", Style::default().fg(DIM_FG)),
        Span::styled("<d>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Describe ", Style::default().fg(DIM_FG)),
        Span::styled("<r>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Restart ", Style::default().fg(DIM_FG)),
        Span::styled("<s>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Stop ", Style::default().fg(DIM_FG)),
        Span::styled("<q>", Style::default().fg(STATUS_FAILED)),
        Span::styled("Quit ", Style::default().fg(DIM_FG)),
    ]))
    .style(Style::default().bg(HEADER_BG));
    frame.render_widget(footer, area);
}

fn render_logs_view(frame: &mut Frame, app: &App, service: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Logs
            Constraint::Length(1), // Footer
        ])
        .split(frame.area());

    let logs = app.current_logs();
    let total_logs = logs.len();
    let visible_height = chunks[1].height as usize;

    // Calculate scroll percentage for visual indicator
    let scroll_percent = if total_logs > visible_height {
        let scrollable = total_logs - visible_height;
        let position = scrollable.saturating_sub(app.log_scroll);
        (position * 100) / scrollable.max(1)
    } else {
        100
    };

    // Header with scroll info
    let scroll_info = if total_logs > 0 {
        let position = total_logs.saturating_sub(app.log_scroll);
        format!(" [{}/{}] {}%", position, total_logs, scroll_percent)
    } else {
        String::new()
    };

    let auto_scroll_indicator = if app.auto_scroll {
        Span::styled(
            " [FOLLOW]",
            Style::default()
                .fg(STATUS_HEALTHY)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [PAUSED]",
            Style::default()
                .fg(STATUS_RUNNING)
                .add_modifier(Modifier::BOLD),
        )
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " krill ",
            Style::default()
                .fg(Color::Black)
                .bg(STATUS_HEALTHY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("Logs: ", Style::default().fg(DIM_FG)),
        Span::styled(
            service,
            Style::default().fg(HEADER_FG).add_modifier(Modifier::BOLD),
        ),
        Span::styled(scroll_info, Style::default().fg(DIM_FG)),
        auto_scroll_indicator,
    ]))
    .style(Style::default().bg(HEADER_BG))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER_COLOR)),
    );
    frame.render_widget(header, chunks[0]);

    // Split logs area to add scroll bar on the right
    let logs_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),    // Logs content
            Constraint::Length(1), // Scroll bar
        ])
        .split(chunks[1]);

    // Calculate which logs to show based on scroll position
    // log_scroll=0 means we're at the bottom (newest logs)
    // log_scroll=N means we're N lines up from the bottom
    let log_lines: Vec<Line> = if total_logs == 0 {
        vec![Line::from(Span::styled(
            "No logs yet. Waiting for output...",
            Style::default().fg(DIM_FG),
        ))]
    } else {
        let end_idx = total_logs.saturating_sub(app.log_scroll);
        let start_idx = end_idx.saturating_sub(visible_height);

        logs[start_idx..end_idx]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                // Add line numbers for easier reference
                let line_num = start_idx + i + 1;
                // Color code based on content
                let style = if line.contains("ERROR") || line.contains("error") {
                    Style::default().fg(STATUS_FAILED)
                } else if line.contains("WARN") || line.contains("warn") {
                    Style::default().fg(STATUS_RUNNING)
                } else {
                    Style::default().fg(HEADER_FG)
                };
                Line::from(vec![
                    Span::styled(format!("{:4} ", line_num), Style::default().fg(DIM_FG)),
                    Span::styled(line.as_str(), style),
                ])
            })
            .collect()
    };

    let logs_widget = Paragraph::new(log_lines);
    frame.render_widget(logs_widget, logs_chunks[0]);

    // Render scroll bar
    render_scroll_bar(
        frame,
        logs_chunks[1],
        total_logs,
        visible_height,
        app.log_scroll,
    );

    // Footer with scroll keybindings
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" <j/k>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Scroll ", Style::default().fg(DIM_FG)),
        Span::styled("<J/K>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Fast ", Style::default().fg(DIM_FG)),
        Span::styled("<g/G>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Top/Bot ", Style::default().fg(DIM_FG)),
        Span::styled("<f>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Follow ", Style::default().fg(DIM_FG)),
        Span::styled("<esc>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Back ", Style::default().fg(DIM_FG)),
        Span::styled("<q>", Style::default().fg(STATUS_FAILED)),
        Span::styled("Quit", Style::default().fg(DIM_FG)),
    ]))
    .style(Style::default().bg(HEADER_BG));
    frame.render_widget(footer, chunks[2]);
}

/// Render a visual scroll bar
fn render_scroll_bar(
    frame: &mut Frame,
    area: Rect,
    total_lines: usize,
    visible_lines: usize,
    scroll_offset: usize,
) {
    if total_lines <= visible_lines || area.height == 0 {
        // No scrolling needed, just fill with spaces
        let empty: Vec<Line> = (0..area.height)
            .map(|_| Line::from(Span::styled(" ", Style::default().fg(BORDER_COLOR))))
            .collect();
        let widget = Paragraph::new(empty);
        frame.render_widget(widget, area);
        return;
    }

    let height = area.height as usize;
    let scrollable = total_lines - visible_lines;

    // Calculate thumb position and size
    let thumb_size = ((visible_lines * height) / total_lines).max(1).min(height);
    let position = scrollable.saturating_sub(scroll_offset);
    let thumb_pos = if scrollable > 0 {
        ((position * (height - thumb_size)) / scrollable).min(height - thumb_size)
    } else {
        0
    };

    let mut lines: Vec<Line> = Vec::with_capacity(height);
    for i in 0..height {
        let char = if i >= thumb_pos && i < thumb_pos + thumb_size {
            "█" // Thumb
        } else {
            "│" // Track
        };
        let style = if i >= thumb_pos && i < thumb_pos + thumb_size {
            Style::default().fg(STATUS_HEALTHY)
        } else {
            Style::default().fg(BORDER_COLOR)
        };
        lines.push(Line::from(Span::styled(char, style)));
    }

    let widget = Paragraph::new(lines);
    frame.render_widget(widget, area);
}

fn render_detail_view(frame: &mut Frame, app: &App, service: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Details
            Constraint::Length(1), // Footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " krill ",
            Style::default()
                .fg(Color::Black)
                .bg(STATUS_HEALTHY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled("Describe: ", Style::default().fg(DIM_FG)),
        Span::styled(
            service,
            Style::default().fg(HEADER_FG).add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().bg(HEADER_BG))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER_COLOR)),
    );
    frame.render_widget(header, chunks[0]);

    // Details
    let mut details = vec![];
    if let Some(svc) = app.services.get(service) {
        let (status_symbol, status_color) = match svc.status {
            ServiceStatus::Healthy => ("●", STATUS_HEALTHY),
            ServiceStatus::Running => ("●", STATUS_RUNNING),
            ServiceStatus::Degraded => ("◐", STATUS_DEGRADED),
            ServiceStatus::Starting => ("◐", STATUS_STARTING),
            ServiceStatus::Stopping => ("◌", STATUS_STOPPED),
            ServiceStatus::Stopped => ("○", STATUS_STOPPED),
            ServiceStatus::Failed => ("✗", STATUS_FAILED),
        };

        // Basic info section
        details.push(Line::from(Span::styled(
            "═══ Service Info ═══",
            Style::default()
                .fg(TABLE_HEADER_FG)
                .add_modifier(Modifier::BOLD),
        )));
        details.push(Line::from(vec![
            Span::styled("Name:         ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(&svc.name, Style::default().fg(HEADER_FG)),
        ]));
        details.push(Line::from(vec![
            Span::styled("Namespace:    ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(&svc.namespace, Style::default().fg(HEADER_FG)),
        ]));
        details.push(Line::from(vec![
            Span::styled("Executor:     ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(&svc.executor_type, Style::default().fg(Color::Cyan)),
        ]));
        details.push(Line::from(vec![
            Span::styled("Status:       ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                format!("{} {:?}", status_symbol, svc.status),
                Style::default().fg(status_color),
            ),
        ]));
        details.push(Line::from(vec![
            Span::styled("PID:          ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                svc.pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "N/A".to_string()),
                Style::default().fg(HEADER_FG),
            ),
        ]));

        // Uptime
        let uptime_str = if let Some(uptime) = svc.uptime {
            let secs = uptime.as_secs();
            if secs < 60 {
                format!("{}s", secs)
            } else if secs < 3600 {
                format!("{}m {}s", secs / 60, secs % 60)
            } else if secs < 86400 {
                format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
            } else {
                format!(
                    "{}d {}h {}m",
                    secs / 86400,
                    (secs % 86400) / 3600,
                    (secs % 3600) / 60
                )
            }
        } else {
            "N/A".to_string()
        };
        details.push(Line::from(vec![
            Span::styled("Uptime:       ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(uptime_str, Style::default().fg(Color::LightBlue)),
        ]));

        details.push(Line::from(""));

        // Dependencies section
        details.push(Line::from(Span::styled(
            "═══ Dependencies ═══",
            Style::default()
                .fg(TABLE_HEADER_FG)
                .add_modifier(Modifier::BOLD),
        )));
        if svc.dependencies.is_empty() {
            details.push(Line::from(Span::styled(
                "None",
                Style::default().fg(DIM_FG),
            )));
        } else {
            for dep in &svc.dependencies {
                details.push(Line::from(vec![
                    Span::styled("  • ", Style::default().fg(STATUS_HEALTHY)),
                    Span::styled(dep, Style::default().fg(HEADER_FG)),
                ]));
            }
        }

        details.push(Line::from(""));

        // Resource Requirements section
        details.push(Line::from(Span::styled(
            "═══ Resources ═══",
            Style::default()
                .fg(TABLE_HEADER_FG)
                .add_modifier(Modifier::BOLD),
        )));
        details.push(Line::from(vec![
            Span::styled("GPU Required: ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                if svc.uses_gpu { "Yes" } else { "No" },
                Style::default().fg(if svc.uses_gpu { STATUS_RUNNING } else { DIM_FG }),
            ),
        ]));
        details.push(Line::from(vec![
            Span::styled("Critical:     ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                if svc.critical { "Yes" } else { "No" },
                Style::default().fg(if svc.critical { STATUS_FAILED } else { DIM_FG }),
            ),
        ]));

        details.push(Line::from(""));

        // Restart Policy section
        details.push(Line::from(Span::styled(
            "═══ Restart Policy ═══",
            Style::default()
                .fg(TABLE_HEADER_FG)
                .add_modifier(Modifier::BOLD),
        )));
        details.push(Line::from(vec![
            Span::styled("Policy:       ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(&svc.restart_policy, Style::default().fg(Color::Cyan)),
        ]));
        details.push(Line::from(vec![
            Span::styled("Max Restarts: ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                if svc.max_restarts == 0 {
                    "Unlimited".to_string()
                } else {
                    svc.max_restarts.to_string()
                },
                Style::default().fg(HEADER_FG),
            ),
        ]));
        details.push(Line::from(vec![
            Span::styled("Restarts:     ", Style::default().fg(TABLE_HEADER_FG)),
            Span::styled(
                format!("{}", svc.restart_count),
                Style::default().fg(if svc.restart_count > 0 {
                    STATUS_RUNNING
                } else {
                    HEADER_FG
                }),
            ),
        ]));
    }

    let detail_para = Paragraph::new(details).block(Block::default().borders(Borders::NONE));
    frame.render_widget(detail_para, chunks[1]);

    // Footer
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" <esc>", Style::default().fg(STATUS_HEALTHY)),
        Span::styled("Back ", Style::default().fg(DIM_FG)),
        Span::styled("<q>", Style::default().fg(STATUS_FAILED)),
        Span::styled("Quit ", Style::default().fg(DIM_FG)),
    ]))
    .style(Style::default().bg(HEADER_BG));
    frame.render_widget(footer, chunks[2]);
}

fn render_confirmation(frame: &mut Frame, app: &App) {
    let area = centered_rect(50, 20, frame.area());

    // Clear background
    let clear = Block::default().style(Style::default().bg(Color::Black));
    frame.render_widget(clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(STATUS_FAILED))
        .style(Style::default().bg(HEADER_BG))
        .title(Span::styled(
            " Confirm ",
            Style::default()
                .fg(STATUS_FAILED)
                .add_modifier(Modifier::BOLD),
        ));

    let text = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            &app.confirmation_message,
            Style::default().fg(HEADER_FG),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("<y>", Style::default().fg(STATUS_HEALTHY)),
            Span::styled("es  ", Style::default().fg(DIM_FG)),
            Span::styled("<n>", Style::default().fg(STATUS_FAILED)),
            Span::styled("o", Style::default().fg(DIM_FG)),
        ]),
    ])
    .block(block)
    .alignment(Alignment::Center);

    frame.render_widget(text, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
