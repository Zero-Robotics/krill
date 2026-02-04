// TUI Rendering

use crate::app::{App, View};
use krill_common::ServiceStatus;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

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
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Service list
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled("krill", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::raw(format!("{} services", app.services.len())),
        Span::raw(" | uptime "),
        Span::raw(app.uptime()),
    ])])
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Left);
    frame.render_widget(header, chunks[0]);

    // Service list
    let items: Vec<ListItem> = app
        .service_list
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let service = app.services.get(name).unwrap();
            let status_symbol = match service.status {
                ServiceStatus::Healthy => "●",
                ServiceStatus::Running => "●",
                ServiceStatus::Degraded => "◐",
                ServiceStatus::Starting => "○",
                ServiceStatus::Stopping => "◌",
                ServiceStatus::Stopped => "○",
                ServiceStatus::Failed => "✗",
            };

            let status_color = match service.status {
                ServiceStatus::Healthy => Color::Green,
                ServiceStatus::Running => Color::Yellow,
                ServiceStatus::Degraded => Color::Magenta,
                ServiceStatus::Starting => Color::Cyan,
                ServiceStatus::Stopping => Color::Gray,
                ServiceStatus::Stopped => Color::Gray,
                ServiceStatus::Failed => Color::Red,
            };

            let pid_str = service
                .pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".to_string());

            let line = Line::from(vec![
                Span::raw(if i == app.selected_index {
                    "▶ "
                } else {
                    "  "
                }),
                Span::styled(format!("{:<15}", name), Style::default()),
                Span::styled(
                    format!("{} {:<10}", status_symbol, format!("{:?}", service.status)),
                    Style::default().fg(status_color),
                ),
                Span::raw(format!("{:<8}", pid_str)),
                Span::raw(format!("{}", service.restart_count)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(" Services "));
    frame.render_widget(list, chunks[1]);

    // Footer with keybindings
    let footer = Paragraph::new(Line::from(vec![
        Span::raw("[↑↓]select "),
        Span::raw("[Enter]logs "),
        Span::raw("[d]etail "),
        Span::raw("[r]estart "),
        Span::raw("[s]top "),
        Span::styled("[S]top-daemon ", Style::default().fg(Color::Red)),
        Span::raw("[q]uit"),
    ]))
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Left);
    frame.render_widget(footer, chunks[2]);
}

fn render_logs_view(frame: &mut Frame, app: &App, service: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Logs
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Logs: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(service),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    // Logs
    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .rev()
        .take(chunks[1].height as usize - 2)
        .rev()
        .map(|line| Line::from(line.as_str()))
        .collect();

    let logs = Paragraph::new(log_lines)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(logs, chunks[1]);

    // Footer
    let footer = Paragraph::new(Line::from("[Esc]back [q]quit"))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    frame.render_widget(footer, chunks[2]);
}

fn render_detail_view(frame: &mut Frame, app: &App, service: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Details
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Detail: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(service),
    ]))
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, chunks[0]);

    // Details
    let mut details = vec![];
    if let Some(svc) = app.services.get(service) {
        details.push(Line::from(format!("Service: {}", svc.name)));
        details.push(Line::from(format!("Status: {:?}", svc.status)));
        details.push(Line::from(format!(
            "PID: {}",
            svc.pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "N/A".to_string())
        )));
        details.push(Line::from(format!("Restart Count: {}", svc.restart_count)));
    }

    let detail_para =
        Paragraph::new(details).block(Block::default().borders(Borders::ALL).title(" Info "));
    frame.render_widget(detail_para, chunks[1]);

    // Footer
    let footer = Paragraph::new(Line::from("[Esc]back [q]quit"))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    frame.render_widget(footer, chunks[2]);
}

fn render_confirmation(frame: &mut Frame, app: &App) {
    let area = centered_rect(60, 20, frame.area());

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::DarkGray))
        .title(" Confirmation ");

    let text = Paragraph::new(app.confirmation_message.as_str())
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

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
