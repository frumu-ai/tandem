use std::io;

use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::time::{Duration, Instant};
use tandem_core::resolve_shared_paths;
use tandem_observability::{
    canonical_logs_dir_from_root, emit_event, init_process_logging, ObservabilityEvent, ProcessKind,
};

mod app;
mod crypto;
mod net;
mod paste_burst;
mod ui;

use app::{App, AppState};
use paste_burst::{CharDecision, FlushResult, PasteBurst};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncRenderMode {
    Auto,
    On,
    Off,
}

impl SyncRenderMode {
    fn from_env() -> Self {
        match std::env::var("TANDEM_TUI_SYNC_RENDER")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("on") => Self::On,
            Some("off") => Self::Off,
            _ => Self::Auto,
        }
    }

    fn enabled_by_default() -> bool {
        #[cfg(target_os = "windows")]
        {
            false
        }
        #[cfg(not(target_os = "windows"))]
        {
            if std::env::var("CI")
                .ok()
                .map(|v| {
                    let t = v.trim();
                    !t.is_empty() && !t.eq_ignore_ascii_case("false") && t != "0"
                })
                .unwrap_or(false)
            {
                return false;
            }
            !matches!(
                std::env::var("TERM").ok().as_deref(),
                Some(term) if term.eq_ignore_ascii_case("dumb")
            )
        }
    }
}

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
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
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
        DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    let tick_rate = Duration::from_millis(80);
    const MAX_EVENTS_PER_FRAME: usize = 64;
    let mut last_tick = Instant::now();
    let mut sync_render_enabled = match SyncRenderMode::from_env() {
        SyncRenderMode::On => true,
        SyncRenderMode::Off => false,
        SyncRenderMode::Auto => SyncRenderMode::enabled_by_default(),
    };
    let mut sync_render_failed = false;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    app.action_tx = Some(tx);
    let mut paste_burst = PasteBurst::default();
    let mut replay_discard_until: Option<Instant> = None;

    loop {
        if app
            .paste_activity_until
            .map(|t| Instant::now() > t)
            .unwrap_or(false)
        {
            app.paste_activity_until = None;
        }
        flush_paste_burst_if_due(app, &mut paste_burst).await?;
        if sync_render_enabled {
            let render_result = (|| -> anyhow::Result<()> {
                execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
                terminal.draw(|f| ui::draw(f, app))?;
                execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
                Ok(())
            })();
            if let Err(err) = render_result {
                if !sync_render_failed {
                    sync_render_failed = true;
                    sync_render_enabled = false;
                    let _ = terminal.clear();
                    emit_event(
                        tracing::Level::WARN,
                        ProcessKind::Tui,
                        ObservabilityEvent {
                            event: "tui.sync_render.disabled",
                            component: "tui.main",
                            correlation_id: None,
                            session_id: None,
                            run_id: None,
                            message_id: None,
                            provider_id: None,
                            model_id: None,
                            status: Some("fallback"),
                            error_code: Some("SYNC_RENDER_FAILED"),
                            detail: Some("disabling synchronized rendering after backend error"),
                        },
                    );
                }
                terminal.draw(|f| ui::draw(f, app)).map_err(|draw_err| {
                    anyhow::anyhow!(
                        "draw failed after sync fallback: {}; original sync error: {}",
                        draw_err,
                        err
                    )
                })?;
            }
        } else {
            terminal.draw(|f| ui::draw(f, app))?;
        }

        while let Ok(action) = rx.try_recv() {
            app.update(action).await?;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            let mut processed_events = 0usize;
            loop {
                match event::read()? {
                    Event::Key(key) => {
                        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                            if is_ctrl_v_paste_key(key)
                                && matches!(app.state, AppState::Chat { .. })
                            {
                                paste_burst.clear_after_explicit_paste();
                                app.paste_activity_until =
                                    Instant::now().checked_add(Duration::from_millis(350));
                                app.update(app::Action::PasteFromClipboard).await?;
                                replay_discard_until =
                                    Instant::now().checked_add(Duration::from_millis(220));
                                // Let the UI redraw immediately so users can see paste feedback.
                                break;
                            } else {
                                if should_drop_replay_plain_key(key, replay_discard_until) {
                                    app.paste_activity_until =
                                        Instant::now().checked_add(Duration::from_millis(250));
                                    if !event::poll(Duration::from_millis(0))? {
                                        break;
                                    }
                                    continue;
                                }
                                if handle_paste_burst_key(app, &mut paste_burst, key).await? {
                                    if !event::poll(Duration::from_millis(0))? {
                                        break;
                                    }
                                    continue;
                                }
                                if let Some(action) = app.handle_key_event(key) {
                                    if action == app::Action::Quit {
                                        app.shutdown().await;
                                        return Ok(());
                                    }
                                    app.update(action).await?;
                                }
                            }
                        }
                    }
                    Event::Mouse(mouse) => {
                        if let Some(action) = app.handle_mouse_event(mouse) {
                            app.update(action).await?;
                        }
                    }
                    Event::Paste(text) => {
                        paste_burst.clear_after_explicit_paste();
                        replay_discard_until = None;
                        app.paste_activity_until =
                            Instant::now().checked_add(Duration::from_millis(350));
                        app.update(app::Action::PasteInput(text)).await?;
                        // Let the UI redraw immediately so users can see paste feedback.
                        break;
                    }
                    _ => {}
                }

                processed_events = processed_events.saturating_add(1);
                if processed_events >= MAX_EVENTS_PER_FRAME {
                    break;
                }
                if !event::poll(Duration::from_millis(0))? {
                    break;
                }
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

fn is_ctrl_v_paste_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn should_drop_replay_plain_key(key: KeyEvent, until: Option<Instant>) -> bool {
    let Some(deadline) = until else {
        return false;
    };
    if Instant::now() > deadline {
        return false;
    }
    let plain_mods = !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SUPER);
    plain_mods && (matches!(key.code, KeyCode::Char(_)) || matches!(key.code, KeyCode::Enter))
}

async fn flush_paste_burst_if_due(
    app: &mut App,
    paste_burst: &mut PasteBurst,
) -> anyhow::Result<()> {
    match paste_burst.flush_if_due(Instant::now()) {
        FlushResult::Paste(pasted) => {
            app.update(app::Action::PasteInput(pasted)).await?;
        }
        FlushResult::Typed(ch) => {
            app.update(app::Action::CommandInput(ch)).await?;
        }
        FlushResult::None => {}
    }
    Ok(())
}

fn has_ctrl_or_alt(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::CONTROL)
        || modifiers.contains(KeyModifiers::ALT)
        || modifiers.contains(KeyModifiers::SUPER)
}

