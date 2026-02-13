use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Tick,
    Quit,
    EnterPin(char),
    SubmitPin,
    CreateSession,
    LoadSessions,
    SessionsLoaded(Vec<Session>),
    SelectSession,
    NewSession,
    NextSession,
    PreviousSession,
    SkipAnimation,
    // Add more actions here
}

use crate::net::client::Session;

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    StartupAnimation {
        frame: usize,
    },

    PinPrompt {
        input: String,
        error: Option<String>,
    },
    MainMenu,
    Chat,
    Connecting,
}

use crate::net::client::EngineClient;
use tokio::process::{Child, Command};

use crate::crypto::{keystore::SecureKeyStore, vault::EncryptedVaultKey};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tandem_core::{migrate_legacy_storage_if_needed, resolve_shared_paths};

pub struct App {
    pub state: AppState,
    pub matrix: crate::ui::matrix::MatrixEffect,
    pub should_quit: bool,
    pub tick_count: usize,
    pub config_dir: Option<PathBuf>,
    pub vault_key: Option<EncryptedVaultKey>,
    pub keystore: Option<SecureKeyStore>,
    pub engine_process: Option<Child>,
    pub client: Option<EngineClient>,
    pub sessions: Vec<Session>,
    pub selected_session_index: usize,
}

impl App {
    pub fn new() -> Self {
        let config_dir = Self::find_or_create_config_dir();

        let vault_key = if let Some(dir) = &config_dir {
            let path = dir.join("vault.key");
            if path.exists() {
                EncryptedVaultKey::load(&path).ok()
            } else {
                None
            }
        } else {
            None
        };

        Self {
            state: AppState::StartupAnimation { frame: 0 },
            matrix: crate::ui::matrix::MatrixEffect::new(0, 0),

            should_quit: false,
            tick_count: 0,
            config_dir,
            vault_key,
            keystore: None,
            engine_process: None,
            client: None,
            sessions: Vec::new(),
            selected_session_index: 0,
        }
    }

    fn find_or_create_config_dir() -> Option<PathBuf> {
        if let Ok(paths) = resolve_shared_paths() {
            let _ = std::fs::create_dir_all(&paths.canonical_root);
            if let Ok(report) = migrate_legacy_storage_if_needed(&paths) {
                tracing::info!(
                    "TUI storage migration status: reason={} performed={} copied={} skipped={} errors={}",
                    report.reason,
                    report.performed,
                    report.copied.len(),
                    report.skipped.len(),
                    report.errors.len()
                );
            }
            return Some(paths.canonical_root);
        }
        None
    }

