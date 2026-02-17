use std::io;

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};
use std::time::{Duration, Instant};
use tandem_core::resolve_shared_paths;
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};

mod app;
mod crypto;
mod net;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let shared = resolve_shared_paths()?;
    let logs_dir = canonical_logs_dir_from_root(&shared.canonical_root);
    let (_log_guard, _log_info) = init_process_logging(ProcessKind::Tui, &logs_dir, 14)?;
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Tui,
        ObservabilityEvent {
            event: "logging.initialized",
            component: "tui.main",
            correlation_id: None,
            session_id: None,
            run_id: None,
            message_id: None,
            provider_id: None,
            model_id: None,
            status: Some("ok"),
            error_code: None,
            detail: Some("tui jsonl logging initialized"),
        },
    );

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new();

    // Run app
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> anyhow::Result<()> {
    let tick_rate = Duration::from_millis(80);
    let mut last_tick = Instant::now();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    app.action_tx = Some(tx);

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        while let Ok(action) = rx.try_recv() {
            app.update(action).await?;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if let Some(action) = app.handle_key_event(key) {
                            if action == app::Action::Quit {
                                app.shutdown().await;
                                return Ok(());
                            }
                            app.update(action).await?;
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(action) = app.handle_mouse_event(mouse) {
                        app.update(action).await?;
                    }
                }
                Event::Paste(text) => {
                    app.update(app::Action::PasteInput(text)).await?;
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick().await;
            last_tick = Instant::now();
        }

        if app.should_quit {
            app.shutdown().await;
            return Ok(());
        }
    }
}
