// Krill TUI Library

pub mod app;
pub mod ui;

pub use app::App;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use krill_common::{ClientMessage, ServerMessage};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Debug, Clone)]
pub struct TuiConfig {
    pub socket: PathBuf,
}

/// Run the TUI application
pub async fn run(config: TuiConfig) -> Result<()> {
    info!("Starting krill-tui");

    // Connect to daemon
    let stream = UnixStream::connect(&config.socket)
        .await
        .context("Failed to connect to daemon. Is krill-daemon running?")?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    // Create channels
    let (message_tx, mut message_rx) = mpsc::unbounded_channel::<ClientMessage>();
    let (server_tx, mut server_rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Spawn task to send messages to daemon
    tokio::spawn(async move {
        while let Some(msg) = message_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if writer
                    .write_all(format!("{}\n", json).as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    // Spawn task to receive messages from daemon
    tokio::spawn(async move {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<ServerMessage>(line.trim()) {
                        if server_tx.send(msg).is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading from daemon: {}", e);
                    break;
                }
            }
        }
    });

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new(message_tx);

    // Subscribe to events
    let _ = app.request_snapshot();
    let subscribe_msg = ClientMessage::Subscribe {
        events: true,
        logs: None,
    };
    if app.message_tx.send(subscribe_msg).is_err() {
        error!("Failed to subscribe to events");
    }

    // Main loop
    let result = run_app(&mut terminal, &mut app, &mut server_rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    server_rx: &mut mpsc::UnboundedReceiver<ServerMessage>,
) -> Result<()> {
    let mut tick_interval = tokio::time::interval(tokio::time::Duration::from_millis(250));
    let mut needs_redraw = true;

    loop {
        // Only redraw if something changed
        if needs_redraw {
            terminal.draw(|f| ui::render(f, app))?;
            needs_redraw = false;
        }

        // Handle events with priority for keyboard input
        tokio::select! {
            biased;  // Process in order, prioritizing user input

            // Keyboard input (highest priority)
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {
                if event::poll(std::time::Duration::from_millis(0))? {
                    if let Event::Key(key) = event::read()? {
                        if !handle_input(app, key)? {
                            break;
                        }
                        needs_redraw = true;
                    }
                }
            }

            // Server messages
            msg = server_rx.recv() => {
                if let Some(message) = msg {
                    app.handle_server_message(message);
                    needs_redraw = true;
                }
            }

            // Tick for periodic updates (lowest priority)
            _ = tick_interval.tick() => {
                // Periodic redraw for time updates
                needs_redraw = true;
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Handle confirmation dialog
    if app.show_confirmation {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.cancel_confirmation();
                app.confirm_stop_daemon()?;
                return Ok(false); // Quit
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.cancel_confirmation();
            }
            _ => {}
        }
        return Ok(true);
    }

    // Handle different views
    match &app.current_view {
        app::View::List => match key.code {
            KeyCode::Char('q') => return Ok(false),
            KeyCode::Up | KeyCode::Char('k') => app.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_down(),
            KeyCode::Enter => app.enter_logs(),
            KeyCode::Char('d') => app.enter_detail(),
            KeyCode::Char('r') => app.restart_selected()?,
            KeyCode::Char('s') => app.stop_selected()?,
            KeyCode::Char('S') => app.show_stop_daemon_confirmation(),
            _ => {}
        },
        app::View::Logs(_) => match key.code {
            KeyCode::Char('q') => return Ok(false),
            KeyCode::Esc => app.back_to_list(),
            // Single line scrolling
            KeyCode::Up | KeyCode::Char('k') => app.scroll_logs_up(1),
            KeyCode::Down | KeyCode::Char('j') => app.scroll_logs_down(1),
            // Page scrolling
            KeyCode::PageUp => app.scroll_logs_up(20),
            KeyCode::PageDown => app.scroll_logs_down(20),
            // Half-page scrolling (vim style)
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.scroll_logs_up(10)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.scroll_logs_down(10)
            }
            // Fast scrolling (5 lines)
            KeyCode::Char('K') => app.scroll_logs_up(5),
            KeyCode::Char('J') => app.scroll_logs_down(5),
            // Jump to top/bottom
            KeyCode::Char('g') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    app.scroll_logs_to_bottom();
                } else {
                    app.scroll_logs_to_top();
                }
            }
            KeyCode::Char('G') => app.scroll_logs_to_bottom(),
            KeyCode::Home => app.scroll_logs_to_top(),
            KeyCode::End => app.scroll_logs_to_bottom(),
            // Toggle auto-scroll
            KeyCode::Char('f') | KeyCode::Char('F') => app.toggle_auto_scroll(),
            _ => {}
        },
        app::View::Detail(_) => match key.code {
            KeyCode::Char('q') => return Ok(false),
            KeyCode::Esc => app.back_to_list(),
            _ => {}
        },
    }

    Ok(true)
}
