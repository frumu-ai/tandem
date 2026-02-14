use crossterm::{
    event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Tick,
    Quit,
    CtrlCPressed,
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
    InsertNewline,
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
    ToggleTaskPin(String),
    PromptSuccess {
        session_id: String,
        agent_id: String,
        messages: Vec<ChatMessage>,
    },
    PromptDelta {
        session_id: String,
        agent_id: String,
        delta: String,
    },
    PromptInfo {
        session_id: String,
        agent_id: String,
        message: String,
    },
    PromptRequest {
        session_id: String,
        agent_id: String,
        request: PendingRequestKind,
    },
    PromptRequestResolved {
        session_id: String,
        agent_id: String,
        request_id: String,
        reply: String,
    },
    PromptFailure {
        session_id: String,
        agent_id: String,
        error: String,
    },
    PromptRunStarted {
        session_id: String,
        agent_id: String,
        run_id: Option<String>,
    },
    NewAgent,
    CloseActiveAgent,
    SwitchAgentNext,
    SwitchAgentPrev,
    SelectAgentByNumber(usize),
    ToggleUiMode,
    GridPageNext,
    GridPagePrev,
    ShowHelpModal,
    CloseModal,
    OpenRequestCenter,
    RequestSelectNext,
    RequestSelectPrev,
    RequestOptionNext,
    RequestOptionPrev,
    RequestToggleCurrent,
    RequestConfirm,
    RequestDigit(u8),
    RequestInput(char),
    RequestBackspace,
    RequestReject,
    ConfirmCloseAgent(bool),
    CancelActiveAgent,
    StartDemoStream,
    SpawnBackgroundDemo,
    OpenDocs,
}

use crate::net::client::Session;

