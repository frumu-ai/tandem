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
    CommandInput(char),
    SubmitCommand,
    ClearCommand,
    BackspaceCommand,
    SwitchToChat,
    Autocomplete,
    AutocompleteNext,
    AutocompletePrev,
    AutocompleteAccept,
    AutocompleteDismiss,
    BackToMenu,
    SetupNextStep,
    SetupPrevItem,
    SetupNextItem,
    SetupInput(char),
    SetupBackspace,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
}

use crate::net::client::Session;

#[derive(Debug, Clone, PartialEq)]
pub enum PinPromptMode {
    UnlockExisting,
    CreateNew,
    ConfirmNew { first_pin: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    StartupAnimation {
        frame: usize,
    },

    PinPrompt {
        input: String,
        error: Option<String>,
        mode: PinPromptMode,
    },
    MainMenu,
    Chat {
        session_id: String,
        command_input: String,
        messages: Vec<ChatMessage>,
        scroll_from_bottom: u16,
    },
    Connecting,
    SetupWizard {
        step: SetupStep,
        provider_catalog: Option<crate::net::client::ProviderCatalog>,
        selected_provider_index: usize,
        selected_model_index: usize,
        api_key_input: String,
        model_input: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetupStep {
    Welcome,
    SelectProvider,
    EnterApiKey,
    SelectModel,
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteMode {
    Command,
    Provider,
    Model,
}

use crate::net::client::EngineClient;
use serde_json::{json, Value};
use tandem_types::ModelSpec;
use tandem_wire::WireSessionMessage;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

use crate::crypto::{keystore::SecureKeyStore, vault::EncryptedVaultKey};
use std::path::PathBuf;
use std::process::Stdio;
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
    pub current_mode: TandemMode,
    pub current_provider: Option<String>,
    pub current_model: Option<String>,
    pub provider_catalog: Option<crate::net::client::ProviderCatalog>,
    pub connection_status: String,
    pub pending_model_provider: Option<String>,
    pub autocomplete_items: Vec<(String, String)>,
    pub autocomplete_index: usize,
    pub autocomplete_mode: AutocompleteMode,
    pub show_autocomplete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TandemMode {
    #[default]
    Ask,
    Coder,
    Explore,
    Immediate,
    Orchestrate,
    Plan,
}

impl TandemMode {
    pub fn as_agent(&self) -> &'static str {
        match self {
            TandemMode::Ask => "general",
            TandemMode::Coder => "build",
            TandemMode::Explore => "explore",
            TandemMode::Immediate => "immediate",
            TandemMode::Orchestrate => "orchestrate",
            TandemMode::Plan => "plan",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ask" => Some(TandemMode::Ask),
            "coder" => Some(TandemMode::Coder),
            "explore" => Some(TandemMode::Explore),
            "immediate" => Some(TandemMode::Immediate),
            "orchestrate" => Some(TandemMode::Orchestrate),
            "plan" => Some(TandemMode::Plan),
            _ => None,
        }
    }

    pub fn all_modes() -> Vec<(&'static str, &'static str)> {
        vec![
            ("ask", "General Q&A - uses general agent"),
            ("coder", "Code assistance - uses build agent"),
            ("explore", "Read-only exploration - uses explore agent"),
            (
                "immediate",
                "Execute without confirmation - uses immediate agent",
            ),
            (
                "orchestrate",
                "Multi-agent orchestration - uses orchestrate agent",
            ),
            (
                "plan",
                "Planning mode with write restrictions - uses plan agent",
            ),
        ]
    }
}

impl App {
    fn shared_engine_mode_enabled() -> bool {
        std::env::var("TANDEM_SHARED_ENGINE_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized == "0" || normalized == "false" || normalized == "off")
            })
            .unwrap_or(true)
    }

    const COMMANDS: &'static [&'static str] = &[
        "help",
        "engine",
        "sessions",
        "new",
        "use",
        "title",
        "prompt",
        "cancel",
        "messages",
        "modes",
        "mode",
        "providers",
        "provider",
        "models",
        "model",
        "keys",
        "key",
        "approve",
        "deny",
        "answer",
        "config",
    ];

    pub const COMMAND_HELP: &'static [(&'static str, &'static str)] = &[
        ("help", "Show available commands"),
        ("engine", "Engine status / restart"),
        ("sessions", "List all sessions"),
        ("new", "Create new session"),
        ("use", "Switch to session by ID"),
        ("title", "Rename current session"),
        ("prompt", "Send prompt to session"),
        ("cancel", "Cancel current operation"),
        ("messages", "Show message history"),
        ("modes", "List available modes"),
        ("mode", "Set or show current mode"),
        ("providers", "List available providers"),
        ("provider", "Set current provider"),
        ("models", "List models for provider"),
        ("model", "Set current model"),
        ("keys", "Show configured API keys"),
        ("key", "Manage provider API keys"),
        ("approve", "Approve a pending request"),
        ("deny", "Deny a pending request"),
        ("answer", "Answer a question"),
        ("config", "Show configuration"),
    ];

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
            current_mode: TandemMode::default(),
            current_provider: None,
            current_model: None,
            provider_catalog: None,
            connection_status: "Initializing...".to_string(),
            pending_model_provider: None,
            autocomplete_items: Vec::new(),
            autocomplete_index: 0,
            autocomplete_mode: AutocompleteMode::Command,
            show_autocomplete: false,
        }
    }

    fn update_autocomplete_for_input(&mut self, input: &str) {
        if !input.starts_with('/') {
            self.show_autocomplete = false;
            self.autocomplete_items.clear();
            return;
        }
        if let Some(rest) = input.strip_prefix("/provider") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let mut providers: Vec<String> = catalog.all.iter().map(|p| p.id.clone()).collect();
                providers.sort();
                let filtered: Vec<String> = if query.is_empty() {
                    providers
                } else {
                    providers
                        .into_iter()
                        .filter(|p| p.to_lowercase().contains(&query))
                        .collect()
                };
                self.autocomplete_items = filtered
                    .into_iter()
                    .map(|p| (p, "provider".to_string()))
                    .collect();
                self.autocomplete_index = 0;
                self.autocomplete_mode = AutocompleteMode::Provider;
                self.show_autocomplete = !self.autocomplete_items.is_empty();
                return;
            }
        }
        if let Some(rest) = input.strip_prefix("/model") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let provider_id = self.current_provider.as_deref().unwrap_or("");
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    let filtered: Vec<String> = if query.is_empty() {
                        model_ids
                    } else {
                        model_ids
                            .into_iter()
                            .filter(|m| m.to_lowercase().contains(&query))
                            .collect()
                    };
                    self.autocomplete_items = filtered
                        .into_iter()
                        .map(|m| (m, "model".to_string()))
                        .collect();
                    self.autocomplete_index = 0;
                    self.autocomplete_mode = AutocompleteMode::Model;
                    self.show_autocomplete = !self.autocomplete_items.is_empty();
                    return;
                }
            }
        }
        let cmd_part = input.trim_start_matches('/').to_lowercase();
        self.autocomplete_items = Self::COMMAND_HELP
            .iter()
            .filter(|(name, _)| name.starts_with(&cmd_part))
            .map(|(name, desc)| (name.to_string(), desc.to_string()))
            .collect();
        self.autocomplete_index = 0;
        self.autocomplete_mode = AutocompleteMode::Command;
        self.show_autocomplete = !self.autocomplete_items.is_empty();
    }

    fn model_ids_for_provider(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
    ) -> Vec<String> {
        if provider_index >= provider_catalog.all.len() {
            return Vec::new();
        }
        let provider = &provider_catalog.all[provider_index];
        let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
        model_ids.sort();
        model_ids
    }

    fn filtered_model_ids(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
        model_input: &str,
    ) -> Vec<String> {
        let model_ids = Self::model_ids_for_provider(provider_catalog, provider_index);
        if model_input.trim().is_empty() {
            return model_ids;
        }
        let query = model_input.trim().to_lowercase();
        model_ids
            .into_iter()
            .filter(|m| m.to_lowercase().contains(&query))
            .collect()
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

            AppState::Chat { .. } => {
                if self.show_autocomplete {
                    match key.code {
                        KeyCode::Esc => Some(Action::AutocompleteDismiss),
                        KeyCode::Enter | KeyCode::Tab => Some(Action::AutocompleteAccept),
                        KeyCode::Down | KeyCode::Char('j')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompleteNext)
                        }
                        KeyCode::Up | KeyCode::Char('k')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompletePrev)
                        }
                        KeyCode::Down => Some(Action::AutocompleteNext),
                        KeyCode::Up => Some(Action::AutocompletePrev),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                } else {
                    match key.code {
                        KeyCode::Esc => Some(Action::BackToMenu),
                        KeyCode::Enter => Some(Action::SubmitCommand),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Tab => Some(Action::Autocomplete),
                        KeyCode::Up => Some(Action::ScrollUp),
                        KeyCode::Down => Some(Action::ScrollDown),
                        KeyCode::PageUp => Some(Action::PageUp),
                        KeyCode::PageDown => Some(Action::PageDown),
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                }
            }

            AppState::SetupWizard { .. } => match key.code {
                KeyCode::Esc => Some(Action::Quit),
                KeyCode::Enter => Some(Action::SetupNextStep),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::SetupNextItem),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::SetupPrevItem),
                KeyCode::Char(c) => Some(Action::SetupInput(c)),
                KeyCode::Backspace => Some(Action::SetupBackspace),
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
                        mode: if self.vault_key.is_some() {
                            PinPromptMode::UnlockExisting
                        } else {
                            PinPromptMode::CreateNew
                        },
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
                    } else if c.is_ascii_digit() && input.len() < 8 {
                        input.push(c);
                    }
                }
            }
            Action::SubmitPin => {
                let (input, mode) = match &self.state {
                    AppState::PinPrompt { input, mode, .. } => (input.clone(), mode.clone()),
                    _ => (String::new(), PinPromptMode::UnlockExisting),
                };

                match mode {
                    PinPromptMode::UnlockExisting => {
                        match &self.vault_key {
                            Some(vk) => match vk.decrypt(&input) {
                                Ok(master_key) => {
                                    if let Some(config_dir) = &self.config_dir {
                                        let keystore_path = config_dir.join("tandem.keystore");
                                        match SecureKeyStore::load(&keystore_path, master_key) {
                                            Ok(store) => {
                                                // Ensure keystore file exists on disk for first-time users.
                                                if let Err(e) = store.save(&keystore_path) {
                                                    self.state = AppState::PinPrompt {
                                                        input: String::new(),
                                                        error: Some(format!(
                                                            "Failed to save keystore: {}",
                                                            e
                                                        )),
                                                        mode: PinPromptMode::UnlockExisting,
                                                    };
                                                    return Ok(());
                                                }
                                                self.keystore = Some(store);
                                                self.state = AppState::Connecting;
                                                return Ok(());
                                            }
                                            Err(_) => {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(
                                                        "Failed to load keystore".to_string(),
                                                    ),
                                                    mode: PinPromptMode::UnlockExisting,
                                                };
                                            }
                                        }
                                    } else {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some("Config dir not found".to_string()),
                                            mode: PinPromptMode::UnlockExisting,
                                        };
                                    }
                                }
                                Err(_) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some("Invalid PIN".to_string()),
                                        mode: PinPromptMode::UnlockExisting,
                                    };
                                }
                            },
                            None => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(
                                        "No vault key found. Create a new PIN.".to_string(),
                                    ),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::CreateNew => {
                        match crate::crypto::vault::validate_pin_format(&input) {
                            Ok(_) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: None,
                                    mode: PinPromptMode::ConfirmNew { first_pin: input },
                                };
                            }
                            Err(e) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(e.to_string()),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::ConfirmNew { first_pin } => {
                        if input != first_pin {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("PINs do not match. Enter a new PIN.".to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }

                        if let Some(config_dir) = &self.config_dir {
                            let vault_path = config_dir.join("vault.key");
                            let keystore_path = config_dir.join("tandem.keystore");
                            match EncryptedVaultKey::create(&input) {
                                Ok((vault_key, master_key)) => {
                                    if let Err(e) = vault_key.save(&vault_path) {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some(format!("Failed to save vault: {}", e)),
                                            mode: PinPromptMode::CreateNew,
                                        };
                                        return Ok(());
                                    }

                                    match SecureKeyStore::load(&keystore_path, master_key) {
                                        Ok(store) => {
                                            if let Err(e) = store.save(&keystore_path) {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(format!(
                                                        "Failed to save keystore: {}",
                                                        e
                                                    )),
                                                    mode: PinPromptMode::CreateNew,
                                                };
                                                return Ok(());
                                            }
                                            self.vault_key = Some(vault_key);
                                            self.keystore = Some(store);
                                            self.state = AppState::Connecting;
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            self.state = AppState::PinPrompt {
                                                input: String::new(),
                                                error: Some(format!(
                                                    "Failed to initialize keystore: {}",
                                                    e
                                                )),
                                                mode: PinPromptMode::CreateNew,
                                            };
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some(format!("Failed to create vault: {}", e)),
                                        mode: PinPromptMode::CreateNew,
                                    };
                                }
                            }
                        } else {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("Config dir not found".to_string()),
                                mode: PinPromptMode::CreateNew,
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

                    if let Ok(_) = client.create_session(Some("New session".to_string())).await {
                        // Refresh sessions
                        if let Ok(sessions) = client.list_sessions().await {
                            self.sessions = sessions;
                            // Select the new one (usually first or last depending on sort)
                            // server sorts by updated desc, so new one is first.
                            self.selected_session_index = 0;
                            if let Some(ref session) = self.sessions.first() {
                                self.state = AppState::Chat {
                                    session_id: session.id.clone(),
                                    command_input: String::new(),
                                    messages: Vec::new(),
                                    scroll_from_bottom: 0,
                                };
                            }
                        }
                    }
                }
            }

            Action::SelectSession => {
                if !self.sessions.is_empty() {
                    let session = &self.sessions[self.selected_session_index];
                    self.state = AppState::Chat {
                        session_id: session.id.clone(),
                        command_input: String::new(),
                        messages: Vec::new(),
                        scroll_from_bottom: 0,
                    };
                }
            }

            Action::CommandInput(c) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.push(c);
                    let input = command_input.clone();
                    self.update_autocomplete_for_input(&input);
                }
            }

            Action::BackspaceCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.pop();
                    let input = command_input.clone();
                    if input == "/" {
                        self.autocomplete_items = Self::COMMAND_HELP
                            .iter()
                            .map(|(name, desc)| (name.to_string(), desc.to_string()))
                            .collect();
                        self.autocomplete_index = 0;
                        self.autocomplete_mode = AutocompleteMode::Command;
                        self.show_autocomplete = true;
                    } else {
                        self.update_autocomplete_for_input(&input);
                    }
                }
            }

            Action::Autocomplete => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if !command_input.starts_with('/') {
                        command_input.clear();
                        command_input.push('/');
                    }
                    let input = command_input.clone();
                    self.update_autocomplete_for_input(&input);
                }
            }

            Action::AutocompleteNext => {
                if !self.autocomplete_items.is_empty() {
                    self.autocomplete_index =
                        (self.autocomplete_index + 1) % self.autocomplete_items.len();
                }
            }

            Action::AutocompletePrev => {
                if !self.autocomplete_items.is_empty() {
                    if self.autocomplete_index > 0 {
                        self.autocomplete_index -= 1;
                    } else {
                        self.autocomplete_index = self.autocomplete_items.len() - 1;
                    }
                }
            }

            Action::AutocompleteAccept => {
                if self.show_autocomplete && !self.autocomplete_items.is_empty() {
                    let (cmd, _) = self.autocomplete_items[self.autocomplete_index].clone();
                    if let AppState::Chat { command_input, .. } = &mut self.state {
                        command_input.clear();
                        match self.autocomplete_mode {
                            AutocompleteMode::Command => {
                                command_input.push_str(&format!("/{} ", cmd));
                            }
                            AutocompleteMode::Provider => {
                                command_input.push_str(&format!("/provider {}", cmd));
                            }
                            AutocompleteMode::Model => {
                                command_input.push_str(&format!("/model {}", cmd));
                            }
                        }
                    }
                    self.show_autocomplete = false;
                    self.autocomplete_items.clear();
                }
            }

            Action::AutocompleteDismiss => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
            }

            Action::BackToMenu => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
                self.state = AppState::MainMenu;
            }

            Action::SetupNextStep => {
                let mut persist_provider: Option<(String, Option<String>, Option<String>)> = None;
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    api_key_input,
                    model_input,
                } = &mut self.state
                {
                    match step.clone() {
                        SetupStep::Welcome => {
                            *step = SetupStep::SelectProvider;
                        }
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    *step = SetupStep::EnterApiKey;
                                }
                            } else {
                                *step = SetupStep::EnterApiKey;
                            }
                            model_input.clear();
                        }
                        SetupStep::EnterApiKey => {
                            if !api_key_input.is_empty() {
                                *step = SetupStep::SelectModel;
                            }
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let provider = &catalog.all[*selected_provider_index];
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    let model_id = if model_ids.is_empty() {
                                        if model_input.trim().is_empty() {
                                            None
                                        } else {
                                            Some(model_input.trim().to_string())
                                        }
                                    } else {
                                        model_ids.get(*selected_model_index).cloned()
                                    };
                                    let api_key = if api_key_input.is_empty() {
                                        None
                                    } else {
                                        Some(api_key_input.clone())
                                    };
                                    persist_provider =
                                        Some((provider.id.clone(), model_id, api_key));
                                }
                            }
                            *step = SetupStep::Complete;
                        }
                        SetupStep::Complete => {
                            // Transition to MainMenu or Chat
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some((provider_id, model_id, api_key)) = persist_provider {
                    self.current_provider = Some(provider_id.clone());
                    self.current_model = model_id.clone();
                    self.persist_provider_defaults(
                        &provider_id,
                        model_id.as_deref(),
                        api_key.as_deref(),
                    )
                    .await;
                }
            }

            Action::SetupPrevItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if *selected_provider_index > 0 {
                                *selected_provider_index -= 1;
                            }
                            *selected_model_index = 0;
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            if *selected_model_index > 0 {
                                *selected_model_index -= 1;
                            }
                            if let Some(catalog) = provider_catalog {
                                let model_count = Self::filtered_model_ids(
                                    catalog,
                                    *selected_provider_index,
                                    model_input,
                                )
                                .len();
                                if model_count == 0 {
                                    *selected_model_index = 0;
                                } else if *selected_model_index >= model_count {
                                    *selected_model_index = model_count.saturating_sub(1);
                                }
                            }
                        }
                        _ => {}
                    }

                    if let Some(catalog) = provider_catalog {
                        if *selected_provider_index >= catalog.all.len() {
                            *selected_provider_index = catalog.all.len().saturating_sub(1);
                        }
                    }
                }
            }

            Action::SetupNextItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() - 1 {
                                    *selected_provider_index += 1;
                                }
                            }
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let model_count = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    )
                                    .len();
                                    if model_count > 0 && *selected_model_index < model_count - 1 {
                                        *selected_model_index += 1;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::SetupInput(c) => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    if matches!(step, SetupStep::EnterApiKey) {
                        api_key_input.push(c);
                    } else if matches!(step, SetupStep::SelectModel) {
                        model_input.push(c);
                        if let Some(catalog) = provider_catalog {
                            let model_count = Self::filtered_model_ids(
                                catalog,
                                *selected_provider_index,
                                model_input,
                            )
                            .len();
                            if model_count == 0 {
                                *selected_model_index = 0;
                            } else if *selected_model_index >= model_count {
                                *selected_model_index = model_count.saturating_sub(1);
                            }
                        }
                    }
                }
            }

            Action::SetupBackspace => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    if matches!(step, SetupStep::EnterApiKey) {
                        api_key_input.pop();
                    } else if matches!(step, SetupStep::SelectModel) {
                        model_input.pop();
                        if let Some(catalog) = provider_catalog {
                            let model_count = Self::filtered_model_ids(
                                catalog,
                                *selected_provider_index,
                                model_input,
                            )
                            .len();
                            if model_count == 0 {
                                *selected_model_index = 0;
                            } else if *selected_model_index >= model_count {
                                *selected_model_index = model_count.saturating_sub(1);
                            }
                        }
                    }
                }
            }

            Action::ScrollUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(1);
                }
            }
            Action::ScrollDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(1);
                }
            }
            Action::PageUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(10);
                }
            }
            Action::PageDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(10);
                }
            }

            Action::ClearCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.clear();
                }
            }

            Action::SubmitCommand => {
                let (session_id, msg_to_send) = if let AppState::Chat {
                    session_id,
                    command_input,
                    ..
                } = &mut self.state
                {
                    if command_input.is_empty() {
                        return Ok(());
                    }
                    let msg = command_input.trim().to_string();
                    command_input.clear();
                    (session_id.clone(), Some(msg))
                } else {
                    (String::new(), None)
                };

                if let Some(msg) = msg_to_send {
                    if msg.starts_with('/') {
                        let response = self.execute_command(&msg).await;
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: response,
                            });
                        }
                    } else if let Some(provider_id) = self.pending_model_provider.clone() {
                        let model_id = msg.trim().to_string();
                        if model_id.is_empty() {
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: "Model cannot be empty. Paste a model name."
                                        .to_string(),
                                });
                            }
                        } else {
                            self.pending_model_provider = None;
                            self.current_provider = Some(provider_id.clone());
                            self.current_model = Some(model_id.clone());
                            self.persist_provider_defaults(&provider_id, Some(&model_id), None)
                                .await;
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!(
                                        "Provider set to {} with model {}.",
                                        provider_id, model_id
                                    ),
                                });
                            }
                        }
                    } else {
                        let agent = Some(self.current_mode.as_agent().to_string());
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::User,
                                content: msg.clone(),
                            });
                        }
                        if let Some(client) = &self.client {
                            let model = self.current_model_spec();
                            match client
                                .send_prompt(&session_id, &msg, agent.as_deref(), model)
                                .await
                            {
                                Ok(messages) => {
                                    if let Some(response) =
                                        Self::extract_assistant_message(&messages)
                                    {
                                        if let AppState::Chat { messages, .. } = &mut self.state {
                                            messages.push(ChatMessage {
                                                role: MessageRole::Assistant,
                                                content: response,
                                            });
                                        }
                                    }
                                }
                                Err(err) => {
                                    if let AppState::Chat { messages, .. } = &mut self.state {
                                        messages.push(ChatMessage {
                                            role: MessageRole::System,
                                            content: format!("Prompt failed: {}", err),
                                        });
                                    }
                                }
                            }
                        }
                    }
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
                // Continue matrix rain animation
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }

                // Try to connect or spawn
                if self.client.is_none() {
                    self.connection_status = "Searching for engine...".to_string();
                    // Check if running
                    let client = EngineClient::new("http://127.0.0.1:3000".to_string());
                    if let Ok(healthy) = client.check_health().await {
                        if healthy {
                            self.connection_status = "Connected! Loading...".to_string();
                            self.client = Some(client.clone());
                            // Check if providers are configured
                            if let Ok(providers) = client.list_providers().await {
                                self.provider_catalog = Some(providers.clone());
                                if providers.connected.is_empty() {
                                    // No provider keys configured, start setup wizard
                                    self.state = AppState::SetupWizard {
                                        step: SetupStep::Welcome,
                                        provider_catalog: Some(providers),
                                        selected_provider_index: 0,
                                        selected_model_index: 0,
                                        api_key_input: String::new(),
                                        model_input: String::new(),
                                    };
                                    return;
                                }
                            }
                            let config = client.config_providers().await.ok();
                            self.apply_provider_defaults(config.as_ref());
                            if let Ok(sessions) = client.list_sessions().await {
                                self.sessions = sessions;
                            }
                            self.state = AppState::MainMenu;
                            return;
                        }
                    }

                    // If not running and no process spawned, spawn it
                    if self.engine_process.is_none() {
                        self.connection_status = "Starting engine...".to_string();
                        // Find binary (assuming cargo run or in target)
                        // For dev: use cargo run
                        // For now, let's just try to spawn "tandem-engine" from path
                        let mut cmd = Command::new("tandem-engine");
                        cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                        cmd.arg("serve").arg("--port").arg("3000");
                        cmd.stdout(Stdio::null()).stderr(Stdio::null());
                        if let Ok(child) = cmd.spawn() {
                            self.engine_process = Some(child);
                        } else {
                            // Fallback for dev environment
                            let mut cargo_cmd = Command::new("cargo");
                            cargo_cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cargo_cmd
                                .arg("run")
                                .arg("-p")
                                .arg("tandem-engine")
                                .arg("--")
                                .arg("serve");
                            cargo_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cargo_cmd.spawn() {
                                self.engine_process = Some(child);
                            }
                        }
                    } else {
                        self.connection_status = "Waiting for engine...".to_string();
                    }
                } else {
                    // We have a client but still connecting?
                    // Re-check health
                }
            }
            AppState::MainMenu | AppState::Chat { .. } => {
                if self.tick_count % 63 == 0 {
                    if let Some(client) = &self.client {
                        if let AppState::MainMenu = self.state {
                            if let Ok(sessions) = client.list_sessions().await {
                                self.sessions = sessions;
                            }
                        }
                        if self.provider_catalog.is_none() {
                            if let Ok(catalog) = client.list_providers().await {
                                self.provider_catalog = Some(catalog);
                            }
                        }
                        if (self.current_provider.is_none() || self.current_model.is_none())
                            && self.provider_catalog.is_some()
                        {
                            let config = client.config_providers().await.ok();
                            self.apply_provider_defaults(config.as_ref());
                        }
                    }
                }
            }

            _ => {}
        }
    }

    pub async fn execute_command(&mut self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return "Unknown command. Type /help for available commands.".to_string();
        }

        let cmd_name = &parts[0][1..];
        let args = &parts[1..];

        match cmd_name.to_lowercase().as_str() {
            "help" => {
                let help_text = r#"Tandem TUI Commands:

BASICS:
  /help              Show this help message
  /engine status     Check engine connection status
  /engine restart    Restart the Tandem engine

SESSIONS:
  /sessions          List all sessions
  /new [title...]    Create new session
  /use <session_id> Switch to session
  /title <new title> Rename current session
  /prompt <text>    Send prompt to current session
  /cancel           Cancel current operation
  /messages [limit] Show session messages

MODES:
  /modes             List available modes
  /mode <name>       Set mode (ask|coder|explore|immediate|orchestrate|plan)
  /mode              Show current mode

PROVIDERS & MODELS:
  /providers         List available providers
  /provider <id>     Set current provider
  /models [provider] List models for provider
  /model <model_id>  Set current model

KEYS:
  /keys              Show configured providers
  /key set <provider> Add/update provider key
  /key remove <provider> Remove provider key
  /key test <provider> Test provider connection

APPROVALS:
  /approve <id> [once|always] Approve request
  /deny <id> [message...]   Deny request
  /answer <id> <text>       Answer question

CONFIG:
  /config            Show configuration"#;
                help_text.to_string()
            }

            "engine" => match args.get(0).map(|s| *s) {
                Some("status") => {
                    if let Some(client) = &self.client {
                        match client.get_engine_status().await {
                            Ok(status) => {
                                format!(
                                    "Engine Status:\n  Healthy: {}\n  Version: {}\n  Mode: {}\n  Endpoint: {}",
                                    if status.healthy { "Yes" } else { "No" },
                                    status.version,
                                    status.mode,
                                    "http://127.0.0.1:3000"
                                )
                            }
                            Err(e) => format!("Failed to get engine status: {}", e),
                        }
                    } else {
                        "Engine: Not connected".to_string()
                    }
                }
                Some("restart") => {
                    self.connection_status = "Restarting engine...".to_string();
                    self.stop_engine_process().await;
                    self.client = None;
                    self.provider_catalog = None;
                    sleep(std::time::Duration::from_millis(300)).await;
                    self.state = AppState::Connecting;
                    "Engine restart requested.".to_string()
                }
                _ => "Usage: /engine status | restart".to_string(),
            },

            "sessions" => {
                if self.sessions.is_empty() {
                    "No sessions found.".to_string()
                } else {
                    let lines: Vec<String> = self
                        .sessions
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let marker = if i == self.selected_session_index {
                                "→ "
                            } else {
                                "  "
                            };
                            format!("{}{} (ID: {})", marker, s.title, s.id)
                        })
                        .collect();
                    format!("Sessions:\n{}", lines.join("\n"))
                }
            }

            "new" => {
                let title = if args.is_empty() {
                    None
                } else {
                    Some(args.join(" ").trim().to_string())
                };
                let title_for_display = title.clone().unwrap_or_else(|| "New Session".to_string());
                if let Some(client) = &self.client {
                    match client.create_session(title).await {
                        Ok(session) => {
                            self.sessions.push(session.clone());
                            self.selected_session_index = self.sessions.len() - 1;
                            format!(
                                "Created session: {} (ID: {})",
                                title_for_display, session.id
                            )
                        }
                        Err(e) => format!("Failed to create session: {}", e),
                    }
                } else {
                    "Not connected to engine".to_string()
                }
            }

            "use" => {
                if args.is_empty() {
                    return "Usage: /use <session_id>".to_string();
                }
                let target_id = args[0];
                if let Some(idx) = self.sessions.iter().position(|s| s.id == target_id) {
                    self.selected_session_index = idx;
                    if let AppState::Chat { session_id, .. } = &mut self.state {
                        *session_id = target_id.to_string();
                    }
                    format!("Switched to session: {}", target_id)
                } else {
                    format!("Session not found: {}", target_id)
                }
            }

            "mode" => {
                if args.is_empty() {
                    let agent = self.current_mode.as_agent();
                    return format!("Current mode: {:?} (agent: {})", self.current_mode, agent);
                }
                let mode_name = args[0];
                if let Some(mode) = TandemMode::from_str(mode_name) {
                    self.current_mode = mode;
                    format!("Mode set to: {:?}", mode)
                } else {
                    format!(
                        "Unknown mode: {}. Use /modes to see available modes.",
                        mode_name
                    )
                }
            }

            "modes" => {
                let lines: Vec<String> = TandemMode::all_modes()
                    .iter()
                    .map(|(name, desc)| format!("  {} - {}", name, desc))
                    .collect();
                format!("Available modes:\n{}", lines.join("\n"))
            }

            "providers" => {
                if let Some(catalog) = &self.provider_catalog {
                    let lines: Vec<String> = catalog
                        .all
                        .iter()
                        .map(|p| {
                            let status = if catalog.connected.contains(&p.id) {
                                "connected"
                            } else {
                                "not configured"
                            };
                            format!("  {} - {}", p.id, status)
                        })
                        .collect();
                    if lines.is_empty() {
                        "No providers available.".to_string()
                    } else {
                        format!("Available providers:\n{}", lines.join("\n"))
                    }
                } else {
                    "Loading providers... (use /providers to refresh)".to_string()
                }
            }

            "provider" => {
                if args.is_empty() {
                    return format!(
                        "Current provider: {}",
                        self.current_provider.as_deref().unwrap_or("none")
                    );
                }
                let provider_id = args[0];
                if let Some(catalog) = &self.provider_catalog {
                    if catalog.all.iter().any(|p| p.id == provider_id) {
                        self.current_provider = Some(provider_id.to_string());
                        self.persist_provider_defaults(provider_id, None, None)
                            .await;
                        self.pending_model_provider = Some(provider_id.to_string());
                        let model_list = catalog
                            .all
                            .iter()
                            .find(|p| p.id == provider_id)
                            .map(|provider| {
                                let mut model_ids: Vec<String> =
                                    provider.models.keys().cloned().collect();
                                model_ids.sort();
                                if model_ids.is_empty() {
                                    "No models listed. Paste a model name to use it.".to_string()
                                } else {
                                    format!(
                                        "Models:\n{}",
                                        model_ids
                                            .iter()
                                            .map(|m| format!("  {}", m))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    )
                                }
                            })
                            .unwrap_or_else(|| "Provider not found.".to_string());
                        format!(
                            "Provider set to: {}\n{}\n\nPaste a model name to select it.",
                            provider_id, model_list
                        )
                    } else {
                        format!(
                            "Unknown provider: {}. Use /providers to see available.",
                            provider_id
                        )
                    }
                } else {
                    self.current_provider = Some(provider_id.to_string());
                    self.persist_provider_defaults(provider_id, None, None)
                        .await;
                    self.pending_model_provider = Some(provider_id.to_string());
                    format!(
                        "Provider set to: {} (will validate on connect)\nPaste a model name to select it.",
                        provider_id
                    )
                }
            }

            "models" => {
                let provider_id = args
                    .first()
                    .map(|s| s.to_string())
                    .or_else(|| self.current_provider.clone());
                if let Some(catalog) = &self.provider_catalog {
                    if let Some(pid) = &provider_id {
                        if let Some(provider) = catalog.all.iter().find(|p| p.id == *pid) {
                            let model_ids: Vec<String> = provider.models.keys().cloned().collect();
                            if model_ids.is_empty() {
                                format!("No models available for provider: {}", pid)
                            } else {
                                format!(
                                    "Models for {}:\n{}",
                                    pid,
                                    model_ids
                                        .iter()
                                        .map(|m| format!("  {}", m))
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                )
                            }
                        } else {
                            format!("Provider not found: {}", pid)
                        }
                    } else {
                        "No provider selected. Use /provider <id> first.".to_string()
                    }
                } else {
                    "Loading providers... (use /providers to refresh)".to_string()
                }
            }

            "model" => {
                if args.is_empty() {
                    return format!(
                        "Current model: {}",
                        self.current_model.as_deref().unwrap_or("none")
                    );
                }
                let model_id = args.join(" ");
                self.current_model = Some(model_id.clone());
                self.pending_model_provider = None;
                if let Some(provider_id) = self.current_provider.clone() {
                    self.persist_provider_defaults(&provider_id, Some(&model_id), None)
                        .await;
                }
                format!("Model set to: {}", model_id)
            }

            "keys" => {
                if let Some(keystore) = &self.keystore {
                    let provider_ids = keystore.list_keys();
                    if provider_ids.is_empty() {
                        "No provider keys configured.".to_string()
                    } else {
                        format!(
                            "Configured providers:\n{}",
                            provider_ids
                                .iter()
                                .map(|p| format!("  {} - configured", p))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    }
                } else {
                    "Keystore not unlocked. Enter PIN to access keys.".to_string()
                }
            }

            "key" => match args.get(0).map(|s| *s) {
                Some("set") => {
                    if args.len() < 2 {
                        return "Usage: /key set <provider_id>".to_string();
                    }
                    let provider_id = args[1];
                    format!(
                        "Interactive key entry not implemented. Provider: {}",
                        provider_id
                    )
                }
                Some("remove") => {
                    if args.len() < 2 {
                        return "Usage: /key remove <provider_id>".to_string();
                    }
                    let provider_id = args[1];
                    format!("Key removal not implemented. Provider: {}", provider_id)
                }
                Some("test") => {
                    if args.len() < 2 {
                        return "Usage: /key test <provider_id>".to_string();
                    }
                    let provider_id = args[1];
                    if let Some(client) = &self.client {
                        if let Ok(catalog) = client.list_providers().await {
                            let is_connected = catalog.connected.contains(&provider_id.to_string());
                            if catalog.all.iter().any(|p| p.id == provider_id) {
                                if is_connected {
                                    return format!(
                                        "Provider {}: Connected and working!",
                                        provider_id
                                    );
                                } else {
                                    return format!("Provider {}: Not connected. Use /key set to add credentials.", provider_id);
                                }
                            }
                        }
                    }
                    format!("Provider {}: Not connected or not available.", provider_id)
                }
                _ => "Usage: /key set|remove|test <provider_id>".to_string(),
            },

            "cancel" => "Cancel not implemented yet.".to_string(),

            "messages" => {
                let limit = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);
                format!("Message history not implemented yet. (limit: {})", limit)
            }

            "prompt" => {
                let text = args.join(" ");
                if text.is_empty() {
                    return "Usage: /prompt <text...>".to_string();
                }
                let (session_id, should_send) = if let AppState::Chat {
                    session_id,
                    messages,
                    ..
                } = &mut self.state
                {
                    messages.push(ChatMessage {
                        role: MessageRole::User,
                        content: text.clone(),
                    });
                    (session_id.clone(), true)
                } else {
                    (String::new(), false)
                };

                if !should_send {
                    return "Not in a chat session. Use /use <session_id> first.".to_string();
                }

                let agent = Some(self.current_mode.as_agent().to_string());
                if let Some(client) = &self.client {
                    let model = self.current_model_spec();
                    match client
                        .send_prompt(&session_id, &text, agent.as_deref(), model)
                        .await
                    {
                        Ok(messages) => {
                            if let Some(response) = Self::extract_assistant_message(&messages) {
                                if let AppState::Chat { messages, .. } = &mut self.state {
                                    messages.push(ChatMessage {
                                        role: MessageRole::Assistant,
                                        content: response,
                                    });
                                }
                            }
                        }
                        Err(err) => {
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: format!("Prompt failed: {}", err),
                                });
                            }
                        }
                    }
                }
                "Prompt sent.".to_string()
            }

            "title" => {
                let new_title = args.join(" ");
                if new_title.is_empty() {
                    return "Usage: /title <new title...>".to_string();
                }
                if let AppState::Chat { session_id, .. } = &mut self.state {
                    if let Some(client) = &self.client {
                        let req = crate::net::client::UpdateSessionRequest {
                            title: Some(new_title.clone()),
                            ..Default::default()
                        };
                        if let Ok(_session) = client.update_session(session_id, req).await {
                            if let Some(s) = self.sessions.iter_mut().find(|s| &s.id == session_id)
                            {
                                s.title = new_title.clone();
                            }
                            return format!("Session renamed to: {}", new_title);
                        }
                    }
                    "Failed to rename session.".to_string()
                } else {
                    "Not in a chat session.".to_string()
                }
            }

            "config" => {
                let lines = vec![
                    format!(
                        "Engine URL: {}",
                        self.client
                            .as_ref()
                            .map(|c| c.base_url())
                            .unwrap_or(&"not connected")
                    ),
                    format!("Sessions: {}", self.sessions.len()),
                    format!("Current Mode: {:?}", self.current_mode),
                    format!(
                        "Current Provider: {}",
                        self.current_provider.as_deref().unwrap_or("none")
                    ),
                    format!(
                        "Current Model: {}",
                        self.current_model.as_deref().unwrap_or("none")
                    ),
                ];
                format!("Configuration:\n{}", lines.join("\n"))
            }

            "approve" | "deny" | "answer" => {
                format!("{} not implemented yet.", cmd_name)
            }

            _ => format!(
                "Unknown command: {}. Type /help for available commands.",
                cmd_name
            ),
        }
    }

    fn current_model_spec(&self) -> Option<ModelSpec> {
        let provider_id = self.current_provider.as_ref()?.to_string();
        let model_id = self.current_model.as_ref()?.to_string();
        Some(ModelSpec {
            provider_id,
            model_id,
        })
    }

    fn extract_assistant_message(messages: &[WireSessionMessage]) -> Option<String> {
        let message = messages
            .iter()
            .rev()
            .find(|msg| msg.info.role.eq_ignore_ascii_case("assistant"))?;
        let parts = message
            .parts
            .iter()
            .filter_map(|part: &serde_json::Value| {
                if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                    part.get("text")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    async fn persist_provider_defaults(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
        api_key: Option<&str>,
    ) {
        let Some(client) = &self.client else {
            return;
        };
        let mut patch = serde_json::Map::new();
        patch.insert("default_provider".to_string(), json!(provider_id));
        if model_id.is_some() || api_key.is_some() {
            let mut provider_patch = serde_json::Map::new();
            if let Some(model_id) = model_id {
                provider_patch.insert("default_model".to_string(), json!(model_id));
            }
            if let Some(api_key) = api_key {
                provider_patch.insert("api_key".to_string(), json!(api_key));
            }
            let mut providers = serde_json::Map::new();
            providers.insert(provider_id.to_string(), Value::Object(provider_patch));
            patch.insert("providers".to_string(), Value::Object(providers));
        }
        let _ = client.patch_config(Value::Object(patch)).await;
    }

    fn apply_provider_defaults(
        &mut self,
        config: Option<&crate::net::client::ConfigProvidersResponse>,
    ) {
        let Some(catalog) = self.provider_catalog.as_ref() else {
            return;
        };

        let connected = if catalog.connected.is_empty() {
            catalog
                .all
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<String>>()
        } else {
            catalog.connected.clone()
        };

        let default_provider = catalog
            .default
            .clone()
            .filter(|id| connected.contains(id))
            .or_else(|| {
                config
                    .and_then(|cfg| cfg.default.clone())
                    .filter(|id| connected.contains(id))
            })
            .or_else(|| connected.first().cloned())
            .or_else(|| catalog.all.first().map(|p| p.id.clone()));

        let provider_invalid = self
            .current_provider
            .as_ref()
            .map(|id| !catalog.all.iter().any(|p| p.id == *id))
            .unwrap_or(true);
        let provider_unusable = self
            .current_provider
            .as_ref()
            .map(|id| !connected.contains(id))
            .unwrap_or(true);

        if provider_invalid || provider_unusable {
            self.current_provider = default_provider;
        } else if self.current_provider.is_none() {
            self.current_provider = default_provider;
        }

        let model_needs_reset = self.current_model.is_none()
            || self
                .current_provider
                .as_ref()
                .and_then(|provider_id| {
                    catalog
                        .all
                        .iter()
                        .find(|p| p.id == *provider_id)
                        .map(|provider| {
                            !self
                                .current_model
                                .as_ref()
                                .map(|m| provider.models.contains_key(m))
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(true);

        if model_needs_reset {
            if let Some(provider_id) = self.current_provider.clone() {
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let default_model = config
                        .and_then(|cfg| cfg.providers.get(&provider_id))
                        .and_then(|p| p.default_model.clone())
                        .filter(|id| provider.models.contains_key(id));
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    self.current_model = default_model.or_else(|| model_ids.first().cloned());
                }
            }
        }
    }

    async fn stop_engine_process(&mut self) {
        let Some(mut child) = self.engine_process.take() else {
            return;
        };

        let pid = child.id();
        let _ = child.start_kill();
        let _ = timeout(std::time::Duration::from_secs(2), child.wait()).await;

        #[cfg(windows)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }

        #[cfg(unix)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }

    pub async fn shutdown(&mut self) {
        if Self::shared_engine_mode_enabled() {
            // Shared mode: detach and let the engine continue serving other clients.
            let _ = self.engine_process.take();
            return;
        }
        self.stop_engine_process().await;
    }
}