    pub fn handle_key_event(&self, key: KeyEvent) -> Option<Action> {
        // Global exit keys
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('x') => return Some(Action::Quit),
                _ => {}
            }
        }

        match self.state {
            AppState::StartupAnimation { .. } => {
                // Any key skips animation
                // But let's ignore modifier keys alone to prevent accidental skips?
                // Actually user said "no animation", maybe it's skipping too easily.
                // Let's only skip on Enter or Esc or Space.
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                        Some(Action::SkipAnimation)
                    }
                    _ => None,
                }
            }
            AppState::PinPrompt { .. } => match key.code {
                KeyCode::Esc => Some(Action::Quit),
                KeyCode::Enter => Some(Action::SubmitPin),
                KeyCode::Char(c) => Some(Action::EnterPin(c)),
                KeyCode::Backspace => Some(Action::EnterPin('\x08')), // Using backspace char for delete
                _ => None,
            },
            AppState::Connecting => {
                // Poll for completion?
                Some(Action::Tick)
            }
            AppState::MainMenu => match key.code {
                KeyCode::Char('q') => Some(Action::Quit),
                KeyCode::Char('n') => Some(Action::NewSession),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::NextSession),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::PreviousSession),
                KeyCode::Enter => Some(Action::SelectSession),
                _ => None,
            },

            _ => None,
        }
    }

    pub async fn update(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SkipAnimation => {
                if let AppState::StartupAnimation { .. } = self.state {
                    self.state = AppState::PinPrompt {
                        input: String::new(),
                        error: None,
                    };
                }
            }
            Action::Tick => {
                if let AppState::StartupAnimation { frame } = &mut self.state {
                    *frame += 1;
                    self.matrix.update(120, 40);
                }
            }

            Action::EnterPin(c) => {
                if let AppState::PinPrompt { input, .. } = &mut self.state {
                    if c == '\x08' {
                        input.pop();
                    } else {
                        input.push(c);
                    }
                }
            }
            Action::SubmitPin => {
                if let AppState::PinPrompt { input, .. } = &mut self.state {
                    match &self.vault_key {
                        Some(vk) => {
                            match vk.decrypt(input) {
                                Ok(master_key) => {
                                    // Load keystore
                                    if let Some(config_dir) = &self.config_dir {
                                        let keystore_path = config_dir.join("tandem.keystore");
                                        if let Ok(store) =
                                            SecureKeyStore::load(keystore_path, master_key)
                                        {
                                            self.keystore = Some(store);
                                            // Transition to Connecting state
                                            self.state = AppState::Connecting;
                                            // We need to trigger connection logic.
                                            // Ideally we return an Action to do it, or spawn a task.
                                            // For now, let's just set state and let tick/update handle it?
                                            // Actually, we can return a command/action from update?
                                            // But update returns Result<()>.
                                            return Ok(());
                                        } else {
                                            self.state = AppState::PinPrompt {
                                                input: String::new(),
                                                error: Some("Failed to load keystore".to_string()),
                                            };
                                        }
                                    } else {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some("Config dir not found".to_string()),
                                        };
                                    }
                                }
                                Err(_) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some("Invalid PIN".to_string()),
                                    };
                                }
                            }
                        }
                        None => {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("No vault.key found".to_string()),
                            };
                        }
                    }
                }
            }

            Action::SessionsLoaded(sessions) => {
                self.sessions = sessions;
                if self.selected_session_index >= self.sessions.len() && !self.sessions.is_empty() {
                    self.selected_session_index = self.sessions.len() - 1;
                }
            }
            Action::NextSession => {
                if !self.sessions.is_empty() {
                    self.selected_session_index =
                        (self.selected_session_index + 1) % self.sessions.len();
                }
            }
            Action::PreviousSession => {
                if !self.sessions.is_empty() {
                    if self.selected_session_index > 0 {
                        self.selected_session_index -= 1;
                    } else {
                        self.selected_session_index = self.sessions.len() - 1;
                    }
                }
            }
            Action::NewSession => {
                if let Some(client) = &self.client {
                    let client = client.clone();
                    // We can't await easily here if update locks self?
                    // Actually update is async, so we can await.
                    // But we hold &mut self.
                    // client clone allows us to call it.
                    // But we can't assign to self.sessions *after* await while holding client?
                    // No, `client` is a local variable. `self` is currently borrowed.
                    // We can't call methods on self.

                    if let Ok(_) = client.create_session(Some("New Session".to_string())).await {
                        // Refresh sessions
                        if let Ok(sessions) = client.list_sessions().await {
                            self.sessions = sessions;
                            // Select the new one (usually first or last depending on sort)
                            // server sorts by updated desc, so new one is first.
                            self.selected_session_index = 0;
                            self.state = AppState::Chat;
                        }
                    }
                }
            }

            Action::SelectSession => {
                if !self.sessions.is_empty() {
                    // Get session ID
                    // self.state = AppState::Chat { session_id: ... }
                    self.state = AppState::Chat;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn tick(&mut self) {
        self.tick_count += 1;
        match &mut self.state {
            AppState::StartupAnimation { frame } => {
                *frame += 1;
                // Update matrix with real terminal size
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }
            }
            AppState::PinPrompt { .. } => {
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }
            }

            AppState::Connecting => {
                // Try to connect or spawn
                if self.client.is_none() {
                    // Check if running
                    let client = EngineClient::new("http://127.0.0.1:3000".to_string());
                    if let Ok(healthy) = client.check_health().await {
                        if healthy {
                            self.client = Some(client);
                            self.state = AppState::MainMenu;
                            // Trigger initial load
                            // We can't dispatch action here easily without a channel or just calling update?
                            // But update is async.
                            // Let's just create a task to fetch sessions?
                            // Or do it in the next Tick in MainMenu?
                            return;
                        }
                    }

                    // If not running and no process spawned, spawn it
                    if self.engine_process.is_none() {
                        // Find binary (assuming cargo run or in target)
                        // For dev: use cargo run
                        // For now, let's just try to spawn "tandem-engine" from path
                        if let Ok(child) = Command::new("tandem-engine")
                            .arg("serve")
                            .arg("--port")
                            .arg("3000")
                            .spawn()
                        {
                            self.engine_process = Some(child);
                        } else {
                            // Fallback for dev environment
                            if let Ok(child) = Command::new("cargo")
                                .arg("run")
                                .arg("-p")
                                .arg("tandem-engine")
                                .arg("--")
                                .arg("serve")
                                .spawn()
                            {
                                self.engine_process = Some(child);
                            }
                        }
                    }
                } else {
                    // We have a client but still connecting?
                    // Re-check health
                }
            }
            AppState::MainMenu => {
                // Poll for sessions if empty? Or refresh periodically?
                if self.tick_count % 20 == 0 {
                    // Every 5 seconds roughly (250ms * 20 = 5s)
                    if let Some(client) = &self.client {
                        if let Ok(sessions) = client.list_sessions().await {
                            self.sessions = sessions;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