#[derive(Debug, Clone, PartialEq)]
pub enum PinPromptMode {
    UnlockExisting,
    CreateNew,
    ConfirmNew { first_pin: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Focus,
    Grid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Streaming,
    Cancelling,
    Done,
    Error,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModalState {
    Help,
    ConfirmCloseAgent { target_agent_id: String },
    RequestCenter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestionDraft {
    pub header: String,
    pub question: String,
    pub options: Vec<crate::net::client::QuestionChoice>,
    pub multiple: bool,
    pub custom: bool,
    pub selected_options: Vec<usize>,
    pub custom_input: String,
    pub option_cursor: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingQuestionRequest {
    pub id: String,
    pub questions: Vec<QuestionDraft>,
    pub question_index: usize,
    pub permission_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingPermissionRequest {
    pub id: String,
    pub tool: String,
    pub args: Option<Value>,
    pub args_source: Option<String>,
    pub args_integrity: Option<String>,
    pub query: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingRequestKind {
    Permission(PendingPermissionRequest),
    Question(PendingQuestionRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingRequest {
    pub session_id: String,
    pub agent_id: String,
    pub kind: PendingRequestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentPane {
    pub agent_id: String,
    pub session_id: String,
    pub draft: String,
    pub messages: Vec<ChatMessage>,
    pub scroll_from_bottom: u16,
    pub tasks: Vec<Task>,
    pub active_task_id: Option<String>,
    pub status: AgentStatus,
    pub active_run_id: Option<String>,
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
        tasks: Vec<Task>,
        active_task_id: Option<String>,
        agents: Vec<AgentPane>,
        active_agent_index: usize,
        ui_mode: UiMode,
        grid_page: usize,
        modal: Option<ModalState>,
        pending_requests: Vec<PendingRequest>,
        request_cursor: usize,
        permission_choice: usize,
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
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(String),
    Code { language: String, code: String },
    ToolCall(ToolCallInfo),
    ToolResult(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Working,
    Done,
    Failed,
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
use std::time::Instant;
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
    pub engine_health: EngineConnectionStatus,
    pub engine_lease_id: Option<String>,
    pub engine_lease_last_renewed: Option<Instant>,
    pub pending_model_provider: Option<String>,
    pub autocomplete_items: Vec<(String, String)>,
    pub autocomplete_index: usize,
    pub autocomplete_mode: AutocompleteMode,
    pub show_autocomplete: bool,
    pub action_tx: Option<tokio::sync::mpsc::UnboundedSender<Action>>,
    pub quit_armed_at: Option<Instant>,
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

const SCROLL_LINE_STEP: u16 = 3;
const SCROLL_PAGE_STEP: u16 = 20;

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
    fn provider_is_connected(&self, provider_id: &str) -> bool {
        self.provider_catalog
            .as_ref()
            .map(|c| c.connected.iter().any(|p| p == provider_id))
            .unwrap_or(false)
    }

    fn open_key_wizard_for_provider(&mut self, provider_id: &str) -> bool {
        let mut selected_provider_index = 0usize;
        let mut found = false;
        if let Some(catalog) = &self.provider_catalog {
            if let Some(idx) = catalog.all.iter().position(|p| p.id == provider_id) {
                selected_provider_index = idx;
                found = true;
            }
        }
        if !found {
            return false;
        }
        self.state = AppState::SetupWizard {
            step: SetupStep::EnterApiKey,
            provider_catalog: self.provider_catalog.clone(),
            selected_provider_index,
            selected_model_index: 0,
            api_key_input: String::new(),
            model_input: String::new(),
        };
        true
    }

    async fn sync_keystore_keys_to_engine(&self, client: &EngineClient) -> usize {
        let Some(keystore) = &self.keystore else {
            return 0;
        };
        let mut providers = serde_json::Map::new();
        let mut synced = 0usize;
        for key_name in keystore.list_keys() {
            if let Ok(Some(api_key)) = keystore.get(&key_name) {
                if api_key.trim().is_empty() {
                    continue;
                }
                let provider_id = Self::normalize_provider_id_from_keystore_key(&key_name);
                providers.insert(provider_id, json!({ "api_key": api_key }));
                synced += 1;
            }
        }
        if synced == 0 {
            return 0;
        }
        let _ = client
            .patch_config(json!({
                "providers": providers
            }))
            .await;
        synced
    }

    fn normalize_provider_id_from_keystore_key(key: &str) -> String {
        let trimmed = key.trim();
        if let Some(rest) = trimmed.strip_prefix("opencode_") {
            if let Some(provider) = rest.strip_suffix("_api_key") {
                return provider.to_string();
            }
        }
        if let Some(provider) = trimmed.strip_suffix("_api_key") {
            return provider.to_string();
        }
        if let Some(provider) = trimmed.strip_suffix("_key") {
            return provider.to_string();
        }
        trimmed.to_string()
    }

    fn save_provider_key_local(&mut self, provider_id: &str, api_key: &str) {
        let Some(keystore) = &mut self.keystore else {
            return;
        };
        if keystore.set(provider_id, api_key.to_string()).is_ok() {
            if let Some(config_dir) = &self.config_dir {
                let _ = keystore.save(config_dir.join("tandem.keystore"));
            }
        }
    }

    fn shared_engine_mode_enabled() -> bool {
        std::env::var("TANDEM_SHARED_ENGINE_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized == "0" || normalized == "false" || normalized == "off")
            })
            .unwrap_or(true)
    }

    pub const COMMAND_HELP: &'static [(&'static str, &'static str)] = &[
        ("help", "Show available commands"),
        ("engine", "Engine status / restart"),
        ("sessions", "List all sessions"),
        ("new", "Create new session"),
        ("agent", "Manage in-chat agents"),
        ("use", "Switch to session by ID"),
        ("title", "Rename current session"),
        ("prompt", "Send prompt to session"),
        ("cancel", "Cancel current operation"),
        ("last_error", "Show last prompt/system error"),
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
        ("requests", "Open pending request center"),
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
            engine_health: EngineConnectionStatus::Disconnected,
            engine_lease_id: None,
            engine_lease_last_renewed: None,
            pending_model_provider: None,
            autocomplete_items: Vec::new(),
            autocomplete_index: 0,
            autocomplete_mode: AutocompleteMode::Command,
            show_autocomplete: false,
            action_tx: None,
            quit_armed_at: None,
        }
    }

    fn make_agent_pane(agent_id: String, session_id: String) -> AgentPane {
        AgentPane {
            agent_id,
            session_id,
            draft: String::new(),
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::new(),
            active_task_id: None,
            status: AgentStatus::Idle,
            active_run_id: None,
        }
    }

    fn active_agent_clone(&self) -> Option<AgentPane> {
        if let AppState::Chat {
            agents,
            active_agent_index,
            ..
        } = &self.state
        {
            return agents.get(*active_agent_index).cloned();
        }
        None
    }

    fn sync_chat_from_active_agent(&mut self) {
        if let AppState::Chat {
            session_id,
            command_input,
            messages,
            scroll_from_bottom,
            tasks,
            active_task_id,
            agents,
            active_agent_index,
            ..
        } = &mut self.state
        {
            if let Some(agent) = agents.get(*active_agent_index) {
                *session_id = agent.session_id.clone();
                *command_input = agent.draft.clone();
                *messages = agent.messages.clone();
                *scroll_from_bottom = agent.scroll_from_bottom;
                *tasks = agent.tasks.clone();
                *active_task_id = agent.active_task_id.clone();
            }
        }
    }

    fn sync_active_agent_from_chat(&mut self) {
        if let AppState::Chat {
            session_id,
            command_input,
            messages,
            scroll_from_bottom,
            tasks,
            active_task_id,
            agents,
            active_agent_index,
            ..
        } = &mut self.state
        {
            if let Some(agent) = agents.get_mut(*active_agent_index) {
                agent.session_id = session_id.clone();
                agent.draft = command_input.clone();
                agent.messages = messages.clone();
                agent.scroll_from_bottom = *scroll_from_bottom;
                agent.tasks = tasks.clone();
                agent.active_task_id = active_task_id.clone();
            }
        }
    }

    fn active_chat_identity(&self) -> Option<(String, String)> {
        if let AppState::Chat {
            agents,
            active_agent_index,
            ..
        } = &self.state
        {
            let agent = agents.get(*active_agent_index)?;
            return Some((agent.session_id.clone(), agent.agent_id.clone()));
        }
        None
    }

    fn request_matches_active(&self, session_id: &str, agent_id: &str) -> bool {
        self.active_chat_identity()
            .map(|(active_session, active_agent)| {
                active_session == session_id && active_agent == agent_id
            })
            .unwrap_or(false)
    }

    fn open_request_center_if_needed(&mut self) {
        if let AppState::Chat {
            pending_requests,
            modal,
            request_cursor,
            ..
        } = &mut self.state
        {
            if pending_requests.is_empty() {
                *modal = None;
                *request_cursor = 0;
                return;
            }
            if *request_cursor >= pending_requests.len() {
                *request_cursor = pending_requests.len().saturating_sub(1);
            }
            if modal.is_none() {
                *modal = Some(ModalState::RequestCenter);
            }
        }
    }

    pub(crate) fn pending_request_counts(&self) -> (usize, usize) {
        if let AppState::Chat {
            pending_requests, ..
        } = &self.state
        {
            let active = self.active_chat_identity();
            if let Some((active_session, active_agent)) = active {
                let active_count = pending_requests
                    .iter()
                    .filter(|r| r.session_id == active_session && r.agent_id == active_agent)
                    .count();
                let background_count = pending_requests.len().saturating_sub(active_count);
                return (active_count, background_count);
            }
            return (0, pending_requests.len());
        }
        (0, 0)
    }

    async fn finalize_connecting(&mut self, client: &EngineClient) -> bool {
        if self.engine_lease_id.is_none() {
            self.acquire_engine_lease().await;
            let synced = self.sync_keystore_keys_to_engine(client).await;
            if synced > 0 {
                self.connection_status = format!("Synced {} provider key(s)...", synced);
            }
        }

        let providers = match client.list_providers().await {
            Ok(providers) => {
                self.provider_catalog = Some(providers.clone());
                providers
            }
            Err(_) => {
                self.connection_status = "Connected. Loading providers...".to_string();
                return false;
            }
        };

        if providers.connected.is_empty() {
            self.state = AppState::SetupWizard {
                step: SetupStep::Welcome,
                provider_catalog: Some(providers),
                selected_provider_index: 0,
                selected_model_index: 0,
                api_key_input: String::new(),
                model_input: String::new(),
            };
            return true;
        }

        let config = client.config_providers().await.ok();
        self.apply_provider_defaults(config.as_ref());

        match client.list_sessions().await {
            Ok(sessions) => {
                self.sessions = sessions;
                self.connection_status = "Engine ready. Loading sessions...".to_string();
                self.state = AppState::MainMenu;
                true
            }
            Err(_) => {
                self.connection_status = "Connected. Loading sessions...".to_string();
                false
            }
        }
    }

    async fn cancel_agent_if_running(&mut self, agent_index: usize) {
        let (session_id, run_id) = if let AppState::Chat { agents, .. } = &self.state {
            if let Some(agent) = agents.get(agent_index) {
                (agent.session_id.clone(), agent.active_run_id.clone())
            } else {
                return;
            }
        } else {
            return;
        };

        if let Some(client) = &self.client {
            if let Some(run_id) = run_id.as_deref() {
                let _ = client.cancel_run_by_id(&session_id, run_id).await;
            } else {
                let _ = client.abort_session(&session_id).await;
            }
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
        // Global control keys
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => {
                    return match self.state {
                        AppState::Chat { .. } => Some(Action::CancelActiveAgent),
                        _ => Some(Action::CtrlCPressed),
                    };
                }
                KeyCode::Char('x') => return Some(Action::Quit),
                KeyCode::Char('n') => return Some(Action::NewAgent),
                KeyCode::Char('w') => return Some(Action::CloseActiveAgent),
                KeyCode::Char('u') => return Some(Action::PageUp),
                KeyCode::Char('d') => return Some(Action::PageDown),
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
                KeyCode::Backspace => Some(Action::EnterPin('\x08')),
                KeyCode::Char(c) if c.is_ascii_digit() => Some(Action::EnterPin(c)),
                _ => None,
            },
            AppState::Connecting => {
                // Ignore typing while engine is loading.
                None
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
                if let AppState::Chat { modal, .. } = &self.state {
                    if let Some(active_modal) = modal {
                        return match key.code {
                            KeyCode::Esc => Some(Action::CloseModal),
                            KeyCode::Enter if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestConfirm)
                            }
                            KeyCode::Up if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestSelectPrev)
                            }
                            KeyCode::Down if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestSelectNext)
                            }
                            KeyCode::Left if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestOptionPrev)
                            }
                            KeyCode::Right if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestOptionNext)
                            }
                            KeyCode::Backspace
                                if matches!(active_modal, ModalState::RequestCenter) =>
                            {
                                Some(Action::RequestBackspace)
                            }
                            KeyCode::Char(' ')
                                if matches!(active_modal, ModalState::RequestCenter) =>
                            {
                                Some(Action::RequestToggleCurrent)
                            }
                            KeyCode::Char('r') | KeyCode::Char('R')
                                if matches!(active_modal, ModalState::RequestCenter) =>
                            {
                                Some(Action::RequestReject)
                            }
                            KeyCode::Char(c)
                                if matches!(active_modal, ModalState::RequestCenter)
                                    && c.is_ascii_digit() =>
                            {
                                Some(Action::RequestDigit(c as u8 - b'0'))
                            }
                            KeyCode::Char(c)
                                if matches!(active_modal, ModalState::RequestCenter) =>
                            {
                                Some(Action::RequestInput(c))
                            }
                            KeyCode::Char('y') | KeyCode::Char('Y')
                                if matches!(active_modal, ModalState::ConfirmCloseAgent { .. }) =>
                            {
                                Some(Action::ConfirmCloseAgent(true))
                            }
                            KeyCode::Char('n') | KeyCode::Char('N')
                                if matches!(active_modal, ModalState::ConfirmCloseAgent { .. }) =>
                            {
                                Some(Action::ConfirmCloseAgent(false))
                            }
                            _ => None,
                        };
                    }
                }
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
                        KeyCode::Esc => None,
                        KeyCode::F(1) => Some(Action::ShowHelpModal),
                        KeyCode::F(2) => Some(Action::OpenDocs),
                        KeyCode::Char('g') | KeyCode::Char('G')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::ToggleUiMode)
                        }
                        KeyCode::Char('r') | KeyCode::Char('R')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenRequestCenter)
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::StartDemoStream)
                        }
                        KeyCode::Char('b') | KeyCode::Char('B')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::SpawnBackgroundDemo)
                        }
                        KeyCode::Char('[') => Some(Action::GridPagePrev),
                        KeyCode::Char(']') => Some(Action::GridPageNext),
                        KeyCode::BackTab => Some(Action::SwitchAgentPrev),
                        KeyCode::Enter
                            if key.modifiers.contains(KeyModifiers::SHIFT)
                                || key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::InsertNewline)
                        }
                        KeyCode::Enter => Some(Action::SubmitCommand),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Tab => Some(Action::SwitchAgentNext),
                        KeyCode::Up => Some(Action::ScrollUp),
                        KeyCode::Down => Some(Action::ScrollDown),
                        KeyCode::PageUp => Some(Action::PageUp),
                        KeyCode::PageDown => Some(Action::PageDown),
                        KeyCode::Char(c)
                            if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() =>
                        {
                            let idx = (c as u8 - b'0') as usize;
                            if idx > 0 {
                                Some(Action::SelectAgentByNumber(idx))
                            } else {
                                None
                            }
                        }
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                }
            }

            AppState::SetupWizard { .. } => match key.code {
                KeyCode::Esc => Some(Action::Quit),
                KeyCode::Enter => Some(Action::SetupNextStep),
                KeyCode::Down => Some(Action::SetupNextItem),
                KeyCode::Up => Some(Action::SetupPrevItem),
                KeyCode::Char(c) => Some(Action::SetupInput(c)),
                KeyCode::Backspace => Some(Action::SetupBackspace),
                _ => None,
            },
        }
    }
    pub fn handle_mouse_event(&self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::ScrollDown => match self.state {
                AppState::MainMenu => Some(Action::NextSession),
                AppState::Chat { .. } => Some(Action::ScrollDown),
                AppState::SetupWizard { .. } => Some(Action::SetupNextItem),
                _ => None,
            },
            MouseEventKind::ScrollUp => match self.state {
                AppState::MainMenu => Some(Action::PreviousSession),
                AppState::Chat { .. } => Some(Action::ScrollUp),
                AppState::SetupWizard { .. } => Some(Action::SetupPrevItem),
                _ => None,
            },
            _ => None,
        }
    }

    pub async fn update(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::CtrlCPressed => {
                let now = Instant::now();
                if self
                    .quit_armed_at
                    .map(|t| now.duration_since(t).as_millis() <= 1500)
                    .unwrap_or(false)
                {
                    self.should_quit = true;
                    self.quit_armed_at = None;
                } else {
                    self.quit_armed_at = Some(now);
                    if let AppState::Chat { messages, .. } = &mut self.state {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(
                                "Press Ctrl+C again within 1.5s to quit.".to_string(),
                            )],
                        });
                    }
                }
            }
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
            Action::ToggleTaskPin(task_id) => {
                if let AppState::Chat { tasks, .. } = &mut self.state {
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                        task.pinned = !task.pinned;
                    }
                }
            }

            Action::Tick => self.tick().await,

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
                // If configuration is missing, force wizard
                if (self.current_provider.is_none() || self.current_model.is_none())
                    && self.provider_catalog.is_some()
                {
                    let mut step = SetupStep::SelectProvider;
                    let mut selected_provider_index = 0;

                    if let Some(ref current_p) = self.current_provider {
                        if let Some(ref catalog) = self.provider_catalog {
                            if let Some(idx) = catalog.all.iter().position(|p| &p.id == current_p) {
                                selected_provider_index = idx;
                                if self.current_model.is_none() {
                                    step = SetupStep::SelectModel;
                                }
                            }
                        }
                    }

                    self.state = AppState::SetupWizard {
                        step,
                        provider_catalog: self.provider_catalog.clone(),
                        selected_provider_index,
                        selected_model_index: 0,
                        api_key_input: String::new(),
                        model_input: String::new(),
                    };
                    return Ok(());
                }

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
                                let first_agent =
                                    Self::make_agent_pane("A1".to_string(), session.id.clone());
                                self.state = AppState::Chat {
                                    session_id: session.id.clone(),
                                    command_input: String::new(),
                                    messages: Vec::new(),
                                    scroll_from_bottom: 0,
                                    tasks: Vec::new(),
                                    active_task_id: None,
                                    agents: vec![first_agent],
                                    active_agent_index: 0,
                                    ui_mode: UiMode::Focus,
                                    grid_page: 0,
                                    modal: None,
                                    pending_requests: Vec::new(),
                                    request_cursor: 0,
                                    permission_choice: 0,
                                };
                            }
                        }
                    }
                }
            }

            Action::SelectSession => {
                if !self.sessions.is_empty() {
                    let session = &self.sessions[self.selected_session_index];
                    let loaded_messages = self.load_chat_history(&session.id).await;
                    let mut first_agent =
                        Self::make_agent_pane("A1".to_string(), session.id.clone());
                    first_agent.messages = loaded_messages.clone();
                    self.state = AppState::Chat {
                        session_id: session.id.clone(),
                        command_input: String::new(),
                        messages: loaded_messages,
                        scroll_from_bottom: 0,
                        tasks: Vec::new(),
                        active_task_id: None,
                        agents: vec![first_agent],
                        active_agent_index: 0,
                        ui_mode: UiMode::Focus,
                        grid_page: 0,
                        modal: None,
                        pending_requests: Vec::new(),
                        request_cursor: 0,
                        permission_choice: 0,
                    };
                }
            }

            Action::CommandInput(c) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.push(c);
                    let input = command_input.clone();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
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
                self.sync_active_agent_from_chat();
            }
            Action::InsertNewline => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.push('\n');
                    let input = command_input.clone();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
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
                self.sync_active_agent_from_chat();
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

            Action::SwitchAgentNext => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        *active_agent_index = (*active_agent_index + 1) % agents.len();
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SwitchAgentPrev => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        if *active_agent_index == 0 {
                            *active_agent_index = agents.len().saturating_sub(1);
                        } else {
                            *active_agent_index -= 1;
                        }
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SelectAgentByNumber(n) => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if n > 0 && n <= agents.len() {
                        *active_agent_index = n - 1;
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::ToggleUiMode => {
                if let AppState::Chat { ui_mode, .. } = &mut self.state {
                    *ui_mode = if *ui_mode == UiMode::Focus {
                        UiMode::Grid
                    } else {
                        UiMode::Focus
                    };
                }
            }
            Action::GridPageNext => {
                if let AppState::Chat {
                    grid_page, agents, ..
                } = &mut self.state
                {
                    let max_page = agents.len().saturating_sub(1) / 4;
                    *grid_page = (*grid_page + 1).min(max_page);
                }
            }
            Action::GridPagePrev => {
                if let AppState::Chat { grid_page, .. } = &mut self.state {
                    *grid_page = grid_page.saturating_sub(1);
                }
            }
            Action::ShowHelpModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    *modal = Some(ModalState::Help);
                }
            }
            Action::OpenDocs => {
                // Open docs in default browser
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", "https://tandem.ai/docs"])
                    .spawn();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open")
                    .arg("https://tandem.ai/docs")
                    .spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open")
                    .arg("https://tandem.ai/docs")
                    .spawn();
            }
            Action::CloseModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    *modal = None;
                }
            }
            Action::OpenRequestCenter => {
                self.open_request_center_if_needed();
            }
            Action::RequestSelectNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = (*request_cursor + 1) % pending_requests.len();
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestSelectPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = if *request_cursor == 0 {
                            pending_requests.len().saturating_sub(1)
                        } else {
                            request_cursor.saturating_sub(1)
                        };
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestOptionNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = (q.option_cursor + 1) % q.options.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestOptionPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = if *permission_choice == 0 {
                                    2
                                } else {
                                    permission_choice.saturating_sub(1)
                                };
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = if q.option_cursor == 0 {
                                            q.options.len().saturating_sub(1)
                                        } else {
                                            q.option_cursor.saturating_sub(1)
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestToggleCurrent => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if q.option_cursor < q.options.len() {
                                        if q.multiple {
                                            if let Some(existing) = q
                                                .selected_options
                                                .iter()
                                                .position(|v| *v == q.option_cursor)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(q.option_cursor);
                                            }
                                        } else {
                                            q.selected_options = vec![q.option_cursor];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestDigit(digit) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                if (1..=3).contains(&digit) {
                                    *permission_choice = digit as usize - 1;
                                }
                            }
                            PendingRequestKind::Question(question) => {
                                let idx = digit.saturating_sub(1) as usize;
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if idx < q.options.len() {
                                        q.option_cursor = idx;
                                        if q.multiple {
                                            if let Some(existing) =
                                                q.selected_options.iter().position(|v| *v == idx)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(idx);
                                            }
                                        } else {
                                            q.selected_options = vec![idx];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestInput(c) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                if q.custom || !q.options.is_empty() {
                                    q.custom_input.push(c);
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestBackspace => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                q.custom_input.pop();
                            }
                        }
                    }
                }
            }
            Action::RequestReject => {
                let (request_id, reject_kind, question_permission_id) = if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &self.state
                {
                    if let Some(request) = pending_requests.get(*request_cursor) {
                        match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                (Some(permission.id.clone()), Some("permission"), None)
                            }
                            PendingRequestKind::Question(question) => (
                                Some(question.id.clone()),
                                Some("question"),
                                question.permission_request_id.clone(),
                            ),
                        }
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };
                if let (Some(request_id), Some(kind)) = (request_id, reject_kind) {
                    if let Some(client) = &self.client {
                        match kind {
                            "permission" => {
                                let _ = client.reply_permission(&request_id, "deny").await;
                            }
                            "question" => {
                                if let Some(permission_id) = question_permission_id {
                                    let _ = client.reply_permission(&permission_id, "deny").await;
                                }
                                let _ = client.reject_question(&request_id).await;
                            }
                            _ => {}
                        }
                    }
                    if let AppState::Chat {
                        pending_requests,
                        request_cursor,
                        modal,
                        ..
                    } = &mut self.state
                    {
                        pending_requests.retain(|request| match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                permission.id != request_id
                            }
                            PendingRequestKind::Question(question) => question.id != request_id,
                        });
                        if pending_requests.is_empty() {
                            *request_cursor = 0;
                            *modal = None;
                        } else if *request_cursor >= pending_requests.len() {
                            *request_cursor = pending_requests.len().saturating_sub(1);
                        }
                    }
                }
            }
            Action::RequestConfirm => {
                let mut remove_request_id: Option<String> = None;
                let mut permission_reply: Option<String> = None;
                let mut question_reply: Option<(String, Vec<Vec<String>>)> = None;
                let mut question_permission_once: Option<String> = None;

                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(permission) => {
                                let reply = match *permission_choice {
                                    0 => "once",
                                    1 => "always",
                                    _ => "deny",
                                };
                                remove_request_id = Some(permission.id.clone());
                                permission_reply = Some(reply.to_string());
                            }
                            PendingRequestKind::Question(question) => {
                                let can_advance = question
                                    .questions
                                    .get(question.question_index)
                                    .map(|q| {
                                        !q.selected_options.is_empty()
                                            || !q.custom_input.trim().is_empty()
                                    })
                                    .unwrap_or(false);
                                if can_advance {
                                    if question.question_index + 1 < question.questions.len() {
                                        question.question_index += 1;
                                    } else {
                                        let mut answers: Vec<Vec<String>> = Vec::new();
                                        for q in &question.questions {
                                            let mut question_answers = Vec::new();
                                            for idx in &q.selected_options {
                                                if let Some(option) = q.options.get(*idx) {
                                                    question_answers.push(option.label.clone());
                                                }
                                            }
                                            let custom = q.custom_input.trim();
                                            if !custom.is_empty() {
                                                question_answers.push(custom.to_string());
                                            }
                                            if question_answers.is_empty() {
                                                question_answers.push(String::new());
                                            }
                                            answers.push(question_answers);
                                        }
                                        remove_request_id = Some(question.id.clone());
                                        if let Some(permission_id) =
                                            question.permission_request_id.clone()
                                        {
                                            question_permission_once = Some(permission_id);
                                        }
                                        question_reply = Some((question.id.clone(), answers));
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(client) = &self.client {
                    if let (Some(request_id), Some(reply)) =
                        (remove_request_id.clone(), permission_reply.clone())
                    {
                        let _ = client.reply_permission(&request_id, &reply).await;
                    }
                    if let Some(permission_id) = question_permission_once.clone() {
                        let _ = client.reply_permission(&permission_id, "once").await;
                    }
                    if let Some((question_id, answers)) = question_reply.clone() {
                        let _ = client.reply_question(&question_id, answers).await;
                    }
                }

                if let Some(request_id) = remove_request_id {
                    if permission_reply.is_some() || question_reply.is_some() {
                        if let AppState::Chat {
                            pending_requests,
                            request_cursor,
                            modal,
                            ..
                        } = &mut self.state
                        {
                            pending_requests.retain(|request| match &request.kind {
                                PendingRequestKind::Permission(permission) => {
                                    permission.id != request_id
                                }
                                PendingRequestKind::Question(question) => question.id != request_id,
                            });
                            if pending_requests.is_empty() {
                                *request_cursor = 0;
                                *modal = None;
                            } else if *request_cursor >= pending_requests.len() {
                                *request_cursor = pending_requests.len().saturating_sub(1);
                            }
                        }
                    }
                }
            }
            Action::NewAgent => {
                self.sync_active_agent_from_chat();
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id,
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                }
                self.sync_chat_from_active_agent();
            }
            Action::CloseActiveAgent => {
                self.sync_active_agent_from_chat();
                let mut confirm = None;
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    modal,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents.get(*active_agent_index) {
                        if !agent.draft.trim().is_empty() {
                            confirm = Some(agent.agent_id.clone());
                        }
                    }
                    if let Some(agent_id) = confirm.clone() {
                        *modal = Some(ModalState::ConfirmCloseAgent {
                            target_agent_id: agent_id,
                        });
                    }
                }
                if confirm.is_none() {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::ConfirmCloseAgent(confirmed) => {
                if !confirmed {
                    if let AppState::Chat { modal, .. } = &mut self.state {
                        *modal = None;
                    }
                } else {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::CancelActiveAgent => {
                let mut cancel_idx: Option<usize> = None;
                if let AppState::Chat {
                    modal,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if modal.is_some() {
                        *modal = None;
                    } else if let Some(agent) = agents.get_mut(*active_agent_index) {
                        if matches!(agent.status, AgentStatus::Running | AgentStatus::Streaming) {
                            agent.status = AgentStatus::Cancelling;
                            cancel_idx = Some(*active_agent_index);
                        } else {
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some(idx) = cancel_idx {
                    self.cancel_agent_if_running(idx).await;
                    if let AppState::Chat { agents, .. } = &mut self.state {
                        if let Some(agent) = agents.get_mut(idx) {
                            agent.status = AgentStatus::Idle;
                            agent.active_run_id = None;
                        }
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::StartDemoStream => {
                if let Some(tx) = &self.action_tx {
                    if let Some(agent) = self.active_agent_clone() {
                        let agent_id = agent.agent_id;
                        let session_id = agent.session_id;
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let _ = tx.send(Action::PromptRunStarted {
                                session_id: session_id.clone(),
                                agent_id: agent_id.clone(),
                                run_id: Some(format!(
                                    "demo-{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis())
                                        .unwrap_or(0)
                                )),
                            });
                            let tokens = ["demo ", "stream ", "for ", "active ", "agent"];
                            for t in tokens {
                                let _ = tx.send(Action::PromptDelta {
                                    session_id: session_id.clone(),
                                    agent_id: agent_id.clone(),
                                    delta: t.to_string(),
                                });
                                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                            }
                        });
                    }
                }
            }
            Action::SpawnBackgroundDemo => {
                self.sync_active_agent_from_chat();
                let previous_active = if let AppState::Chat {
                    active_agent_index, ..
                } = &self.state
                {
                    *active_agent_index
                } else {
                    0
                };
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                let (new_agent_id, new_agent_session_id) = if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id.clone(),
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                    let session_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    (next_agent_id, session_id)
                } else {
                    ("A1".to_string(), String::new())
                };
                self.sync_chat_from_active_agent();
                if let Some(tx) = &self.action_tx {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(Action::PromptRunStarted {
                            session_id: new_agent_session_id.clone(),
                            agent_id: new_agent_id.clone(),
                            run_id: Some(format!(
                                "demo-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                            )),
                        });
                        let tokens = ["background ", "demo ", "stream"];
                        for t in tokens {
                            let _ = tx.send(Action::PromptDelta {
                                session_id: new_agent_session_id.clone(),
                                agent_id: new_agent_id.clone(),
                                delta: t.to_string(),
                            });
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        }
                    });
                }
                if let AppState::Chat {
                    active_agent_index,
                    agents,
                    ..
                } = &mut self.state
                {
                    *active_agent_index = previous_active.min(agents.len().saturating_sub(1));
                }
                self.sync_chat_from_active_agent();
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
                    if let Some(ref key) = api_key {
                        self.save_provider_key_local(&provider_id, key);
                    }
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
                            *selected_model_index = 0;
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
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    if !model_ids.is_empty()
                                        && *selected_model_index < model_ids.len() - 1
                                    {
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
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.push(c);
                        }
                        SetupStep::SelectModel => {
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
                        _ => {}
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
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.pop();
                        }
                        SetupStep::SelectModel => {
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
                        _ => {}
                    }
                }
            }

            Action::ScrollUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::ScrollDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }

            Action::ClearCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.clear();
                }
                self.sync_active_agent_from_chat();
            }

            Action::SubmitCommand => {
                let (session_id, active_agent_id, msg_to_send) = if let AppState::Chat {
                    session_id,
                    command_input,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if command_input.trim().is_empty() {
                        return Ok(());
                    }
                    let msg = command_input.trim().to_string();
                    command_input.clear();
                    let agent_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.agent_id.clone())
                        .unwrap_or_else(|| "A1".to_string());
                    (session_id.clone(), agent_id, Some(msg))
                } else {
                    (String::new(), "A1".to_string(), None)
                };

                if let Some(msg) = msg_to_send {
                    if msg.starts_with("/tool ") {
                        // Pass through engine-native tool invocation syntax.
                        // The engine loop handles permission and execution for /tool.
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::User,
                                content: vec![ContentBlock::Text(msg.clone())],
                            });
                        }
                        self.sync_active_agent_from_chat();

                        if let Some(client) = &self.client {
                            if let Some(tx) = &self.action_tx {
                                let client = client.clone();
                                let tx = tx.clone();
                                let session_id = session_id.clone();
                                let msg = msg.clone();
                                let agent = Some(self.current_mode.as_agent().to_string());
                                let model = self.current_model_spec();
                                let agent_id = active_agent_id.clone();
                                let saw_stream_error =
                                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

                                tokio::spawn(async move {
                                    let saw_stream_error_cb = saw_stream_error.clone();
                                    match client
                                        .send_prompt_with_stream_events(
                                            &session_id,
                                            &msg,
                                            agent.as_deref(),
                                            Some(&agent_id),
                                            model,
                                            |event| {
                                                if let Some(err) =
                                                    crate::net::client::extract_stream_error(
                                                        &event.payload,
                                                    )
                                                {
                                                    if !saw_stream_error_cb.swap(
                                                        true,
                                                        std::sync::atomic::Ordering::Relaxed,
                                                    ) {
                                                        let _ = tx.send(Action::PromptFailure {
                                                            session_id: session_id.clone(),
                                                            agent_id: agent_id.clone(),
                                                            error: err,
                                                        });
                                                    }
                                                }
                                                if event.event_type == "session.run.started" {
                                                    let _ = tx.send(Action::PromptRunStarted {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        run_id: event.run_id.clone(),
                                                    });
                                                }
                                                if let Some(delta) =
                                                    crate::net::client::extract_delta_text(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptDelta {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        delta,
                                                    });
                                                }
                                                if let Some(message) =
                                                    crate::net::client::extract_stream_activity(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptInfo {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        message,
                                                    });
                                                }
                                                if let Some(request_event) =
                                                    crate::net::client::extract_stream_request(
                                                        &event.payload,
                                                    )
                                                {
                                                    let action = Self::stream_request_to_action(
                                                        session_id.clone(),
                                                        agent_id.clone(),
                                                        request_event,
                                                    );
                                                    let _ = tx.send(action);
                                                }
                                            },
                                        )
                                        .await
                                    {
                                        Ok(run) => {
                                            if saw_stream_error
                                                .load(std::sync::atomic::Ordering::Relaxed)
                                            {
                                                return;
                                            }
                                            if !run.streamed {
                                                if let Some(response) =
                                                    Self::extract_assistant_message(&run.messages)
                                                {
                                                    let _ = tx.send(Action::PromptSuccess {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        messages: vec![ChatMessage {
                                                            role: MessageRole::Assistant,
                                                            content: response,
                                                        }],
                                                    });
                                                } else {
                                                    let _ = tx.send(Action::PromptFailure {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                            .to_string(),
                                                    });
                                                }
                                            } else {
                                                let _ = tx.send(Action::PromptSuccess {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    messages: vec![],
                                                });
                                            }
                                        }
                                        Err(err) => {
                                            if !saw_stream_error
                                                .load(std::sync::atomic::Ordering::Relaxed)
                                            {
                                                let _ = tx.send(Action::PromptFailure {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    error: err.to_string(),
                                                });
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    } else if msg.starts_with('/') {
                        let response = self.execute_command(&msg).await;
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(response)],
                            });
                        }
                        self.sync_active_agent_from_chat();
                    } else if let Some(provider_id) = self.pending_model_provider.clone() {
                        let model_id = msg.trim().to_string();
                        if model_id.is_empty() {
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: vec![ContentBlock::Text(
                                        "Model cannot be empty. Paste a model name.".to_string(),
                                    )],
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
                                    content: vec![ContentBlock::Text(format!(
                                        "Provider set to {} with model {}.",
                                        provider_id, model_id
                                    ))],
                                });
                            }
                            self.sync_active_agent_from_chat();
                        }
                    } else {
                        if let Some(provider_id) = self.current_provider.clone() {
                            if !self.provider_is_connected(&provider_id)
                                && self.open_key_wizard_for_provider(&provider_id)
                            {
                                if let AppState::Chat { messages, .. } = &mut self.state {
                                    messages.push(ChatMessage {
                                        role: MessageRole::System,
                                        content: vec![ContentBlock::Text(format!(
                                            "Provider '{}' has no configured key. Enter API key in setup wizard to continue.",
                                            provider_id
                                        ))],
                                    });
                                }
                                self.sync_active_agent_from_chat();
                                return Ok(());
                            }
                        }
                        // User message
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::User,
                                content: vec![ContentBlock::Text(msg.clone())],
                            });
                        }
                        self.sync_active_agent_from_chat();

                        if let Some(client) = &self.client {
                            if let Some(tx) = &self.action_tx {
                                let client = client.clone();
                                let tx = tx.clone();
                                let session_id = session_id.clone();
                                let msg = msg.clone();
                                let agent = Some(self.current_mode.as_agent().to_string());
                                let model = self.current_model_spec();
                                let agent_id = active_agent_id.clone();
                                let saw_stream_error =
                                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

                                tokio::spawn(async move {
                                    let saw_stream_error_cb = saw_stream_error.clone();
                                    match client
                                        .send_prompt_with_stream_events(
                                            &session_id,
                                            &msg,
                                            agent.as_deref(),
                                            Some(&agent_id),
                                            model,
                                            |event| {
                                                if let Some(err) =
                                                    crate::net::client::extract_stream_error(
                                                        &event.payload,
                                                    )
                                                {
                                                    if !saw_stream_error_cb.swap(
                                                        true,
                                                        std::sync::atomic::Ordering::Relaxed,
                                                    ) {
                                                        let _ = tx.send(Action::PromptFailure {
                                                            session_id: session_id.clone(),
                                                            agent_id: agent_id.clone(),
                                                            error: err,
                                                        });
                                                    }
                                                }
                                                if event.event_type == "session.run.started" {
                                                    let _ = tx.send(Action::PromptRunStarted {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        run_id: event.run_id.clone(),
                                                    });
                                                }
                                                if let Some(delta) =
                                                    crate::net::client::extract_delta_text(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptDelta {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        delta,
                                                    });
                                                }
                                                if let Some(message) =
                                                    crate::net::client::extract_stream_activity(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptInfo {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        message,
                                                    });
                                                }
                                                if let Some(request_event) =
                                                    crate::net::client::extract_stream_request(
                                                        &event.payload,
                                                    )
                                                {
                                                    let action = Self::stream_request_to_action(
                                                        session_id.clone(),
                                                        agent_id.clone(),
                                                        request_event,
                                                    );
                                                    let _ = tx.send(action);
                                                }
                                            },
                                        )
                                        .await
                                    {
                                        Ok(run) => {
                                            if saw_stream_error
                                                .load(std::sync::atomic::Ordering::Relaxed)
                                            {
                                                return;
                                            }
                                            if !run.streamed {
                                                if let Some(response) =
                                                    Self::extract_assistant_message(&run.messages)
                                                {
                                                    let _ = tx.send(Action::PromptSuccess {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        messages: vec![ChatMessage {
                                                            role: MessageRole::Assistant,
                                                            content: response,
                                                        }],
                                                    });
                                                } else {
                                                    let _ = tx.send(Action::PromptFailure {
                                                        session_id: session_id.clone(),
                                                        agent_id: agent_id.clone(),
                                                        error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                            .to_string(),
                                                    });
                                                }
                                            } else {
                                                let _ = tx.send(Action::PromptSuccess {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    messages: vec![],
                                                });
                                            }
                                        }
                                        Err(err) => {
                                            if !saw_stream_error
                                                .load(std::sync::atomic::Ordering::Relaxed)
                                            {
                                                let _ = tx.send(Action::PromptFailure {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    error: err.to_string(),
                                                });
                                            }
                                        }
                                    }
                                });
                            } else {
                                // Fallback for synchronous (should not happen if main.rs is updated)
                                // or if channel is missing.
                                // We can't await here without blocking, so we just log error to chat
                                if let AppState::Chat { messages, .. } = &mut self.state {
                                    messages.push(ChatMessage {
                                        role: MessageRole::System,
                                        content: vec![ContentBlock::Text(
                                            "Error: Async channel not initialized. Cannot send prompt."
                                                .to_string(),
                                        )],
                                    });
                                }
                            }
                        }
                    }
                }
            }

            Action::PromptRunStarted {
                session_id: event_session_id,
                agent_id,
                run_id,
            } => {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    session_id,
                    ..
                } = &mut self.state
                {
                    if let Some(agent_idx) = agents
                        .iter()
                        .position(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        let agent = &mut agents[agent_idx];
                        agent.status = AgentStatus::Running;
                        agent.active_run_id = run_id;
                        if *active_agent_index == agent_idx {
                            *session_id = agent.session_id.clone();
                        }
                    }
                }
            }
            Action::PromptSuccess {
                session_id: event_session_id,
                agent_id,
                messages: new_messages,
            } => {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    messages,
                    scroll_from_bottom,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents
                        .iter_mut()
                        .find(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        agent.messages.extend(new_messages.clone());
                        agent.status = AgentStatus::Done;
                        agent.active_run_id = None;
                        agent.scroll_from_bottom = 0;
                    }
                    if *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id
                    {
                        messages.extend(new_messages);
                        *scroll_from_bottom = 0;
                    }
                }
                self.sync_active_agent_from_chat();
            }
            Action::PromptDelta {
                session_id: event_session_id,
                agent_id,
                delta,
            } => {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    messages,
                    scroll_from_bottom,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents
                        .iter_mut()
                        .find(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        agent.status = AgentStatus::Streaming;
                        agent.scroll_from_bottom = 0;
                        if let Some(ChatMessage {
                            role: MessageRole::Assistant,
                            content,
                        }) = agent.messages.last_mut()
                        {
                            if let Some(ContentBlock::Text(existing)) = content.first_mut() {
                                existing.push_str(&delta);
                            } else {
                                content.push(ContentBlock::Text(delta.clone()));
                            }
                        } else {
                            agent.messages.push(ChatMessage {
                                role: MessageRole::Assistant,
                                content: vec![ContentBlock::Text(delta.clone())],
                            });
                        }
                    }
                    if *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id
                    {
                        *scroll_from_bottom = 0;
                        if let Some(ChatMessage {
                            role: MessageRole::Assistant,
                            content,
                        }) = messages.last_mut()
                        {
                            if let Some(ContentBlock::Text(existing)) = content.first_mut() {
                                existing.push_str(&delta);
                            } else {
                                content.push(ContentBlock::Text(delta));
                            }
                        } else {
                            messages.push(ChatMessage {
                                role: MessageRole::Assistant,
                                content: vec![ContentBlock::Text(delta)],
                            });
                        }
                    }
                }
            }
            Action::PromptInfo {
                session_id: event_session_id,
                agent_id,
                message,
            } => {
                if let AppState::Chat { agents, .. } = &mut self.state {
                    if let Some(agent) = agents
                        .iter_mut()
                        .find(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        if !matches!(agent.status, AgentStatus::Streaming) {
                            agent.status = AgentStatus::Running;
                        }
                        // Keep the latest stream activity out of transcript; request state and
                        // status line already communicate progress.
                        let _ = message;
                    }
                }
                self.sync_active_agent_from_chat();
            }
            Action::PromptRequest {
                session_id: event_session_id,
                agent_id,
                request,
            } => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    modal,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let request_id = match &request {
                        PendingRequestKind::Permission(permission) => permission.id.clone(),
                        PendingRequestKind::Question(question) => question.id.clone(),
                    };
                    let exists = pending_requests.iter().any(|entry| match &entry.kind {
                        PendingRequestKind::Permission(permission) => permission.id == request_id,
                        PendingRequestKind::Question(question) => question.id == request_id,
                    });
                    if !exists {
                        pending_requests.push(PendingRequest {
                            session_id: event_session_id.clone(),
                            agent_id: agent_id.clone(),
                            kind: request,
                        });
                    }

                    let active_matches = *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id;
                    if active_matches {
                        if let Some(idx) =
                            pending_requests.iter().position(|entry| match &entry.kind {
                                PendingRequestKind::Permission(permission) => {
                                    permission.id == request_id
                                }
                                PendingRequestKind::Question(question) => question.id == request_id,
                            })
                        {
                            *request_cursor = idx;
                        } else {
                            *request_cursor = pending_requests.len().saturating_sub(1);
                        }
                        *modal = Some(ModalState::RequestCenter);
                    }
                }
            }
            Action::PromptRequestResolved { request_id, .. } => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    modal,
                    ..
                } = &mut self.state
                {
                    pending_requests.retain(|entry| match &entry.kind {
                        PendingRequestKind::Permission(permission) => permission.id != request_id,
                        PendingRequestKind::Question(question) => question.id != request_id,
                    });
                    if pending_requests.is_empty() {
                        *request_cursor = 0;
                        if matches!(modal, Some(ModalState::RequestCenter)) {
                            *modal = None;
                        }
                    } else if *request_cursor >= pending_requests.len() {
                        *request_cursor = pending_requests.len().saturating_sub(1);
                    }
                }
            }
            Action::PromptFailure {
                session_id: event_session_id,
                agent_id,
                error,
            } => {
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    messages,
                    scroll_from_bottom,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents
                        .iter_mut()
                        .find(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        agent.status = AgentStatus::Error;
                        agent.active_run_id = None;
                        agent.scroll_from_bottom = 0;
                        agent.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(format!("Prompt failed: {}", error))],
                        });
                    }
                    if *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id
                    {
                        *scroll_from_bottom = 0;
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(format!("Prompt failed: {}", error))],
                        });
                    }
                }
                self.sync_active_agent_from_chat();
            }

            _ => {}
        }
        Ok(())
    }

    pub async fn tick(&mut self) {
        self.tick_count += 1;

        // Check engine health every ~1 second (assuming 60tps)
        if self.tick_count % 60 == 0 {
            if let Some(client) = &self.client {
                match client.check_health().await {
                    Ok(true) => self.engine_health = EngineConnectionStatus::Connected,
                    _ => self.engine_health = EngineConnectionStatus::Error,
                }
            } else {
                self.engine_health = EngineConnectionStatus::Disconnected;
            }
        }

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
                            self.connection_status =
                                "Connected. Verifying readiness...".to_string();
                            self.client = Some(client.clone());
                            let _ = self.finalize_connecting(&client).await;
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
                    if let Some(client) = self.client.clone() {
                        if let Ok(true) = client.check_health().await {
                            let _ = self.finalize_connecting(&client).await;
                        } else {
                            self.connection_status = "Waiting for engine health...".to_string();
                        }
                    }
                }
            }
            AppState::MainMenu | AppState::Chat { .. } => {
                self.renew_engine_lease_if_due().await;
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
  /agent new         Create agent in current chat
  /agent list        List chat agents
  /agent use <A#>    Switch active agent
  /agent close       Close active agent
  /title <new title> Rename current session
  /prompt <text>    Send prompt to current session
  /tool <name> <json_args> Pass-through engine tool call
  /cancel           Cancel current operation
  /last_error       Show last prompt/system error
  /messages [limit] Show session messages
  /task add <desc>   Add a new task
  /task done <id>    Mark task as done
  /task fail <id>    Mark task as failed
  /task work <id>    Mark task as working
  /task pin <id>     Toggle pin status
  /task list         List all tasks

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
  /approve <id> [always]  Approve request
  /approve all            Approve all pending in this session
  /deny <id>              Deny request
  /answer <id> <reply>    Send raw permission reply (allow/deny/once/always/reject)
  /requests               Open pending request center

CONFIG:
  /config            Show configuration

MULTI-AGENT KEYS:
  Tab / Shift+Tab    Cycle active agent
  Alt+1..Alt+9       Jump to agent slot
  Ctrl+N             New agent
  Ctrl+W             Close active agent
  Ctrl+C             Cancel active run
  Alt+G              Toggle Focus/Grid
  Alt+R              Open request center
  [ / ]              Prev/next grid page
  Alt+S / Alt+B      Demo stream controls (dev)
  Shift+Enter        Insert newline
  Esc                Close modal / return to input
  Ctrl+X             Quit"#;
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
                    self.release_engine_lease().await;
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

            "agent" => match args.first().copied() {
                Some("new") => {
                    self.sync_active_agent_from_chat();
                    let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                        format!("A{}", agents.len() + 1)
                    } else {
                        "A1".to_string()
                    };
                    let mut new_session_id: Option<String> = None;
                    if let Some(client) = &self.client {
                        if let Ok(session) = client
                            .create_session(Some(format!("{} session", next_agent_id)))
                            .await
                        {
                            new_session_id = Some(session.id);
                        }
                    }
                    if let AppState::Chat {
                        agents,
                        active_agent_index,
                        ..
                    } = &mut self.state
                    {
                        let fallback_session = agents
                            .get(*active_agent_index)
                            .map(|a| a.session_id.clone())
                            .unwrap_or_default();
                        let pane = Self::make_agent_pane(
                            next_agent_id,
                            new_session_id.unwrap_or(fallback_session),
                        );
                        agents.push(pane);
                        *active_agent_index = agents.len().saturating_sub(1);
                    }
                    self.sync_chat_from_active_agent();
                    "Created new agent.".to_string()
                }
                Some("list") => {
                    if let AppState::Chat {
                        agents,
                        active_agent_index,
                        ..
                    } = &self.state
                    {
                        let mut out = Vec::new();
                        for (i, a) in agents.iter().enumerate() {
                            let marker = if i == *active_agent_index { ">" } else { " " };
                            out.push(format!(
                                "{} {} [{}] {}",
                                marker,
                                a.agent_id,
                                a.session_id,
                                format!("{:?}", a.status)
                            ));
                        }
                        format!("Agents:\n{}", out.join("\n"))
                    } else {
                        "Not in chat.".to_string()
                    }
                }
                Some("use") => {
                    if let Some(agent_id) = args.get(1) {
                        self.sync_active_agent_from_chat();
                        if let AppState::Chat {
                            agents,
                            active_agent_index,
                            ..
                        } = &mut self.state
                        {
                            if let Some(idx) = agents.iter().position(|a| &a.agent_id == agent_id) {
                                *active_agent_index = idx;
                                self.sync_chat_from_active_agent();
                                return format!("Switched to {}.", agent_id);
                            }
                        }
                        format!("Agent not found: {}", agent_id)
                    } else {
                        "Usage: /agent use <A#>".to_string()
                    }
                }
                Some("close") => {
                    self.sync_active_agent_from_chat();
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            return "Cannot close last agent.".to_string();
                        }
                        agents.remove(active_idx);
                        if *active_agent_index >= agents.len() {
                            *active_agent_index = agents.len().saturating_sub(1);
                        }
                        let max_page = agents.len().saturating_sub(1) / 4;
                        if *grid_page > max_page {
                            *grid_page = max_page;
                        }
                    }
                    self.sync_chat_from_active_agent();
                    "Closed active agent.".to_string()
                }
                _ => "Usage: /agent new|list|use <A#>|close".to_string(),
            },

            "use" => {
                if args.is_empty() {
                    return "Usage: /use <session_id>".to_string();
                }
                let target_id = args[0];
                if let Some(idx) = self.sessions.iter().position(|s| s.id == target_id) {
                    self.selected_session_index = idx;
                    let loaded_messages = self.load_chat_history(target_id).await;
                    if let AppState::Chat {
                        session_id,
                        messages,
                        scroll_from_bottom,
                        agents,
                        active_agent_index,
                        ..
                    } = &mut self.state
                    {
                        *session_id = target_id.to_string();
                        *messages = loaded_messages.clone();
                        *scroll_from_bottom = 0;
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent.session_id = target_id.to_string();
                            agent.messages = loaded_messages;
                            agent.scroll_from_bottom = 0;
                        }
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
                let mut step = SetupStep::SelectProvider;
                let mut selected_provider_index = 0;
                let mut filter_model = String::new();

                if !args.is_empty() {
                    let provider_id = args[0];
                    if let Some(catalog) = &self.provider_catalog {
                        if let Some(idx) = catalog.all.iter().position(|p| p.id == provider_id) {
                            selected_provider_index = idx;
                            step = if catalog.connected.contains(&provider_id.to_string()) {
                                SetupStep::SelectModel
                            } else {
                                SetupStep::EnterApiKey
                            };
                        }
                    }
                } else if let Some(current) = &self.current_provider {
                    if let Some(catalog) = &self.provider_catalog {
                        if let Some(idx) = catalog.all.iter().position(|p| &p.id == current) {
                            selected_provider_index = idx;
                            step = if catalog.connected.contains(current) {
                                SetupStep::SelectModel
                            } else {
                                SetupStep::EnterApiKey
                            };
                        }
                    }
                }

                self.state = AppState::SetupWizard {
                    step,
                    provider_catalog: self.provider_catalog.clone(),
                    selected_provider_index,
                    selected_model_index: 0,
                    api_key_input: String::new(),
                    model_input: filter_model,
                };
                "Opening provider selection...".to_string()
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
                    // Open wizard for model selection
                    let mut selected_provider_index = 0;
                    if let Some(current) = &self.current_provider {
                        if let Some(catalog) = &self.provider_catalog {
                            if let Some(idx) = catalog.all.iter().position(|p| &p.id == current) {
                                selected_provider_index = idx;
                            }
                        }
                    }
                    self.state = AppState::SetupWizard {
                        step: SetupStep::SelectModel,
                        provider_catalog: self.provider_catalog.clone(),
                        selected_provider_index,
                        selected_model_index: 0,
                        api_key_input: String::new(),
                        model_input: String::new(),
                    };
                    return "Opening model selection...".to_string();
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
                    let mut provider_ids: Vec<String> = keystore
                        .list_keys()
                        .into_iter()
                        .map(|k| Self::normalize_provider_id_from_keystore_key(&k))
                        .collect();
                    provider_ids.sort();
                    provider_ids.dedup();
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
                    let provider_id = args
                        .get(1)
                        .map(|s| s.to_string())
                        .or_else(|| self.current_provider.clone());
                    let Some(provider_id) = provider_id else {
                        return "Usage: /key set <provider_id> (or set /provider first)"
                            .to_string();
                    };
                    if self.open_key_wizard_for_provider(&provider_id) {
                        format!("Opening key setup wizard for {}...", provider_id)
                    } else {
                        format!("Provider not found: {}", provider_id)
                    }
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

            "cancel" => {
                let active_idx = if let AppState::Chat {
                    active_agent_index, ..
                } = &self.state
                {
                    *active_agent_index
                } else {
                    0
                };
                self.cancel_agent_if_running(active_idx).await;
                if let AppState::Chat { agents, .. } = &mut self.state {
                    if let Some(agent) = agents.get_mut(active_idx) {
                        agent.status = AgentStatus::Idle;
                        agent.active_run_id = None;
                    }
                }
                self.sync_chat_from_active_agent();
                "Cancel requested for active agent.".to_string()
            }

            "task" => {
                if let AppState::Chat { tasks, .. } = &mut self.state {
                    match args.get(0).map(|s| *s) {
                        Some("add") => {
                            if args.len() < 2 {
                                return "Usage: /task add <description>".to_string();
                            }
                            let description = args[1..].join(" ");
                            let id = format!("task-{}", tasks.len() + 1);
                            tasks.push(Task {
                                id: id.clone(),
                                description: description.clone(),
                                status: TaskStatus::Pending,
                                pinned: false,
                            });
                            format!("Task added: {} (ID: {})", description, id)
                        }
                        Some("done") | Some("fail") | Some("work") | Some("pending") => {
                            if args.len() < 2 {
                                return "Usage: /task <status> <id>".to_string();
                            }
                            let id = args[1];
                            if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                                match args[0] {
                                    "done" => task.status = TaskStatus::Done,
                                    "fail" => task.status = TaskStatus::Failed,
                                    "work" => task.status = TaskStatus::Working,
                                    "pending" => task.status = TaskStatus::Pending,
                                    _ => {}
                                }
                                format!("Task {} marked as {}", id, args[0])
                            } else {
                                format!("Task not found: {}", id)
                            }
                        }
                        Some("pin") => {
                            if args.len() < 2 {
                                return "Usage: /task pin <id>".to_string();
                            }
                            let id = args[1];
                            if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
                                task.pinned = !task.pinned;
                                format!("Task {} pinned: {}", id, task.pinned)
                            } else {
                                format!("Task not found: {}", id)
                            }
                        }
                        Some("list") => {
                            if tasks.is_empty() {
                                "No tasks.".to_string()
                            } else {
                                let lines: Vec<String> = tasks
                                    .iter()
                                    .map(|t| {
                                        format!(
                                            "[{}] {} ({:?}) - Pinned: {}",
                                            t.id, t.description, t.status, t.pinned
                                        )
                                    })
                                    .collect();
                                format!("Tasks:\n{}", lines.join("\n"))
                            }
                        }
                        _ => "Usage: /task add|done|fail|work|pin|list ...".to_string(),
                    }
                } else {
                    "Not in a chat session.".to_string()
                }
            }

            "messages" => {
                let limit = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);
                format!("Message history not implemented yet. (limit: {})", limit)
            }

            "last_error" => {
                if let AppState::Chat { messages, .. } = &self.state {
                    let maybe_error = messages.iter().rev().find_map(|m| {
                        if m.role != MessageRole::System {
                            return None;
                        }
                        let text = m
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        if text.to_lowercase().contains("failed")
                            || text.to_lowercase().contains("error")
                        {
                            Some(text)
                        } else {
                            None
                        }
                    });
                    maybe_error.unwrap_or_else(|| "No recent error found.".to_string())
                } else {
                    "Not in a chat session.".to_string()
                }
            }

            "prompt" => {
                let text = args.join(" ");
                if text.is_empty() {
                    return "Usage: /prompt <text...>".to_string();
                }
                let (session_id, active_agent_id, should_send) = if let AppState::Chat {
                    session_id,
                    messages,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    messages.push(ChatMessage {
                        role: MessageRole::User,
                        content: vec![ContentBlock::Text(text.clone())],
                    });
                    let agent_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.agent_id.clone())
                        .unwrap_or_else(|| "A1".to_string());
                    (session_id.clone(), agent_id, true)
                } else {
                    (String::new(), "A1".to_string(), false)
                };

                if !should_send {
                    return "Not in a chat session. Use /use <session_id> first.".to_string();
                }

                let agent = Some(self.current_mode.as_agent().to_string());
                if let Some(client) = &self.client {
                    let model = self.current_model_spec();
                    if let Some(tx) = &self.action_tx {
                        let client = client.clone();
                        let tx = tx.clone();
                        let agent_id = active_agent_id.clone();
                        let saw_stream_error =
                            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        tokio::spawn(async move {
                            let saw_stream_error_cb = saw_stream_error.clone();
                            match client
                                .send_prompt_with_stream_events(
                                    &session_id,
                                    &text,
                                    agent.as_deref(),
                                    Some(&agent_id),
                                    model,
                                    |event| {
                                        if let Some(err) =
                                            crate::net::client::extract_stream_error(&event.payload)
                                        {
                                            if !saw_stream_error_cb
                                                .swap(true, std::sync::atomic::Ordering::Relaxed)
                                            {
                                                let _ = tx.send(Action::PromptFailure {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    error: err,
                                                });
                                            }
                                        }
                                        if event.event_type == "session.run.started" {
                                            let _ = tx.send(Action::PromptRunStarted {
                                                session_id: session_id.clone(),
                                                agent_id: agent_id.clone(),
                                                run_id: event.run_id.clone(),
                                            });
                                        }
                                        if let Some(delta) =
                                            crate::net::client::extract_delta_text(&event.payload)
                                        {
                                            let _ = tx.send(Action::PromptDelta {
                                                session_id: session_id.clone(),
                                                agent_id: agent_id.clone(),
                                                delta,
                                            });
                                        }
                                        if let Some(message) =
                                            crate::net::client::extract_stream_activity(
                                                &event.payload,
                                            )
                                        {
                                            let _ = tx.send(Action::PromptInfo {
                                                session_id: session_id.clone(),
                                                agent_id: agent_id.clone(),
                                                message,
                                            });
                                        }
                                        if let Some(request_event) =
                                            crate::net::client::extract_stream_request(
                                                &event.payload,
                                            )
                                        {
                                            let action = Self::stream_request_to_action(
                                                session_id.clone(),
                                                agent_id.clone(),
                                                request_event,
                                            );
                                            let _ = tx.send(action);
                                        }
                                    },
                                )
                                .await
                            {
                                Ok(run) => {
                                    if saw_stream_error.load(std::sync::atomic::Ordering::Relaxed) {
                                        return;
                                    }
                                    if !run.streamed {
                                        if let Some(response) =
                                            Self::extract_assistant_message(&run.messages)
                                        {
                                            let _ = tx.send(Action::PromptSuccess {
                                                session_id: session_id.clone(),
                                                agent_id: agent_id.clone(),
                                                messages: vec![ChatMessage {
                                                    role: MessageRole::Assistant,
                                                    content: response,
                                                }],
                                            });
                                        } else {
                                            let _ = tx.send(Action::PromptFailure {
                                                session_id: session_id.clone(),
                                                agent_id: agent_id.clone(),
                                                error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                    .to_string(),
                                            });
                                        }
                                    } else {
                                        let _ = tx.send(Action::PromptSuccess {
                                            session_id: session_id.clone(),
                                            agent_id: agent_id.clone(),
                                            messages: vec![],
                                        });
                                    }
                                }
                                Err(err) => {
                                    if !saw_stream_error.load(std::sync::atomic::Ordering::Relaxed)
                                    {
                                        let _ = tx.send(Action::PromptFailure {
                                            session_id: session_id.clone(),
                                            agent_id: agent_id.clone(),
                                            error: err.to_string(),
                                        });
                                    }
                                }
                            }
                        });
                    } else if let AppState::Chat { messages, .. } = &mut self.state {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(
                                "Error: Async channel not initialized. Cannot send prompt."
                                    .to_string(),
                            )],
                        });
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

            "requests" => {
                if let AppState::Chat {
                    pending_requests,
                    modal,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if pending_requests.is_empty() {
                        "No pending requests.".to_string()
                    } else {
                        if *request_cursor >= pending_requests.len() {
                            *request_cursor = pending_requests.len().saturating_sub(1);
                        }
                        *modal = Some(ModalState::RequestCenter);
                        format!(
                            "Opened request center ({} pending).",
                            pending_requests.len()
                        )
                    }
                } else {
                    "Requests are only available in chat mode.".to_string()
                }
            }

            "approve" | "deny" | "answer" => {
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let session_id = if let AppState::Chat { session_id, .. } = &self.state {
                    Some(session_id.clone())
                } else {
                    None
                };

                match cmd_name {
                    "approve" => {
                        if args
                            .first()
                            .map(|s| s.eq_ignore_ascii_case("all"))
                            .unwrap_or(false)
                            || args.is_empty()
                        {
                            let Ok(snapshot) = client.list_permissions().await else {
                                return "Failed to load pending permissions.".to_string();
                            };
                            let pending: Vec<String> = snapshot
                                .requests
                                .iter()
                                .filter(|r| r.status.as_deref() == Some("pending"))
                                .filter(|r| {
                                    if let Some(sid) = &session_id {
                                        r.session_id.as_deref() == Some(sid.as_str())
                                    } else {
                                        true
                                    }
                                })
                                .map(|r| r.id.clone())
                                .collect();
                            if pending.is_empty() {
                                return "No pending permissions.".to_string();
                            }
                            let mut approved = 0usize;
                            for id in pending {
                                if client.reply_permission(&id, "allow").await.unwrap_or(false) {
                                    approved += 1;
                                }
                            }
                            return format!("Approved {} pending permission request(s).", approved);
                        }

                        let id = args[0];
                        let reply = if args
                            .get(1)
                            .map(|s| s.eq_ignore_ascii_case("always"))
                            .unwrap_or(false)
                        {
                            "always"
                        } else {
                            "allow"
                        };
                        if client.reply_permission(id, reply).await.unwrap_or(false) {
                            format!("Approved permission request {}.", id)
                        } else {
                            format!("Permission request not found: {}", id)
                        }
                    }
                    "deny" => {
                        if args.is_empty() {
                            return "Usage: /deny <id>".to_string();
                        }
                        let id = args[0];
                        if client.reply_permission(id, "deny").await.unwrap_or(false) {
                            format!("Denied permission request {}.", id)
                        } else {
                            format!("Permission request not found: {}", id)
                        }
                    }
                    "answer" => {
                        if args.is_empty() {
                            return "Usage: /answer <id> <text>".to_string();
                        }
                        let id = args[0];
                        let reply = if args.len() > 1 {
                            args[1..].join(" ")
                        } else {
                            "allow".to_string()
                        };
                        if client
                            .reply_permission(id, reply.as_str())
                            .await
                            .unwrap_or(false)
                        {
                            format!("Replied to permission request {}.", id)
                        } else {
                            format!("Permission request not found: {}", id)
                        }
                    }
                    _ => "Unsupported permission command.".to_string(),
                }
            }

            _ => format!(
                "Unknown command: {}. Type /help for available commands.",
                cmd_name
            ),
        }
    }

    fn stream_request_to_action(
        session_id: String,
        agent_id: String,
        event: crate::net::client::StreamRequestEvent,
    ) -> Action {
        match event {
            crate::net::client::StreamRequestEvent::PermissionAsked(request) => {
                if request
                    .tool
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case("question"))
                    .unwrap_or(false)
                {
                    let questions =
                        Self::question_drafts_from_permission_args(request.args.as_ref());
                    if !questions.is_empty() {
                        return Action::PromptRequest {
                            session_id,
                            agent_id,
                            request: PendingRequestKind::Question(PendingQuestionRequest {
                                id: request.id.clone(),
                                questions,
                                question_index: 0,
                                permission_request_id: Some(request.id),
                            }),
                        };
                    }
                }
                Action::PromptRequest {
                    session_id,
                    agent_id,
                    request: PendingRequestKind::Permission(PendingPermissionRequest {
                        id: request.id,
                        tool: request.tool.unwrap_or_else(|| "tool".to_string()),
                        args: request.args,
                        args_source: request.args_source,
                        args_integrity: request.args_integrity,
                        query: request.query,
                        status: request.status,
                    }),
                }
            }
            crate::net::client::StreamRequestEvent::PermissionReplied { request_id, reply } => {
                Action::PromptRequestResolved {
                    session_id,
                    agent_id,
                    request_id,
                    reply,
                }
            }
            crate::net::client::StreamRequestEvent::QuestionAsked(request) => {
                let questions = request
                    .questions
                    .into_iter()
                    .map(|q| {
                        let has_options = !q.options.is_empty();
                        QuestionDraft {
                            header: q.header,
                            question: q.question,
                            options: q.options,
                            multiple: q.multiple.unwrap_or(false),
                            custom: q.custom.unwrap_or(true) || has_options,
                            selected_options: Vec::new(),
                            custom_input: String::new(),
                            option_cursor: 0,
                        }
                    })
                    .collect::<Vec<_>>();
                Action::PromptRequest {
                    session_id,
                    agent_id,
                    request: PendingRequestKind::Question(PendingQuestionRequest {
                        id: request.id,
                        questions,
                        question_index: 0,
                        permission_request_id: None,
                    }),
                }
            }
        }
    }

    fn question_drafts_from_permission_args(
        args: Option<&serde_json::Value>,
    ) -> Vec<QuestionDraft> {
        let Some(args) = args else {
            return Vec::new();
        };
        let Some(items) = args.get("questions").and_then(|v| v.as_array()) else {
            return Vec::new();
        };

        items
            .iter()
            .filter_map(|item| {
                let question = item.get("question").and_then(|v| v.as_str())?;
                let header = item
                    .get("header")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Question")
                    .to_string();
                let options = item
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|opt| {
                                let label = opt.get("label").and_then(|v| v.as_str())?;
                                let description = opt
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                Some(crate::net::client::QuestionChoice {
                                    label: label.to_string(),
                                    description,
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let has_options = !options.is_empty();
                let multiple = item
                    .get("multiple")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let custom =
                    item.get("custom").and_then(|v| v.as_bool()).unwrap_or(true) || has_options;

                Some(QuestionDraft {
                    header,
                    question: question.to_string(),
                    options,
                    multiple,
                    custom,
                    selected_options: Vec::new(),
                    custom_input: String::new(),
                    option_cursor: 0,
                })
            })
            .collect()
    }

    fn current_model_spec(&self) -> Option<ModelSpec> {
        let provider_id = self.current_provider.as_ref()?.to_string();
        let model_id = self.current_model.as_ref()?.to_string();
        Some(ModelSpec {
            provider_id,
            model_id,
        })
    }

    fn extract_assistant_message(messages: &[WireSessionMessage]) -> Option<Vec<ContentBlock>> {
        let message = messages
            .iter()
            .rev()
            .find(|msg| msg.info.role.eq_ignore_ascii_case("assistant"))?;

        let mut blocks = Vec::new();
        for part in &message.parts {
            let type_str = part.get("type").and_then(|v| v.as_str());
            match type_str {
                Some("text") => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Text(text.to_string()));
                    }
                }
                Some("tool_use") => {
                    let id = part
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = part
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = part
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or("{}".to_string());
                    blocks.push(ContentBlock::ToolCall(ToolCallInfo {
                        id,
                        name,
                        args: input,
                    }));
                }
                _ => {}
            }
        }

        if blocks.is_empty() {
            None
        } else {
            Some(blocks)
        }
    }

    async fn load_chat_history(&self, session_id: &str) -> Vec<ChatMessage> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        let Ok(wire_messages) = client.get_session_messages(session_id).await else {
            return Vec::new();
        };
        wire_messages
            .iter()
            .filter_map(Self::wire_message_to_chat_message)
            .collect()
    }

    fn wire_message_to_chat_message(msg: &WireSessionMessage) -> Option<ChatMessage> {
        let role = match msg.info.role.to_ascii_lowercase().as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            _ => MessageRole::System,
        };
        let mut content = Vec::new();
        for part in &msg.parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            match part_type {
                "text" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
                "tool_use" => {
                    let id = part
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = part
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool")
                        .to_string();
                    let args = part
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    content.push(ContentBlock::ToolCall(ToolCallInfo { id, name, args }));
                }
                "tool_result" => {
                    let text = part
                        .get("output")
                        .or_else(|| part.get("result"))
                        .or_else(|| part.get("text"))
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_else(|| "tool result".to_string());
                    content.push(ContentBlock::ToolResult(text));
                }
                _ => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
            }
        }
        if content.is_empty() {
            None
        } else {
            Some(ChatMessage { role, content })
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
        self.release_engine_lease().await;
        if Self::shared_engine_mode_enabled() {
            // Shared mode: detach and let the engine continue serving other clients.
            let _ = self.engine_process.take();
            return;
        }
        self.stop_engine_process().await;
    }

    async fn acquire_engine_lease(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        if self.engine_lease_id.is_some() {
            return;
        }
        match client.acquire_lease("tui-cli", "tui", Some(60_000)).await {
            Ok(lease) => {
                self.engine_lease_id = Some(lease.lease_id);
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Err(err) => {
                self.connection_status = format!("Lease acquire failed: {}", err);
            }
        }
    }

    async fn renew_engine_lease_if_due(&mut self) {
        let Some(lease_id) = self.engine_lease_id.clone() else {
            return;
        };
        let should_renew = self
            .engine_lease_last_renewed
            .map(|t| t.elapsed().as_secs() >= 20)
            .unwrap_or(true);
        if !should_renew {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        match client.renew_lease(&lease_id).await {
            Ok(true) => {
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Ok(false) => {
                self.engine_lease_id = None;
                self.engine_lease_last_renewed = None;
                self.acquire_engine_lease().await;
            }
            Err(_) => {}
        }
    }

    async fn release_engine_lease(&mut self) {
        let Some(lease_id) = self.engine_lease_id.take() else {
            return;
        };
        self.engine_lease_last_renewed = None;
        if let Some(client) = &self.client {
            let _ = client.release_lease(&lease_id).await;
        }
    }
}