async fn handle_paste_burst_key(
    app: &mut App,
    paste_burst: &mut PasteBurst,
    key: KeyEvent,
) -> anyhow::Result<bool> {
    if !matches!(app.state, AppState::Chat { .. }) {
        paste_burst.clear_after_explicit_paste();
        return Ok(false);
    }

    let now = Instant::now();
    if matches!(key.code, KeyCode::Enter) && paste_burst.append_newline_if_active(now) {
        return Ok(true);
    }

    if matches!(key.code, KeyCode::Enter)
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && paste_burst.newline_should_insert_instead_of_submit(now)
    {
        app.update(app::Action::InsertNewline).await?;
        return Ok(true);
    }

    if matches!(key.code, KeyCode::Enter)
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && paste_burst.newline_should_insert_instead_of_submit(now)
    {
        let _ = paste_burst.append_newline_if_active(now);
        return Ok(true);
    }

    if let KeyCode::Char(ch) = key.code {
        if !has_ctrl_or_alt(key.modifiers) && !ch.is_control() {
            match paste_burst.on_plain_char(ch, now) {
                CharDecision::BufferAppend
                | CharDecision::BeginBuffer
                | CharDecision::BeginBufferFromPending => {
                    paste_burst.append_char_to_buffer(ch, now);
                    return Ok(true);
                }
                CharDecision::RetainFirstChar => {
                    return Ok(true);
                }
            }
        }
    }

    if !matches!(key.code, KeyCode::Char(_) | KeyCode::Enter) {
        if let Some(pasted) = paste_burst.flush_before_modified_input() {
            app.update(app::Action::PasteInput(pasted)).await?;
        }
        paste_burst.clear_window_after_non_char();
    } else if let KeyCode::Char(_) = key.code {
        if has_ctrl_or_alt(key.modifiers) {
            if let Some(pasted) = paste_burst.flush_before_modified_input() {
                app.update(app::Action::PasteInput(pasted)).await?;
            }
            paste_burst.clear_window_after_non_char();
        }
    }

    Ok(false)
}
