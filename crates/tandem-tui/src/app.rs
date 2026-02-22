use crate::ui::components::composer_input::ComposerInputState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

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
    DeleteForwardCommand,
    InsertNewline,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    MoveCursorUp,
    MoveCursorDown,
    PasteInput(String),
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
    PromptTodoUpdated {
        session_id: String,
        todos: Vec<Value>,
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
    CycleMode,
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
    PlanWizardNextField,
    PlanWizardPrevField,
    PlanWizardInput(char),
    PlanWizardBackspace,
    PlanWizardSubmit,
    ConfirmCloseAgent(bool),
    CancelActiveAgent,
    StartDemoStream,
    SpawnBackgroundDemo,
    OpenDocs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn chat_app() -> App {
        let mut app = App::new();
        let session_id = "s-test".to_string();
        let agent = App::make_agent_pane("A1".to_string(), session_id.clone());
        app.state = AppState::Chat {
            session_id,
            command_input: ComposerInputState::new(),
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::new(),
            active_task_id: None,
            agents: vec![agent],
            active_agent_index: 0,
            ui_mode: UiMode::Focus,
            grid_page: 0,
            modal: None,
            pending_requests: Vec::new(),
            request_cursor: 0,
            permission_choice: 0,
            plan_wizard: PlanFeedbackWizardState::default(),
            last_plan_task_fingerprint: Vec::new(),
            plan_awaiting_approval: false,
        };
        app
    }

    #[test]
    fn keymap_cursor_and_edit_actions_in_chat() {
        let app = chat_app();
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::MoveCursorLeft)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::MoveCursorRight)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            Some(Action::MoveCursorHome)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            Some(Action::MoveCursorEnd)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            Some(Action::DeleteForwardCommand)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::BackspaceCommand)
        );
    }

    #[test]
    fn keymap_line_nav_and_newline_shortcuts() {
        let app = chat_app();
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(Action::MoveCursorUp)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(Action::MoveCursorDown)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Some(Action::InsertNewline)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
            Some(Action::InsertNewline)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::SubmitCommand)
        );
    }

    #[test]
    fn autocomplete_mode_keeps_cursor_keymap() {
        let mut app = chat_app();
        app.show_autocomplete = true;
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::MoveCursorLeft)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::MoveCursorRight)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            Some(Action::DeleteForwardCommand)
        );
    }

    fn chat_assistant_text(app: &App) -> String {
        let AppState::Chat { messages, .. } = &app.state else {
            return String::new();
        };
        messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::Assistant))
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_success_flushes_tail() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "line1\nline2\ntail";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r1".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["li", "ne1\nl", "ine2", "\n", "ta", "il"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        let partial = chat_assistant_text(&app);
        assert_eq!(partial, "line1\nline2\n");

        app.update(Action::PromptSuccess {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            messages: vec![],
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);

        if let AppState::Chat { agents, .. } = &app.state {
            assert!(agents
                .iter()
                .find(|a| a.agent_id == agent_id)
                .and_then(|a| a.stream_collector.as_ref())
                .is_none());
        }
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_failure_flushes_tail_before_error() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "alpha\nbeta\ngamma";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r2".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["alpha\nbe", "ta\ng", "amma"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        app.update(Action::PromptFailure {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            error: "boom".to_string(),
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);

        if let AppState::Chat {
            messages, agents, ..
        } = &app.state
        {
            assert!(messages.iter().any(|m| {
                matches!(m.role, MessageRole::System)
                    && m.content.iter().any(
                        |b| matches!(b, ContentBlock::Text(t) if t.contains("Prompt failed: boom")),
                    )
            }));
            assert!(agents
                .iter()
                .find(|a| a.agent_id == agent_id)
                .and_then(|a| a.stream_collector.as_ref())
                .is_none());
        }
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_utf8_chunks() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "🙂🙂🙂\n汉字漢字\nA\u{0304}B";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r3".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["🙂", "🙂🙂\n汉", "字漢", "字\nA", "\u{0304}", "B"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        app.update(Action::PromptSuccess {
            session_id,
            agent_id,
            messages: vec![],
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);
    }
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
    PlanFeedbackWizard,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanFeedbackWizardState {
    pub plan_name: String,
    pub scope: String,
    pub constraints: String,
    pub priorities: String,
    pub notes: String,
    pub cursor_step: usize,
    pub source_request_id: Option<String>,
    pub task_preview: Vec<String>,
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
    pub draft: ComposerInputState,
    pub stream_collector: Option<crate::ui::markdown_stream::MarkdownStreamCollector>,
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
        command_input: ComposerInputState,
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
        plan_wizard: PlanFeedbackWizardState,
        last_plan_task_fingerprint: Vec<String>,
        plan_awaiting_approval: bool,
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
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use tandem_types::ModelSpec;
use tandem_wire::WireSessionMessage;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

use crate::crypto::{
    keystore::SecureKeyStore,
    vault::{EncryptedVaultKey, MAX_PIN_LENGTH},
};
use anyhow::anyhow;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::time::Instant;
use tandem_core::{
    engine_api_token_file_path, load_or_create_engine_api_token, migrate_legacy_storage_if_needed,
    resolve_shared_paths, DEFAULT_ENGINE_HOST, DEFAULT_ENGINE_PORT,
};

pub struct App {
    pub state: AppState,
    pub matrix: crate::ui::matrix::MatrixEffect,
    pub should_quit: bool,
    pub tick_count: usize,
    pub config_dir: Option<PathBuf>,
    pub vault_key: Option<EncryptedVaultKey>,
    pub keystore: Option<SecureKeyStore>,
    pub engine_process: Option<Child>,
    pub engine_binary_path: Option<PathBuf>,
    pub engine_download_retry_at: Option<Instant>,
    pub engine_download_last_error: Option<String>,
    pub engine_download_total_bytes: Option<u64>,
    pub engine_downloaded_bytes: u64,
    pub engine_download_active: bool,
    pub engine_download_phase: Option<String>,
    pub startup_engine_bootstrap_done: bool,
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
    pub engine_api_token: Option<String>,
    pub engine_api_token_backend: Option<String>,
    pub engine_base_url_override: Option<String>,
    pub engine_connection_source: EngineConnectionSource,
    pub pending_model_provider: Option<String>,
    pub autocomplete_items: Vec<(String, String)>,
    pub autocomplete_index: usize,
    pub autocomplete_mode: AutocompleteMode,
    pub show_autocomplete: bool,
    pub action_tx: Option<tokio::sync::mpsc::UnboundedSender<Action>>,
    pub quit_armed_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStalePolicy {
    AutoReplace,
    Fail,
    Warn,
}

impl EngineStalePolicy {
    fn from_env() -> Self {
        match std::env::var("TANDEM_ENGINE_STALE_POLICY")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("fail") => Self::Fail,
            Some("warn") => Self::Warn,
            _ => Self::AutoReplace,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::AutoReplace => "auto_replace",
            Self::Fail => "fail",
            Self::Warn => "warn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineConnectionSource {
    Unknown,
    SharedAttached,
    ManagedLocal,
}

impl EngineConnectionSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::SharedAttached => "shared-attached",
            Self::ManagedLocal => "managed-local",
        }
    }
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
const MIN_ENGINE_BINARY_SIZE: u64 = 100 * 1024;
const ENGINE_REPO: &str = "frumu-ai/tandem";
const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Deserialize, Clone)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
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

    pub fn next(&self) -> Self {
        match self {
            TandemMode::Ask => TandemMode::Coder,
            TandemMode::Coder => TandemMode::Explore,
            TandemMode::Explore => TandemMode::Immediate,
            TandemMode::Immediate => TandemMode::Orchestrate,
            TandemMode::Orchestrate => TandemMode::Plan,
            TandemMode::Plan => TandemMode::Ask,
        }
    }
}

impl App {
    fn sanitize_provider_catalog(
        mut catalog: crate::net::client::ProviderCatalog,
    ) -> crate::net::client::ProviderCatalog {
        catalog.all.retain(|p| p.id != "local");
        catalog.connected.retain(|id| id != "local");
        catalog
    }

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
        let mut synced = 0usize;
        for key_name in keystore.list_keys() {
            if let Ok(Some(api_key)) = keystore.get(&key_name) {
                if api_key.trim().is_empty() {
                    continue;
                }
                let provider_id = Self::normalize_provider_id_from_keystore_key(&key_name);
                if client.set_auth(&provider_id, &api_key).await.is_ok() {
                    synced += 1;
                }
            }
        }
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

    fn configured_engine_port() -> u16 {
        std::env::var("TANDEM_ENGINE_PORT")
            .ok()
            .and_then(|raw| raw.trim().parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_ENGINE_PORT)
    }

    fn configured_engine_base_url() -> String {
        if let Ok(raw) = std::env::var("TANDEM_ENGINE_URL") {
            let trimmed = raw.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        format!(
            "http://{}:{}",
            DEFAULT_ENGINE_HOST,
            Self::configured_engine_port()
        )
    }

    fn engine_target_base_url(&self) -> String {
        self.engine_base_url_override
            .clone()
            .unwrap_or_else(Self::configured_engine_base_url)
    }

    fn engine_base_url_for_port(port: u16) -> String {
        format!("http://{}:{}", DEFAULT_ENGINE_HOST, port)
    }

    fn pick_spawn_port() -> u16 {
        let configured = Self::configured_engine_port();
        if TcpListener::bind((DEFAULT_ENGINE_HOST, configured)).is_ok() {
            return configured;
        }
        TcpListener::bind((DEFAULT_ENGINE_HOST, 0))
            .ok()
            .and_then(|listener| listener.local_addr().ok().map(|addr| addr.port()))
            .filter(|port| *port != 0)
            .unwrap_or(configured)
    }

    fn masked_engine_api_token(token: &str) -> String {
        let trimmed = token.trim();
        if trimmed.is_empty() || trimmed.len() <= 8 {
            return "****".to_string();
        }
        format!("{}****{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
    }

    fn resolve_engine_api_token() -> Option<(String, String)> {
        if let Ok(raw) = std::env::var("TANDEM_API_TOKEN") {
            let token = raw.trim();
            if !token.is_empty() {
                return Some((token.to_string(), "env".to_string()));
            }
        }
        let token_material = load_or_create_engine_api_token();
        Some((token_material.token, token_material.backend))
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
        ("routines", "List scheduled routines"),
        ("routine_create", "Create interval routine"),
        ("routine_edit", "Edit routine interval"),
        ("routine_pause", "Pause a routine"),
        ("routine_resume", "Resume a routine"),
        ("routine_run_now", "Trigger a routine now"),
        ("routine_delete", "Delete a routine"),
        ("routine_history", "Show routine execution history"),
        ("missions", "List engine missions"),
        ("mission_create", "Create an engine mission"),
        ("mission_get", "Get mission details"),
        ("mission_event", "Apply mission event JSON"),
        ("mission_start", "Apply mission_started"),
        ("mission_review_ok", "Approve review gate"),
        ("mission_test_ok", "Approve test gate"),
        ("mission_review_no", "Deny review gate"),
        ("config", "Show configuration"),
    ];

    pub fn new() -> Self {
        let config_dir = Self::find_or_create_config_dir();
        let (engine_api_token, engine_api_token_backend) = Self::resolve_engine_api_token()
            .map(|(token, backend)| (Some(token), Some(backend)))
            .unwrap_or((None, None));

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
            engine_binary_path: None,
            engine_download_retry_at: None,
            engine_download_last_error: None,
            engine_download_total_bytes: None,
            engine_downloaded_bytes: 0,
            engine_download_active: false,
            engine_download_phase: None,
            startup_engine_bootstrap_done: false,
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
            engine_api_token,
            engine_api_token_backend,
            engine_base_url_override: None,
            engine_connection_source: EngineConnectionSource::Unknown,
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
            draft: ComposerInputState::new(),
            stream_collector: None,
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
                let providers = Self::sanitize_provider_catalog(providers);
                self.provider_catalog = Some(providers.clone());
                providers
            }
            Err(_) => {
                self.connection_status = "Connected. Loading providers...".to_string();
                return false;
            }
        };

        let needs_first_key_setup = self
            .keystore
            .as_ref()
            .map(|keystore| keystore.list_keys().is_empty())
            .unwrap_or(false);

        if providers.connected.is_empty() || needs_first_key_setup {
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

    fn engine_binary_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine.exe";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine";
    }

    fn engine_asset_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine-windows-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine-darwin-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine-darwin-arm64.zip";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine-linux-x64.tar.gz";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine-linux-arm64.tar.gz";
    }

    fn engine_asset_matches(asset_name: &str) -> bool {
        if !asset_name.starts_with("tandem-engine-") {
            return false;
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            return asset_name.contains("windows") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("arm64");
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("arm64");
        }
    }

    fn shared_binaries_dir() -> Option<PathBuf> {
        resolve_shared_paths()
            .ok()
            .map(|paths| paths.canonical_root.join("binaries"))
            .or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    std::env::var_os("APPDATA")
                        .map(PathBuf::from)
                        .map(|d| d.join("tandem").join("binaries"))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var_os("XDG_DATA_HOME")
                        .map(PathBuf::from)
                        .or_else(|| {
                            std::env::var_os("HOME")
                                .map(PathBuf::from)
                                .map(|h| h.join(".local").join("share"))
                        })
                        .map(|d| d.join("tandem").join("binaries"))
                }
            })
    }

    fn find_desktop_bundled_engine_binary() -> Option<PathBuf> {
        let binary_name = Self::engine_binary_name();
        let mut candidates: Vec<PathBuf> = Vec::new();

        #[cfg(target_os = "windows")]
        {
            let mut roots: Vec<PathBuf> = Vec::new();
            if let Some(v) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
                roots.push(v);
            }
            if let Some(v) = std::env::var_os("ProgramW6432").map(PathBuf::from) {
                if !roots.contains(&v) {
                    roots.push(v);
                }
            }
            if let Some(v) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                roots.push(v.join("Programs"));
            }

            for root in roots {
                let app_dir = root.join("Tandem");
                candidates.push(app_dir.join("binaries").join(binary_name));
                candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    app_dir
                        .join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        #[cfg(target_os = "macos")]
        {
            let app_dir = PathBuf::from("/Applications/Tandem.app")
                .join("Contents")
                .join("Resources");
            candidates.push(app_dir.join("binaries").join(binary_name));
            candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
        }

        #[cfg(target_os = "linux")]
        {
            let roots = [
                PathBuf::from("/opt/tandem"),
                PathBuf::from("/usr/lib/tandem"),
            ];
            for root in roots {
                candidates.push(root.join("binaries").join(binary_name));
                candidates.push(root.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    root.join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        for candidate in candidates {
            if candidate
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                return Some(candidate);
            }
        }

        None
    }

    fn find_dev_engine_binary() -> Option<PathBuf> {
        let Ok(current_dir) = env::current_dir() else {
            return None;
        };
        let binary_name = Self::engine_binary_name();
        let candidates = [
            current_dir.join("target").join("debug").join(binary_name),
            current_dir
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir
                .join("src-tauri")
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir.join("binaries").join(binary_name),
            current_dir
                .join("src-tauri")
                .join("binaries")
                .join(binary_name),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn find_extracted_binary(dir: &std::path::Path, binary_name: &str) -> anyhow::Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Ok(found) = Self::find_extracted_binary(&path, binary_name) {
                    return Ok(found);
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case(binary_name) {
                    return Ok(path);
                }
            }
        }
        Err(anyhow!("Extracted engine binary not found"))
    }

    fn parse_semver_triplet(raw: &str) -> Option<(u64, u64, u64)> {
        let token = raw
            .split_whitespace()
            .find(|part| part.chars().filter(|c| *c == '.').count() >= 2)?;
        let core = token.trim_start_matches('v');
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch_str = parts.next()?;
        let patch_digits = patch_str
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if patch_digits.is_empty() {
            return None;
        }
        let patch = patch_digits.parse::<u64>().ok()?;
        Some((major, minor, patch))
    }

    fn format_semver_triplet(version: (u64, u64, u64)) -> String {
        format!("{}.{}.{}", version.0, version.1, version.2)
    }

    fn desired_engine_version() -> Option<(u64, u64, u64)> {
        Self::parse_semver_triplet(env!("CARGO_PKG_VERSION"))
    }

    fn installed_engine_version(path: &std::path::Path) -> Option<(u64, u64, u64)> {
        let output = StdCommand::new(path).arg("--version").output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Self::parse_semver_triplet(&stdout).or_else(|| Self::parse_semver_triplet(&stderr))
    }

    fn engine_binary_is_stale(path: &std::path::Path) -> bool {
        let Some(desired) = Self::desired_engine_version() else {
            return false;
        };
        let Some(installed) = Self::installed_engine_version(path) else {
            // If we cannot determine a version, keep existing behavior and accept it.
            return false;
        };
        installed < desired
    }

    async fn ensure_engine_binary(&mut self) -> anyhow::Result<Option<PathBuf>> {
        if let Some(path) = &self.engine_binary_path {
            if path
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                if Self::engine_binary_is_stale(path) {
                    self.engine_download_phase =
                        Some("Cached engine is stale; refreshing binary".to_string());
                    self.engine_binary_path = None;
                } else {
                    return Ok(Some(path.clone()));
                }
            } else {
                self.engine_binary_path = None;
            }
        }

        if cfg!(debug_assertions) {
            if let Some(path) = Self::find_dev_engine_binary() {
                self.engine_binary_path = Some(path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase = Some("Using local dev engine binary".to_string());
                return Ok(Some(path.clone()));
            }
        }

        if let Some(path) = Self::find_desktop_bundled_engine_binary() {
            if Self::engine_binary_is_stale(&path) {
                self.engine_download_phase = Some(
                    "Desktop bundled engine is stale; using updated sidecar binary".to_string(),
                );
            } else {
                self.engine_binary_path = Some(path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase =
                    Some("Using desktop bundled engine binary".to_string());
                return Ok(Some(path));
            }
        }

        let Some(binaries_dir) = Self::shared_binaries_dir() else {
            return Err(anyhow!(
                "Unable to resolve Tandem binaries directory for engine download"
            ));
        };
        let binary_path = binaries_dir.join(Self::engine_binary_name());
        if binary_path
            .metadata()
            .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
            .unwrap_or(false)
        {
            if Self::engine_binary_is_stale(&binary_path) {
                self.engine_download_phase =
                    Some("Local sidecar engine is stale; downloading latest".to_string());
            } else {
                self.engine_binary_path = Some(binary_path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase = Some("Using cached engine binary".to_string());
                return Ok(Some(binary_path));
            }
        }

        fs::create_dir_all(&binaries_dir)?;
        self.connection_status = "Downloading engine...".to_string();
        let path = self
            .download_engine_binary(&binaries_dir, &binary_path)
            .await?;
        self.engine_binary_path = Some(path.clone());
        self.engine_download_active = false;
        self.engine_download_last_error = None;
        self.engine_download_retry_at = None;
        self.engine_download_phase = Some("Engine download complete".to_string());
        Ok(Some(path))
    }

    async fn download_engine_binary(
        &mut self,
        binaries_dir: &PathBuf,
        binary_path: &PathBuf,
    ) -> anyhow::Result<PathBuf> {
        self.engine_download_active = true;
        self.engine_download_total_bytes = None;
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some("Fetching release metadata".to_string());

        let client = Client::new();
        let release_url = format!("{}/repos/{}/releases", GITHUB_API, ENGINE_REPO);
        let releases: Vec<GitHubRelease> = client
            .get(release_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let release = releases
            .iter()
            .find(|release| {
                !release.draft
                    && !release.prerelease
                    && release
                        .assets
                        .iter()
                        .any(|asset| Self::engine_asset_matches(&asset.name))
            })
            .or_else(|| {
                releases.iter().find(|release| {
                    !release.draft
                        && release
                            .assets
                            .iter()
                            .any(|asset| Self::engine_asset_matches(&asset.name))
                })
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine release found"))?;
        if release.prerelease {
            tracing::info!(
                "No stable compatible tandem-engine release found; using prerelease {}",
                release.tag_name
            );
        }

        let asset_name = Self::engine_asset_name();
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .find(|asset| Self::engine_asset_matches(&asset.name))
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine asset found"))?;

        let download_url = asset.browser_download_url.clone();
        let archive_path = binary_path.with_extension("download");
        self.engine_download_total_bytes = Some(asset.size);
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some(format!("Downloading {}", asset.name));
        let mut response = client
            .get(&download_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?;
        if let Some(total) = response.content_length() {
            self.engine_download_total_bytes = Some(total);
        }
        let mut file = tokio::fs::File::create(&archive_path).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            self.engine_downloaded_bytes = self
                .engine_downloaded_bytes
                .saturating_add(chunk.len() as u64);
            self.connection_status = match self.engine_download_total_bytes {
                Some(total) if total > 0 => {
                    let pct = (self.engine_downloaded_bytes as f64 / total as f64) * 100.0;
                    format!("Downloading engine... {:.0}%", pct.clamp(0.0, 100.0))
                }
                _ => format!(
                    "Downloading engine... {} KB",
                    self.engine_downloaded_bytes / 1024
                ),
            };
        }
        file.flush().await?;
        self.engine_download_phase = Some("Extracting engine archive".to_string());

        let asset_name = asset.name.clone();
        let archive_path_clone = archive_path.clone();
        let binaries_dir_clone = binaries_dir.clone();
        let binary_path_clone = binary_path.clone();

        let extracted_path = tokio::task::spawn_blocking(move || -> anyhow::Result<PathBuf> {
            if asset_name.ends_with(".zip") {
                let file = fs::File::open(&archive_path_clone)?;
                let mut archive = zip::ZipArchive::new(file)?;
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    let outpath = binaries_dir_clone.join(file.mangled_name());
                    if file.is_dir() {
                        fs::create_dir_all(&outpath)?;
                    } else {
                        if let Some(p) = outpath.parent() {
                            fs::create_dir_all(p)?;
                        }
                        let mut outfile = fs::File::create(&outpath)?;
                        std::io::copy(&mut file, &mut outfile)?;
                    }
                }
            } else if asset_name.ends_with(".tar.gz") {
                let file = fs::File::open(&archive_path_clone)?;
                let gz = flate2::read::GzDecoder::new(file);
                let mut archive = tar::Archive::new(gz);
                archive.unpack(&binaries_dir_clone)?;
            }

            let extracted =
                Self::find_extracted_binary(&binaries_dir_clone, Self::engine_binary_name())?;
            if extracted != binary_path_clone {
                if binary_path_clone.exists() {
                    fs::remove_file(&binary_path_clone)?;
                }
                fs::rename(&extracted, &binary_path_clone)?;
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&binary_path_clone)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&binary_path_clone, perms)?;
            }

            fs::remove_file(&archive_path_clone).ok();
            Ok(binary_path_clone)
        })
        .await??;
        self.engine_download_phase = Some("Finalizing engine install".to_string());
        Ok(extracted_path)
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
                if !self.startup_engine_bootstrap_done {
                    return None;
                }
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
                            KeyCode::Enter
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardSubmit)
                            }
                            KeyCode::Up if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestSelectPrev)
                            }
                            KeyCode::Up
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardPrevField)
                            }
                            KeyCode::Down if matches!(active_modal, ModalState::RequestCenter) => {
                                Some(Action::RequestSelectNext)
                            }
                            KeyCode::Down
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardNextField)
                            }
                            KeyCode::Tab
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardNextField)
                            }
                            KeyCode::BackTab
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardPrevField)
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
                            KeyCode::Backspace
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardBackspace)
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
                            KeyCode::Char(c)
                                if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                            {
                                Some(Action::PlanWizardInput(c))
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
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
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
                        KeyCode::Char('m') | KeyCode::Char('M')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::CycleMode)
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
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Tab => Some(Action::SwitchAgentNext),
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorUp)
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorDown)
                        }
                        KeyCode::Up => Some(Action::ScrollUp),
                        KeyCode::Down => Some(Action::ScrollDown),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
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
                        // If a vault key exists, unlock flow should always be used.
                        // An empty/missing keystore can be recreated after successful unlock.
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
                    } else if c.is_ascii_digit() && input.len() < MAX_PIN_LENGTH {
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
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::UnlockExisting,
                            };
                            return Ok(());
                        }
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
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }
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
                                    command_input: ComposerInputState::new(),
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
                                    plan_wizard: PlanFeedbackWizardState::default(),
                                    last_plan_task_fingerprint: Vec::new(),
                                    plan_awaiting_approval: false,
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
                    let (recalled_tasks, recalled_active_task_id) =
                        Self::rebuild_tasks_from_messages(&loaded_messages);
                    let mut first_agent =
                        Self::make_agent_pane("A1".to_string(), session.id.clone());
                    first_agent.messages = loaded_messages.clone();
                    first_agent.tasks = recalled_tasks.clone();
                    first_agent.active_task_id = recalled_active_task_id.clone();
                    self.state = AppState::Chat {
                        session_id: session.id.clone(),
                        command_input: ComposerInputState::new(),
                        messages: loaded_messages,
                        scroll_from_bottom: 0,
                        tasks: recalled_tasks,
                        active_task_id: recalled_active_task_id,
                        agents: vec![first_agent],
                        active_agent_index: 0,
                        ui_mode: UiMode::Focus,
                        grid_page: 0,
                        modal: None,
                        pending_requests: Vec::new(),
                        request_cursor: 0,
                        permission_choice: 0,
                        plan_wizard: PlanFeedbackWizardState::default(),
                        last_plan_task_fingerprint: Vec::new(),
                        plan_awaiting_approval: false,
                    };
                }
            }

            Action::CommandInput(c) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char(c);
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }

            Action::BackspaceCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.backspace();
                    let input = command_input.text().to_string();
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
            Action::DeleteForwardCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.delete_forward();
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::InsertNewline => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char('\n');
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorLeft => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_left();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorRight => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_right();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorHome => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_home();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorEnd => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_end();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorUp => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_up();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorDown => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_down();
                }
                self.sync_active_agent_from_chat();
            }
            Action::PasteInput(text) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_str(&text);
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }

            Action::Autocomplete => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if !command_input.text().starts_with('/') {
                        command_input.clear();
                        command_input.insert_char('/');
                    }
                    let input = command_input.text().to_string();
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
                                command_input.set_text(format!("/{} ", cmd));
                            }
                            AutocompleteMode::Provider => {
                                command_input.set_text(format!("/provider {}", cmd));
                            }
                            AutocompleteMode::Model => {
                                command_input.set_text(format!("/model {}", cmd));
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
            Action::CycleMode => {
                self.current_mode = self.current_mode.next();
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
            Action::PlanWizardNextField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = (plan_wizard.cursor_step + 1) % 5;
                }
            }
            Action::PlanWizardPrevField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = if plan_wizard.cursor_step == 0 {
                        4
                    } else {
                        plan_wizard.cursor_step.saturating_sub(1)
                    };
                }
            }
            Action::PlanWizardInput(c) => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.push(c);
                }
            }
            Action::PlanWizardBackspace => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.pop();
                }
            }
            Action::PlanWizardSubmit => {
                let follow_up = if let AppState::Chat { plan_wizard, .. } = &self.state {
                    Self::build_plan_feedback_markdown(plan_wizard)
                } else {
                    String::new()
                };
                if !follow_up.trim().is_empty() {
                    if let AppState::Chat {
                        command_input,
                        modal,
                        ..
                    } = &mut self.state
                    {
                        *modal = None;
                        command_input.set_text(follow_up);
                    }
                    self.sync_active_agent_from_chat();
                    if let Some(tx) = &self.action_tx {
                        let _ = tx.send(Action::SubmitCommand);
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
                let mut approved_task_payload: Option<(String, Option<Value>)> = None;
                let mut approved_request_id: Option<String> = None;

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
                                approved_request_id = Some(permission.id.clone());
                                if reply != "deny" && Self::is_task_tool_name(&permission.tool) {
                                    approved_task_payload =
                                        Some((permission.tool.clone(), permission.args.clone()));
                                }
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

                if let Some((tool, args)) = approved_task_payload {
                    let fingerprint = Self::plan_fingerprint_from_args(args.as_ref());
                    let preview = Self::plan_preview_from_args(args.as_ref());
                    let should_open_wizard = if let AppState::Chat {
                        last_plan_task_fingerprint,
                        ..
                    } = &self.state
                    {
                        Self::is_todo_write_tool_name(&tool)
                            && !fingerprint.is_empty()
                            && *last_plan_task_fingerprint != fingerprint
                    } else {
                        false
                    };

                    if let AppState::Chat {
                        tasks,
                        active_task_id,
                        plan_wizard,
                        modal,
                        last_plan_task_fingerprint,
                        ..
                    } = &mut self.state
                    {
                        Self::apply_task_payload(tasks, active_task_id, &tool, args.as_ref());
                        if Self::is_todo_write_tool_name(&tool) && !fingerprint.is_empty() {
                            *last_plan_task_fingerprint = fingerprint;
                        }
                        if should_open_wizard {
                            *modal = Some(ModalState::PlanFeedbackWizard);
                            *plan_wizard = PlanFeedbackWizardState {
                                plan_name: String::new(),
                                scope: String::new(),
                                constraints: String::new(),
                                priorities: String::new(),
                                notes: String::new(),
                                cursor_step: 0,
                                source_request_id: approved_request_id.clone(),
                                task_preview: preview,
                            };
                        }
                    }
                    self.sync_active_agent_from_chat();
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
                        if !agent.draft.text().trim().is_empty() {
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
                    plan_awaiting_approval,
                    ..
                } = &mut self.state
                {
                    let raw = command_input.text().to_string();
                    if raw.trim().is_empty() {
                        return Ok(());
                    }
                    let msg = raw.trim().to_string();
                    command_input.clear();
                    let agent_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.agent_id.clone())
                        .unwrap_or_else(|| "A1".to_string());
                    *plan_awaiting_approval = false;
                    (session_id.clone(), agent_id, Some(msg))
                } else {
                    (String::new(), "A1".to_string(), None)
                };

                if let Some(msg) = msg_to_send {
                    let is_single_line = !msg.contains('\n');
                    if is_single_line && msg.starts_with("/tool ") {
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
                                let prompt_msg = self.prepare_prompt_text(&msg);
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
                                            &prompt_msg,
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
                                                if let Some((event_session_id, todos)) =
                                                    crate::net::client::extract_stream_todo_update(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptTodoUpdated {
                                                        session_id: event_session_id,
                                                        todos,
                                                    });
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
                                            } else if !run.streamed {
                                                let _ = tx.send(Action::PromptFailure {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                        .to_string(),
                                                });
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
                    } else if is_single_line && msg.starts_with('/') {
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
                                let prompt_msg = self.prepare_prompt_text(&msg);
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
                                            &prompt_msg,
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
                                                if let Some((event_session_id, todos)) =
                                                    crate::net::client::extract_stream_todo_update(
                                                        &event.payload,
                                                    )
                                                {
                                                    let _ = tx.send(Action::PromptTodoUpdated {
                                                        session_id: event_session_id,
                                                        todos,
                                                    });
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
                                            } else if !run.streamed {
                                                let _ = tx.send(Action::PromptFailure {
                                                    session_id: session_id.clone(),
                                                    agent_id: agent_id.clone(),
                                                    error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                        .to_string(),
                                                });
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
                        agent.stream_collector =
                            Some(crate::ui::markdown_stream::MarkdownStreamCollector::new());
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
                let mut finalized_tail: Option<String> = None;
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
                        if let Some(collector) = &mut agent.stream_collector {
                            let tail = collector.finalize();
                            finalized_tail = Some(tail.clone());
                            Self::append_assistant_delta(&mut agent.messages, &tail);
                        }
                        agent.stream_collector = None;
                        Self::merge_prompt_success_messages(&mut agent.messages, &new_messages);
                        agent.status = AgentStatus::Done;
                        agent.active_run_id = None;
                        agent.scroll_from_bottom = 0;
                    }
                    if *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id
                    {
                        if let Some(tail) = finalized_tail {
                            Self::append_assistant_delta(messages, &tail);
                        } else if let Some(agent) = agents.get_mut(*active_agent_index) {
                            if let Some(collector) = &mut agent.stream_collector {
                                let tail = collector.finalize();
                                Self::append_assistant_delta(messages, &tail);
                            }
                            agent.stream_collector = None;
                        }
                        Self::merge_prompt_success_messages(messages, &new_messages);
                        *scroll_from_bottom = 0;
                    }
                }
                self.sync_active_agent_from_chat();
            }
            Action::PromptTodoUpdated {
                session_id: event_session_id,
                todos,
            } => {
                let payload = serde_json::json!({ "todos": todos });
                let should_guard_pending = matches!(self.current_mode, TandemMode::Plan)
                    && Self::task_payload_all_pending(Some(&payload));
                if let AppState::Chat {
                    session_id,
                    messages,
                    tasks,
                    active_task_id,
                    agents,
                    modal,
                    plan_wizard,
                    last_plan_task_fingerprint,
                    plan_awaiting_approval,
                    ..
                } = &mut self.state
                {
                    if should_guard_pending && *plan_awaiting_approval {
                        return Ok(());
                    }

                    let fingerprint = Self::plan_fingerprint_from_args(Some(&payload));
                    let preview = Self::plan_preview_from_args(Some(&payload));
                    let should_open_wizard = matches!(self.current_mode, TandemMode::Plan)
                        && !fingerprint.is_empty()
                        && *last_plan_task_fingerprint != fingerprint;

                    if *session_id == event_session_id {
                        Self::apply_task_payload(
                            tasks,
                            active_task_id,
                            "todo_write",
                            Some(&payload),
                        );
                    }
                    for agent in agents.iter_mut() {
                        if agent.session_id == event_session_id {
                            Self::apply_task_payload(
                                &mut agent.tasks,
                                &mut agent.active_task_id,
                                "todo_write",
                                Some(&payload),
                            );
                        }
                    }
                    if !fingerprint.is_empty() {
                        *last_plan_task_fingerprint = fingerprint;
                    }
                    if should_guard_pending {
                        *plan_awaiting_approval = true;
                    }
                    if should_open_wizard {
                        *modal = Some(ModalState::PlanFeedbackWizard);
                        *plan_wizard = PlanFeedbackWizardState {
                            plan_name: String::new(),
                            scope: String::new(),
                            constraints: String::new(),
                            priorities: String::new(),
                            notes: String::new(),
                            cursor_step: 0,
                            source_request_id: None,
                            task_preview: preview,
                        };
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(
                                "Plan tasks updated. Review and refine in the Plan Feedback wizard."
                                    .to_string(),
                            )],
                        });
                    }
                }
                self.sync_chat_from_active_agent();
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
                    let mut committed = String::new();
                    if let Some(agent) = agents
                        .iter_mut()
                        .find(|a| a.agent_id == agent_id && a.session_id == event_session_id)
                    {
                        agent.status = AgentStatus::Streaming;
                        agent.scroll_from_bottom = 0;
                        let collector = agent.stream_collector.get_or_insert_with(
                            crate::ui::markdown_stream::MarkdownStreamCollector::new,
                        );
                        committed = collector.push_delta_commit_complete(&delta);
                        Self::append_assistant_delta(&mut agent.messages, &committed);
                    }
                    if *active_agent_index < agents.len()
                        && agents[*active_agent_index].agent_id == agent_id
                        && agents[*active_agent_index].session_id == event_session_id
                    {
                        *scroll_from_bottom = 0;
                        Self::append_assistant_delta(messages, &committed);
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
                let mut finalized_tail: Option<String> = None;
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
                        if let Some(collector) = &mut agent.stream_collector {
                            let tail = collector.finalize();
                            finalized_tail = Some(tail.clone());
                            Self::append_assistant_delta(&mut agent.messages, &tail);
                        }
                        agent.stream_collector = None;
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
                        if let Some(tail) = finalized_tail {
                            Self::append_assistant_delta(messages, &tail);
                        } else if let Some(agent) = agents.get_mut(*active_agent_index) {
                            if let Some(collector) = &mut agent.stream_collector {
                                let tail = collector.finalize();
                                Self::append_assistant_delta(messages, &tail);
                            }
                            agent.stream_collector = None;
                        }
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

                if !self.startup_engine_bootstrap_done {
                    if let Some(retry_at) = self.engine_download_retry_at {
                        if retry_at > Instant::now() {
                            let wait_secs = retry_at
                                .saturating_duration_since(Instant::now())
                                .as_secs()
                                .max(1);
                            self.connection_status =
                                format!("Engine download failed. Retrying in {}s...", wait_secs);
                            return;
                        }
                    }
                    self.connection_status = "Preparing engine bootstrap...".to_string();
                    match self.ensure_engine_binary().await {
                        Ok(_) => {
                            self.startup_engine_bootstrap_done = true;
                            self.connection_status =
                                "Engine ready. Press Enter to continue.".to_string();
                        }
                        Err(err) => {
                            tracing::warn!("TUI engine bootstrap failed: {}", err);
                            self.engine_download_active = false;
                            self.engine_download_last_error = Some(err.to_string());
                            self.engine_download_retry_at =
                                Some(Instant::now() + std::time::Duration::from_secs(5));
                            self.connection_status = format!("Engine download failed: {}", err);
                        }
                    }
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
                    let client = EngineClient::new_with_token(
                        self.engine_target_base_url(),
                        self.engine_api_token.clone(),
                    );
                    if let Ok(status) = client.get_engine_status().await {
                        if status.healthy {
                            let required = Self::desired_engine_version();
                            let connected = Self::parse_semver_triplet(&status.version);
                            let stale = match (required, connected) {
                                (Some(required), Some(connected)) => connected < required,
                                _ => false,
                            };
                            if stale {
                                let policy = EngineStalePolicy::from_env();
                                let required_text = required
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| "unknown".to_string());
                                let connected_text = connected
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| status.version.clone());
                                match policy {
                                    EngineStalePolicy::AutoReplace => {
                                        self.connection_status = format!(
                                            "Found stale engine {} (required {}). Starting fresh managed engine...",
                                            connected_text, required_text
                                        );
                                        self.client = None;
                                    }
                                    EngineStalePolicy::Fail => {
                                        self.connection_status = format!(
                                            "Detected stale engine {} (required {}). Set TANDEM_ENGINE_STALE_POLICY=auto_replace or run /engine restart.",
                                            connected_text, required_text
                                        );
                                        return;
                                    }
                                    EngineStalePolicy::Warn => {
                                        self.connection_status = format!(
                                            "Warning: stale engine {} (required {}), continuing due to TANDEM_ENGINE_STALE_POLICY=warn.",
                                            connected_text, required_text
                                        );
                                        self.engine_connection_source =
                                            EngineConnectionSource::SharedAttached;
                                        self.client = Some(client.clone());
                                        let _ = self.finalize_connecting(&client).await;
                                        return;
                                    }
                                }
                            } else {
                                self.connection_status =
                                    "Connected. Verifying readiness...".to_string();
                                self.engine_connection_source =
                                    EngineConnectionSource::SharedAttached;
                                self.client = Some(client.clone());
                                let _ = self.finalize_connecting(&client).await;
                                return;
                            }
                        }
                    }

                    // If not running and no process spawned, spawn it
                    if self.engine_process.is_none() {
                        self.connection_status = "Starting engine...".to_string();
                        if let Some(retry_at) = self.engine_download_retry_at {
                            if retry_at > Instant::now() {
                                let wait_secs = retry_at
                                    .saturating_duration_since(Instant::now())
                                    .as_secs()
                                    .max(1);
                                self.connection_status = format!(
                                    "Engine download failed. Retrying in {}s...",
                                    wait_secs
                                );
                                return;
                            }
                        }
                        let engine_binary = match self.ensure_engine_binary().await {
                            Ok(path) => path,
                            Err(err) => {
                                tracing::warn!("TUI could not prepare engine binary: {}", err);
                                self.engine_download_active = false;
                                self.engine_download_last_error = Some(err.to_string());
                                self.engine_download_retry_at =
                                    Some(Instant::now() + std::time::Duration::from_secs(5));
                                self.connection_status = format!("Engine download failed: {}", err);
                                return;
                            }
                        };

                        let mut spawned = false;
                        let spawn_port = Self::pick_spawn_port();
                        let configured_port = spawn_port.to_string();
                        if let Some(binary_path) = engine_binary {
                            let mut cmd = Command::new(binary_path);
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                spawned = true;
                            }
                        }

                        if !spawned {
                            let mut cmd = Command::new("tandem-engine");
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                spawned = true;
                            }
                        }

                        if !spawned && cfg!(debug_assertions) {
                            let mut cargo_cmd = Command::new("cargo");
                            cargo_cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cargo_cmd
                                .arg("run")
                                .arg("-p")
                                .arg("tandem-ai")
                                .arg("--")
                                .arg("serve")
                                .arg("--port")
                                .arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cargo_cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cargo_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cargo_cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                spawned = true;
                            }
                        }

                        if !spawned {
                            tracing::warn!(
                                "TUI failed to spawn tandem-engine from downloaded binary, PATH, and cargo fallback"
                            );
                            self.connection_status = "Failed to start engine.".to_string();
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
                                self.provider_catalog =
                                    Some(Self::sanitize_provider_catalog(catalog));
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
  /engine token      Show masked engine API token
  /engine token show Show full engine API token

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

ROUTINES:
  /routines                               List routines
  /routine_create <id> <sec> <entrypoint> Create an interval routine
  /routine_edit <id> <sec>                Update interval schedule
  /routine_pause <id>                     Pause routine
  /routine_resume <id>                    Resume routine
  /routine_run_now <id> [count]           Trigger routine immediately
  /routine_delete <id>                    Delete routine
  /routine_history <id> [limit]           Show routine history

MISSIONS:
  /missions                                List missions
  /mission_create <title> :: <goal>        Create mission (supports optional work item title after third :: segment)
  /mission_get <mission_id>                Show mission details
  /mission_event <mission_id> <event_json> Apply mission event payload JSON
  /mission_start <mission_id>              Quick mission_started event
  /mission_review_ok <mission_id> <work_item_id> [approval_id]
                                           Quick approval_granted for review
  /mission_test_ok <mission_id> <work_item_id> [approval_id]
                                           Quick approval_granted for test
  /mission_review_no <mission_id> <work_item_id> [reason]
                                           Quick approval_denied for review
  /agent-team                              Show agent-team dashboard summary
  /agent-team missions                     List agent-team mission rollups
  /agent-team instances [mission_id]       List agent-team instances
  /agent-team approvals                    List pending agent-team approvals
  /agent-team approve spawn <approval_id> [reason]
                                           Approve pending spawn approval
  /agent-team deny spawn <approval_id> [reason]
                                           Deny pending spawn approval
  /agent-team approve tool <request_id>    Approve tool permission request
  /agent-team deny tool <request_id>       Deny tool permission request

CONFIG:
  /config            Show configuration

MULTI-AGENT KEYS:
  Tab / Shift+Tab    Cycle active agent
  Alt+1..Alt+9       Jump to agent slot
  Ctrl+N             New agent
  Ctrl+W             Close active agent
  Ctrl+C             Cancel active run
  Alt+M              Cycle mode
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
                                let required = Self::desired_engine_version()
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| "unknown".to_string());
                                let stale_policy = EngineStalePolicy::from_env();
                                format!(
                                    "Engine Status:\n  Healthy: {}\n  Version: {}\n  Required: {}\n  Mode: {}\n  Endpoint: {}\n  Source: {}\n  Stale policy: {}",
                                    if status.healthy { "Yes" } else { "No" },
                                    status.version,
                                    required,
                                    status.mode,
                                    client.base_url(),
                                    self.engine_connection_source.as_str(),
                                    stale_policy.as_str()
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
                    self.engine_base_url_override = None;
                    self.engine_connection_source = EngineConnectionSource::Unknown;
                    self.provider_catalog = None;
                    sleep(std::time::Duration::from_millis(300)).await;
                    self.state = AppState::Connecting;
                    "Engine restart requested.".to_string()
                }
                Some("token") => {
                    let show_full =
                        args.get(1).map(|s| s.eq_ignore_ascii_case("show")) == Some(true);
                    let Some(token) = self.engine_api_token.as_deref().map(str::trim) else {
                        return "Engine token is not configured.".to_string();
                    };
                    if token.is_empty() {
                        return "Engine token is not configured.".to_string();
                    }
                    let value = if show_full {
                        token.to_string()
                    } else {
                        Self::masked_engine_api_token(token)
                    };
                    let path = engine_api_token_file_path().to_string_lossy().to_string();
                    let backend = self
                        .engine_api_token_backend
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    if show_full {
                        format!(
                            "Engine API token:\n  {}\nStorage: {}\nPath:\n  {}",
                            value, backend, path
                        )
                    } else {
                        format!(
                            "Engine API token (masked):\n  {}\nStorage: {}\nUse `/engine token show` to reveal.\nPath:\n  {}",
                            value, backend, path
                        )
                    }
                }
                _ => "Usage: /engine status | restart | token [show]".to_string(),
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
                    let (recalled_tasks, recalled_active_task_id) =
                        Self::rebuild_tasks_from_messages(&loaded_messages);
                    if let AppState::Chat {
                        session_id,
                        messages,
                        scroll_from_bottom,
                        tasks,
                        active_task_id,
                        agents,
                        active_agent_index,
                        ..
                    } = &mut self.state
                    {
                        *session_id = target_id.to_string();
                        *messages = loaded_messages.clone();
                        *scroll_from_bottom = 0;
                        *tasks = recalled_tasks.clone();
                        *active_task_id = recalled_active_task_id.clone();
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent.session_id = target_id.to_string();
                            agent.messages = loaded_messages;
                            agent.scroll_from_bottom = 0;
                            agent.tasks = recalled_tasks;
                            agent.active_task_id = recalled_active_task_id;
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
                let filter_model = String::new();

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
                            let catalog = Self::sanitize_provider_catalog(catalog);
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
                    let prompt_text = self.prepare_prompt_text(&text);
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
                                    &prompt_text,
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
                                        if let Some((event_session_id, todos)) =
                                            crate::net::client::extract_stream_todo_update(
                                                &event.payload,
                                            )
                                        {
                                            let _ = tx.send(Action::PromptTodoUpdated {
                                                session_id: event_session_id,
                                                todos,
                                            });
                                        }
                                    },
                                )
                                .await
                            {
                                Ok(run) => {
                                    if saw_stream_error.load(std::sync::atomic::Ordering::Relaxed) {
                                        return;
                                    }
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
                                    } else if !run.streamed {
                                        let _ = tx.send(Action::PromptFailure {
                                            session_id: session_id.clone(),
                                            agent_id: agent_id.clone(),
                                            error: "No assistant response received. Check provider key/config with /keys, /provider, /model."
                                                .to_string(),
                                        });
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

            "routines" => {
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                match client.routines_list().await {
                    Ok(routines) => {
                        if routines.is_empty() {
                            return "No routines configured.".to_string();
                        }
                        let lines = routines
                            .into_iter()
                            .map(|routine| {
                                let schedule = match routine.schedule {
                                    crate::net::client::RoutineSchedule::IntervalSeconds {
                                        seconds,
                                    } => format!("interval:{}s", seconds),
                                    crate::net::client::RoutineSchedule::Cron { expression } => {
                                        format!("cron:{expression}")
                                    }
                                };
                                format!(
                                    "- {} [{}] {} ({})",
                                    routine.routine_id, routine.name, schedule, routine.entrypoint
                                )
                            })
                            .collect::<Vec<_>>();
                        format!("Routines:\n{}", lines.join("\n"))
                    }
                    Err(err) => format!("Failed to list routines: {}", err),
                }
            }

            "routine_create" => {
                if args.len() < 3 {
                    return "Usage: /routine_create <id> <interval_seconds> <entrypoint>"
                        .to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0].to_string();
                let interval_seconds = match args[1].parse::<u64>() {
                    Ok(seconds) if seconds > 0 => seconds,
                    _ => return "interval_seconds must be a positive integer.".to_string(),
                };
                let entrypoint = args[2..].join(" ");
                let request = crate::net::client::RoutineCreateRequest {
                    routine_id: Some(routine_id.clone()),
                    name: routine_id.clone(),
                    schedule: crate::net::client::RoutineSchedule::IntervalSeconds {
                        seconds: interval_seconds,
                    },
                    timezone: None,
                    misfire_policy: Some(crate::net::client::RoutineMisfirePolicy::RunOnce),
                    entrypoint: entrypoint.clone(),
                    args: Some(json!({})),
                    allowed_tools: None,
                    output_targets: None,
                    creator_type: Some("user".to_string()),
                    creator_id: Some("tui".to_string()),
                    requires_approval: Some(true),
                    external_integrations_allowed: Some(false),
                    next_fire_at_ms: None,
                };
                match client.routines_create(request).await {
                    Ok(routine) => format!(
                        "Created routine {} ({}s -> {}).",
                        routine.routine_id, interval_seconds, routine.entrypoint
                    ),
                    Err(err) => format!("Failed to create routine: {}", err),
                }
            }

            "routine_edit" => {
                if args.len() != 2 {
                    return "Usage: /routine_edit <id> <interval_seconds>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                let interval_seconds = match args[1].parse::<u64>() {
                    Ok(seconds) if seconds > 0 => seconds,
                    _ => return "interval_seconds must be a positive integer.".to_string(),
                };
                let request = crate::net::client::RoutinePatchRequest {
                    schedule: Some(crate::net::client::RoutineSchedule::IntervalSeconds {
                        seconds: interval_seconds,
                    }),
                    ..Default::default()
                };
                match client.routines_patch(routine_id, request).await {
                    Ok(_) => format!(
                        "Updated routine {} schedule to every {}s.",
                        routine_id, interval_seconds
                    ),
                    Err(err) => format!("Failed to edit routine: {}", err),
                }
            }

            "routine_pause" => {
                if args.len() != 1 {
                    return "Usage: /routine_pause <id>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                let request = crate::net::client::RoutinePatchRequest {
                    status: Some(crate::net::client::RoutineStatus::Paused),
                    ..Default::default()
                };
                match client.routines_patch(routine_id, request).await {
                    Ok(_) => format!("Paused routine {}.", routine_id),
                    Err(err) => format!("Failed to pause routine: {}", err),
                }
            }

            "routine_resume" => {
                if args.len() != 1 {
                    return "Usage: /routine_resume <id>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                let request = crate::net::client::RoutinePatchRequest {
                    status: Some(crate::net::client::RoutineStatus::Active),
                    ..Default::default()
                };
                match client.routines_patch(routine_id, request).await {
                    Ok(_) => format!("Resumed routine {}.", routine_id),
                    Err(err) => format!("Failed to resume routine: {}", err),
                }
            }

            "routine_run_now" => {
                if args.is_empty() {
                    return "Usage: /routine_run_now <id> [run_count]".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                let run_count = if args.len() > 1 {
                    match args[1].parse::<u32>() {
                        Ok(count) if count > 0 => Some(count),
                        _ => return "run_count must be a positive integer.".to_string(),
                    }
                } else {
                    None
                };
                let request = crate::net::client::RoutineRunNowRequest {
                    run_count,
                    reason: Some("manual_tui".to_string()),
                };
                match client.routines_run_now(routine_id, request).await {
                    Ok(resp) => format!(
                        "Triggered routine {} (run_count={}).",
                        resp.routine_id, resp.run_count
                    ),
                    Err(err) => format!("Failed to trigger routine: {}", err),
                }
            }

            "routine_delete" => {
                if args.len() != 1 {
                    return "Usage: /routine_delete <id>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                match client.routines_delete(routine_id).await {
                    Ok(true) => format!("Deleted routine {}.", routine_id),
                    Ok(false) => format!("Routine not found: {}", routine_id),
                    Err(err) => format!("Failed to delete routine: {}", err),
                }
            }

            "routine_history" => {
                if args.is_empty() {
                    return "Usage: /routine_history <id> [limit]".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let routine_id = args[0];
                let limit = if args.len() > 1 {
                    match args[1].parse::<usize>() {
                        Ok(value) => Some(value),
                        Err(_) => return "limit must be a positive integer.".to_string(),
                    }
                } else {
                    Some(10)
                };
                match client.routines_history(routine_id, limit).await {
                    Ok(events) => {
                        if events.is_empty() {
                            return format!("No history for routine {}.", routine_id);
                        }
                        let lines = events
                            .iter()
                            .map(|event| {
                                format!(
                                    "- {} run_count={} status={} at={}",
                                    event.trigger_type,
                                    event.run_count,
                                    event.status,
                                    event.fired_at_ms
                                )
                            })
                            .collect::<Vec<_>>();
                        format!("Routine history ({}):\n{}", routine_id, lines.join("\n"))
                    }
                    Err(err) => format!("Failed to load routine history: {}", err),
                }
            }

            "missions" => {
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                match client.mission_list().await {
                    Ok(missions) => {
                        if missions.is_empty() {
                            return "No missions found.".to_string();
                        }
                        let lines = missions
                            .into_iter()
                            .map(|mission| {
                                format!(
                                    "- {} [{}] {} (work_items={})",
                                    mission.mission_id,
                                    format!("{:?}", mission.status).to_lowercase(),
                                    mission.spec.title,
                                    mission.work_items.len()
                                )
                            })
                            .collect::<Vec<_>>();
                        format!("Missions:\n{}", lines.join("\n"))
                    }
                    Err(err) => format!("Failed to list missions: {}", err),
                }
            }

            "mission_create" => {
                if args.is_empty() {
                    return "Usage: /mission_create <title> :: <goal> [:: work_item_title]"
                        .to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let raw = args.join(" ");
                let segments = raw
                    .split("::")
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                if segments.len() < 2 {
                    return "Usage: /mission_create <title> :: <goal> [:: work_item_title]"
                        .to_string();
                }
                let work_items = if let Some(work_item_title) = segments.get(2) {
                    vec![crate::net::client::MissionCreateWorkItem {
                        work_item_id: None,
                        title: (*work_item_title).to_string(),
                        detail: None,
                        assigned_agent: None,
                    }]
                } else {
                    vec![crate::net::client::MissionCreateWorkItem {
                        work_item_id: None,
                        title: "Initial implementation".to_string(),
                        detail: Some("Auto-seeded work item".to_string()),
                        assigned_agent: None,
                    }]
                };
                let request = crate::net::client::MissionCreateRequest {
                    title: segments[0].to_string(),
                    goal: segments[1].to_string(),
                    work_items,
                };
                match client.mission_create(request).await {
                    Ok(mission) => format!(
                        "Created mission {}: {}",
                        mission.mission_id, mission.spec.title
                    ),
                    Err(err) => format!("Failed to create mission: {}", err),
                }
            }

            "mission_get" => {
                if args.len() != 1 {
                    return "Usage: /mission_get <mission_id>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                match client.mission_get(args[0]).await {
                    Ok(mission) => {
                        let item_lines = mission
                            .work_items
                            .iter()
                            .map(|item| {
                                format!(
                                    "- {} [{}]",
                                    item.title,
                                    format!("{:?}", item.status).to_lowercase()
                                )
                            })
                            .collect::<Vec<_>>();
                        format!(
                            "Mission {} [{}]\nTitle: {}\nGoal: {}\nWork Items:\n{}",
                            mission.mission_id,
                            format!("{:?}", mission.status).to_lowercase(),
                            mission.spec.title,
                            mission.spec.goal,
                            if item_lines.is_empty() {
                                "- (none)".to_string()
                            } else {
                                item_lines.join("\n")
                            }
                        )
                    }
                    Err(err) => format!("Failed to get mission: {}", err),
                }
            }

            "mission_event" => {
                if args.len() < 2 {
                    return "Usage: /mission_event <mission_id> <event_json>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let mission_id = args[0];
                let raw_json = args[1..].join(" ");
                let event = match serde_json::from_str::<Value>(&raw_json) {
                    Ok(value) => value,
                    Err(err) => {
                        return format!("Invalid event JSON: {}", err);
                    }
                };
                match client.mission_apply_event(mission_id, event).await {
                    Ok(result) => format!(
                        "Applied event to mission {} (revision={}, commands={})",
                        result.mission.mission_id,
                        result.mission.revision,
                        result.commands.len()
                    ),
                    Err(err) => format!("Failed to apply mission event: {}", err),
                }
            }

            "mission_start" => {
                if args.len() != 1 {
                    return "Usage: /mission_start <mission_id>".to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let mission_id = args[0];
                let event = serde_json::json!({
                    "type": "mission_started",
                    "mission_id": mission_id
                });
                match client.mission_apply_event(mission_id, event).await {
                    Ok(result) => format!(
                        "Mission started {} (revision={})",
                        result.mission.mission_id, result.mission.revision
                    ),
                    Err(err) => format!("Failed to start mission: {}", err),
                }
            }

            "mission_review_ok" => {
                if args.len() < 2 {
                    return "Usage: /mission_review_ok <mission_id> <work_item_id> [approval_id]"
                        .to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let mission_id = args[0];
                let work_item_id = args[1];
                let approval_id = args.get(2).copied().unwrap_or("review-1");
                let event = serde_json::json!({
                    "type": "approval_granted",
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "approval_id": approval_id
                });
                match client.mission_apply_event(mission_id, event).await {
                    Ok(result) => format!(
                        "Review approved for {}:{} (revision={})",
                        mission_id, work_item_id, result.mission.revision
                    ),
                    Err(err) => format!("Failed to approve review: {}", err),
                }
            }

            "mission_test_ok" => {
                if args.len() < 2 {
                    return "Usage: /mission_test_ok <mission_id> <work_item_id> [approval_id]"
                        .to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let mission_id = args[0];
                let work_item_id = args[1];
                let approval_id = args.get(2).copied().unwrap_or("test-1");
                let event = serde_json::json!({
                    "type": "approval_granted",
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "approval_id": approval_id
                });
                match client.mission_apply_event(mission_id, event).await {
                    Ok(result) => format!(
                        "Test approved for {}:{} (revision={})",
                        mission_id, work_item_id, result.mission.revision
                    ),
                    Err(err) => format!("Failed to approve test: {}", err),
                }
            }

            "mission_review_no" => {
                if args.len() < 2 {
                    return "Usage: /mission_review_no <mission_id> <work_item_id> [reason]"
                        .to_string();
                }
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let mission_id = args[0];
                let work_item_id = args[1];
                let reason = if args.len() > 2 {
                    args[2..].join(" ")
                } else {
                    "needs_revision".to_string()
                };
                let event = serde_json::json!({
                    "type": "approval_denied",
                    "mission_id": mission_id,
                    "work_item_id": work_item_id,
                    "approval_id": "review-1",
                    "reason": reason
                });
                match client.mission_apply_event(mission_id, event).await {
                    Ok(result) => format!(
                        "Review denied for {}:{} (revision={})",
                        mission_id, work_item_id, result.mission.revision
                    ),
                    Err(err) => format!("Failed to deny review: {}", err),
                }
            }

            "agent-team" | "agent_team" => {
                let Some(client) = &self.client else {
                    return "Engine client not connected.".to_string();
                };
                let sub = args.first().copied().unwrap_or("summary");
                match sub {
                    "summary" => {
                        let missions = client.agent_team_missions().await;
                        let instances = client.agent_team_instances(None).await;
                        let approvals = client.agent_team_approvals().await;
                        match (missions, instances, approvals) {
                            (Ok(missions), Ok(instances), Ok(approvals)) => format!(
                                "Agent-Team Summary:\n  Missions: {}\n  Instances: {}\n  Spawn approvals: {}\n  Tool approvals: {}",
                                missions.len(),
                                instances.len(),
                                approvals.spawn_approvals.len(),
                                approvals.tool_approvals.len()
                            ),
                            _ => "Failed to load agent-team summary.".to_string(),
                        }
                    }
                    "missions" => match client.agent_team_missions().await {
                        Ok(missions) => {
                            if missions.is_empty() {
                                return "No agent-team missions found.".to_string();
                            }
                            let lines = missions
                                .into_iter()
                                .map(|mission| {
                                    format!(
                                        "- {} total={} running={} done={} failed={} cancelled={}",
                                        mission.mission_id,
                                        mission.instance_count,
                                        mission.running_count,
                                        mission.completed_count,
                                        mission.failed_count,
                                        mission.cancelled_count
                                    )
                                })
                                .collect::<Vec<_>>();
                            format!("Agent-Team Missions:\n{}", lines.join("\n"))
                        }
                        Err(err) => format!("Failed to list agent-team missions: {}", err),
                    },
                    "instances" => {
                        let mission_id = args.get(1).copied();
                        match client.agent_team_instances(mission_id).await {
                            Ok(instances) => {
                                if instances.is_empty() {
                                    return "No agent-team instances found.".to_string();
                                }
                                let lines = instances
                                    .into_iter()
                                    .map(|instance| {
                                        format!(
                                            "- {} role={} mission={} status={} parent={}",
                                            instance.instance_id,
                                            instance.role,
                                            instance.mission_id,
                                            instance.status,
                                            instance.parent_instance_id.unwrap_or_else(|| "-".to_string())
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                format!("Agent-Team Instances:\n{}", lines.join("\n"))
                            }
                            Err(err) => format!("Failed to list agent-team instances: {}", err),
                        }
                    }
                    "approvals" => match client.agent_team_approvals().await {
                        Ok(approvals) => {
                            let mut lines = Vec::new();
                            for spawn in approvals.spawn_approvals {
                                lines.push(format!("- spawn approval {}", spawn.approval_id));
                            }
                            for tool in approvals.tool_approvals {
                                lines.push(format!(
                                    "- tool approval {} ({})",
                                    tool.approval_id,
                                    tool.tool.unwrap_or_else(|| "tool".to_string())
                                ));
                            }
                            if lines.is_empty() {
                                "No agent-team approvals pending.".to_string()
                            } else {
                            format!("Agent-Team Approvals:\n{}", lines.join("\n"))
                            }
                        }
                        Err(err) => format!("Failed to list agent-team approvals: {}", err),
                    },
                    "approve" => {
                        if args.len() < 3 {
                            return "Usage: /agent-team approve <spawn|tool> <id> [reason]"
                                .to_string();
                        }
                        let target = args[1];
                        let id = args[2];
                        let reason = if args.len() > 3 {
                            args[3..].join(" ")
                        } else {
                            "approved in TUI".to_string()
                        };
                        match target {
                            "spawn" => match client.agent_team_approve_spawn(id, &reason).await {
                                Ok(true) => format!("Approved spawn approval {}.", id),
                                Ok(false) => format!("Spawn approval not found or denied: {}", id),
                                Err(err) => format!("Failed to approve spawn approval: {}", err),
                            },
                            "tool" => match client.reply_permission(id, "allow").await {
                                Ok(true) => format!("Approved tool request {}.", id),
                                Ok(false) => format!("Tool request not found: {}", id),
                                Err(err) => format!("Failed to approve tool request: {}", err),
                            },
                            _ => "Usage: /agent-team approve <spawn|tool> <id> [reason]"
                                .to_string(),
                        }
                    }
                    "deny" => {
                        if args.len() < 3 {
                            return "Usage: /agent-team deny <spawn|tool> <id> [reason]"
                                .to_string();
                        }
                        let target = args[1];
                        let id = args[2];
                        let reason = if args.len() > 3 {
                            args[3..].join(" ")
                        } else {
                            "denied in TUI".to_string()
                        };
                        match target {
                            "spawn" => match client.agent_team_deny_spawn(id, &reason).await {
                                Ok(true) => format!("Denied spawn approval {}.", id),
                                Ok(false) => format!("Spawn approval not found or already resolved: {}", id),
                                Err(err) => format!("Failed to deny spawn approval: {}", err),
                            },
                            "tool" => match client.reply_permission(id, "deny").await {
                                Ok(true) => format!("Denied tool request {}.", id),
                                Ok(false) => format!("Tool request not found: {}", id),
                                Err(err) => format!("Failed to deny tool request: {}", err),
                            },
                            _ => "Usage: /agent-team deny <spawn|tool> <id> [reason]"
                                .to_string(),
                        }
                    }
                    _ => {
                        "Usage: /agent-team [summary|missions|instances [mission_id]|approvals|approve <spawn|tool> <id> [reason]|deny <spawn|tool> <id> [reason]]".to_string()
                    }
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

    fn is_task_tool_name(tool: &str) -> bool {
        matches!(
            Self::canonical_tool_name(tool).as_str(),
            "task" | "todo_write" | "todowrite" | "update_todo_list" | "new_task"
        )
    }

    fn is_todo_write_tool_name(tool: &str) -> bool {
        matches!(
            Self::canonical_tool_name(tool).as_str(),
            "todo_write" | "todowrite" | "update_todo_list"
        )
    }

    fn canonical_tool_name(tool: &str) -> String {
        let last = tool
            .rsplit('.')
            .next()
            .unwrap_or(tool)
            .trim()
            .to_lowercase();
        last.replace('-', "_")
    }

    fn task_status_from_text(status: &str) -> TaskStatus {
        match status.to_ascii_lowercase().as_str() {
            "done" | "completed" | "complete" => TaskStatus::Done,
            "working" | "in_progress" | "in-progress" | "active" => TaskStatus::Working,
            "failed" | "error" | "blocked" => TaskStatus::Failed,
            _ => TaskStatus::Pending,
        }
    }

    fn extract_task_payload_items(args: Option<&serde_json::Value>) -> Vec<(String, TaskStatus)> {
        let Some(args) = args else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let arrays = [
            args.get("todos").and_then(|v| v.as_array()),
            args.get("tasks").and_then(|v| v.as_array()),
            args.get("steps").and_then(|v| v.as_array()),
            args.get("items").and_then(|v| v.as_array()),
        ];
        for arr in arrays.into_iter().flatten() {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    let content = obj
                        .get("content")
                        .or_else(|| obj.get("description"))
                        .or_else(|| obj.get("title"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    if content.is_empty() {
                        continue;
                    }
                    let status_text = obj
                        .get("status")
                        .or_else(|| obj.get("state"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending");
                    out.push((
                        content.to_string(),
                        Self::task_status_from_text(status_text),
                    ));
                }
            }
        }
        out
    }

    fn task_payload_all_pending(args: Option<&serde_json::Value>) -> bool {
        let items = Self::extract_task_payload_items(args);
        !items.is_empty()
            && items
                .iter()
                .all(|(_, status)| matches!(status, TaskStatus::Pending))
    }

    fn apply_task_payload(
        tasks: &mut Vec<Task>,
        active_task_id: &mut Option<String>,
        tool: &str,
        args: Option<&serde_json::Value>,
    ) {
        let incoming = Self::extract_task_payload_items(args);
        if incoming.is_empty() {
            return;
        }

        if Self::is_todo_write_tool_name(tool) {
            let mut normalized: Vec<(String, TaskStatus)> = Vec::new();
            for (description, status) in incoming {
                if let Some(existing) = normalized
                    .iter_mut()
                    .find(|(d, _)| d.eq_ignore_ascii_case(description.as_str()))
                {
                    existing.1 = status;
                } else {
                    normalized.push((description, status));
                }
            }

            let pinned_by_description = tasks
                .iter()
                .map(|t| (t.description.to_ascii_lowercase(), t.pinned))
                .collect::<std::collections::HashMap<_, _>>();

            tasks.clear();
            for (idx, (description, status)) in normalized.into_iter().enumerate() {
                let pinned = pinned_by_description
                    .get(&description.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(false);
                tasks.push(Task {
                    id: format!("task-{}", idx + 1),
                    description,
                    status,
                    pinned,
                });
            }
        } else {
            for (description, status) in incoming {
                if let Some(existing) = tasks.iter_mut().find(|t| t.description == description) {
                    existing.status = status.clone();
                } else {
                    let id = format!("task-{}", tasks.len() + 1);
                    tasks.push(Task {
                        id,
                        description,
                        status: status.clone(),
                        pinned: false,
                    });
                }
            }
        }

        if let Some(working) = tasks
            .iter()
            .find(|t| matches!(t.status, TaskStatus::Working))
        {
            *active_task_id = Some(working.id.clone());
        } else {
            *active_task_id = None;
        }
    }

    fn plan_fingerprint_from_args(args: Option<&serde_json::Value>) -> Vec<String> {
        let Some(args) = args else {
            return Vec::new();
        };
        let arrays = [
            args.get("todos").and_then(|v| v.as_array()),
            args.get("tasks").and_then(|v| v.as_array()),
            args.get("steps").and_then(|v| v.as_array()),
            args.get("items").and_then(|v| v.as_array()),
        ];

        let mut items: Vec<String> = Vec::new();
        for arr in arrays.into_iter().flatten() {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(content) = obj
                        .get("content")
                        .or_else(|| obj.get("description"))
                        .or_else(|| obj.get("title"))
                        .and_then(|v| v.as_str())
                    {
                        let normalized = content.trim().to_lowercase();
                        if !normalized.is_empty() {
                            items.push(normalized);
                        }
                    }
                }
            }
        }
        items.sort();
        items.dedup();
        items
    }

    fn plan_preview_from_args(args: Option<&serde_json::Value>) -> Vec<String> {
        Self::extract_task_payload_items(args)
            .into_iter()
            .map(|(content, _)| content)
            .take(10)
            .collect()
    }

    fn build_plan_feedback_markdown(wizard: &PlanFeedbackWizardState) -> String {
        let plan_name = if wizard.plan_name.trim().is_empty() {
            "Current plan".to_string()
        } else {
            wizard.plan_name.trim().to_string()
        };
        let scope = if wizard.scope.trim().is_empty() {
            "Use the proposed tasks as the working scope.".to_string()
        } else {
            wizard.scope.trim().to_string()
        };
        let constraints = if wizard.constraints.trim().is_empty() {
            "No additional constraints.".to_string()
        } else {
            wizard.constraints.trim().to_string()
        };
        let priorities = if wizard.priorities.trim().is_empty() {
            "Follow logical dependency order.".to_string()
        } else {
            wizard.priorities.trim().to_string()
        };
        let notes = if wizard.notes.trim().is_empty() {
            "No additional notes.".to_string()
        } else {
            wizard.notes.trim().to_string()
        };

        let mut task_lines = String::new();
        if wizard.task_preview.is_empty() {
            task_lines.push_str("- Use the current todo list from `todowrite`.\n");
        } else {
            for (idx, task) in wizard.task_preview.iter().enumerate() {
                task_lines.push_str(&format!("{}. {}\n", idx + 1, task));
            }
        }

        format!(
            "## Plan Feedback\n\
             \n\
             **Plan:** {}\n\
             \n\
             ### Approved Task Draft\n\
             {}\n\
             ### Scope\n\
             {}\n\
             \n\
             ### Constraints\n\
             {}\n\
             \n\
             ### Priority Order\n\
             {}\n\
             \n\
             ### Additional Notes\n\
             {}\n\
             \n\
             ### Next Action\n\
             Revise the plan using this feedback, update `todowrite` with refined tasks, and then ask for approval before execution.",
            plan_name, task_lines, scope, constraints, priorities, notes
        )
    }

    fn rebuild_tasks_from_messages(messages: &[ChatMessage]) -> (Vec<Task>, Option<String>) {
        let mut tasks = Vec::new();
        let mut active_task_id = None;

        for message in messages {
            for block in &message.content {
                let ContentBlock::ToolCall(tool_call) = block else {
                    continue;
                };
                if !Self::is_task_tool_name(&tool_call.name) {
                    continue;
                }
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&tool_call.args) {
                    Self::apply_task_payload(
                        &mut tasks,
                        &mut active_task_id,
                        &tool_call.name,
                        Some(&args),
                    );
                }
            }
        }

        (tasks, active_task_id)
    }

    fn prepare_prompt_text(&self, text: &str) -> String {
        let trimmed = text.trim_start();
        if trimmed.starts_with("/tool ") {
            return text.to_string();
        }
        if !matches!(self.current_mode, TandemMode::Plan) {
            return text.to_string();
        }
        let task_context = self.plan_task_context_block();
        let task_context_block = task_context
            .as_deref()
            .map(|ctx| format!("\nCurrent task list context:\n{}\n", ctx))
            .unwrap_or_default();
        format!(
            "You are operating in Plan mode.\n\
             Please use the todowrite tool to create a structured task list. Then, ask for user approval before starting execution/completing the tasks.\n\
             Tool rule: Use `todowrite` (or `todo_write` / `update_todo_list`) for plan tasks.\n\
             Do NOT use the generic `task` tool for plan creation.\n\
             First-action rule: On a new planning request, your FIRST action must be creating/updating a structured todo list.\n\
             Breakdown rule: Do not create a single generic task. Create a concrete multi-step plan with at least 6 actionable tasks (prefer 8-12 when appropriate).\n\
             Do not return only a plain numbered/text plan before creating/updating todos.\n\
             Clarification rule: If information is missing, still create an initial draft todo breakdown first, then ask clarification questions.\n\
             Approval rule: After task creation/update, ask for user approval before execution/completing tasks.\n\
             Execution rule: During execution, after verifying each task is done, use `todowrite` with status=\"completed\" for that task.\n\
             If information is missing, ask clarifying questions via the question tool.\n\
             Ask ONE clarification question at a time, then wait for the user's answer.\n\
             Prefer structured question tool prompts over plain-text question lists.\n\
             If there is already one active task list, treat it as the default plan context; do not ask \"which plan\" unless there are multiple distinct plans.\n\
             When the user says execute/continue/go, update statuses and next steps for the current task list.\n\
             After tool calls, provide a concise summary.\n{}\n\
             User request:\n{}",
            task_context_block,
            text
        )
    }

    fn plan_task_context_block(&self) -> Option<String> {
        let (tasks, active_task_id) = match &self.state {
            AppState::Chat {
                tasks,
                active_task_id,
                ..
            } => (tasks, active_task_id),
            _ => return None,
        };
        if tasks.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        lines.push(format!("Total tasks: {}", tasks.len()));
        if let Some(active_id) = active_task_id {
            lines.push(format!("Active task id: {}", active_id));
        }
        for task in tasks.iter().take(12) {
            let active_marker = if active_task_id.as_deref() == Some(task.id.as_str()) {
                ">"
            } else {
                "-"
            };
            lines.push(format!(
                "{} [{}] {}",
                active_marker,
                Self::task_status_label(&task.status),
                task.description
            ));
        }
        if tasks.len() > 12 {
            lines.push(format!("... and {} more", tasks.len() - 12));
        }
        Some(lines.join("\n"))
    }

    fn task_status_label(status: &TaskStatus) -> &'static str {
        match status {
            TaskStatus::Pending => "pending",
            TaskStatus::Working => "working",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
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

    fn extract_assistant_message(messages: &[WireSessionMessage]) -> Option<Vec<ContentBlock>> {
        let message = messages
            .iter()
            .rev()
            .find(|msg| msg.info.role.eq_ignore_ascii_case("assistant"))?;

        let mut blocks = Vec::new();
        for part in &message.parts {
            let type_str = part.get("type").and_then(|v| v.as_str());
            match type_str {
                Some("text") | Some("output_text") | Some("message_text") => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Text(text.to_string()));
                    } else if let Some(value) = part.get("value").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Text(value.to_string()));
                    } else if let Some(content) = part.get("content").and_then(|v| v.as_str()) {
                        blocks.push(ContentBlock::Text(content.to_string()));
                    }
                }
                Some("tool_use") | Some("tool_call") | Some("tool") => {
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
                Some(_) | None => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
            }
        }

        if blocks.is_empty() {
            None
        } else {
            Some(blocks)
        }
    }

    fn merge_prompt_success_messages(target: &mut Vec<ChatMessage>, new_messages: &[ChatMessage]) {
        if new_messages.is_empty() {
            return;
        }
        if new_messages.len() == 1 {
            if let Some(new_text) = Self::assistant_text_of(&new_messages[0]) {
                if let Some(last) = target.last_mut() {
                    if let Some(last_text) = Self::assistant_text_of(last) {
                        let last_trimmed = last_text.trim();
                        let new_trimmed = new_text.trim();
                        if new_trimmed.is_empty() {
                            return;
                        }
                        if last_trimmed.is_empty()
                            || new_trimmed == last_trimmed
                            || new_trimmed.starts_with(last_trimmed)
                        {
                            *last = new_messages[0].clone();
                            return;
                        }
                        // Keep richer local assistant content if server success snapshot is regressive.
                        if last_trimmed.starts_with(new_trimmed) {
                            return;
                        }
                    }
                }
            }
        }
        target.extend_from_slice(new_messages);
    }

    fn append_assistant_delta(target: &mut Vec<ChatMessage>, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if let Some(ChatMessage {
            role: MessageRole::Assistant,
            content,
        }) = target.last_mut()
        {
            if let Some(ContentBlock::Text(existing)) = content.first_mut() {
                existing.push_str(delta);
                return;
            }
            content.push(ContentBlock::Text(delta.to_string()));
            return;
        }
        target.push(ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(delta.to_string())],
        });
    }

    fn assistant_text_of(message: &ChatMessage) -> Option<String> {
        if !matches!(message.role, MessageRole::Assistant) {
            return None;
        }
        let text = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            None
        } else {
            Some(text)
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
        if model_id.is_some() {
            let mut provider_patch = serde_json::Map::new();
            if let Some(model_id) = model_id {
                provider_patch.insert("default_model".to_string(), json!(model_id));
            }
            let mut providers = serde_json::Map::new();
            providers.insert(provider_id.to_string(), Value::Object(provider_patch));
            patch.insert("providers".to_string(), Value::Object(providers));
        }
        if let Some(api_key) = api_key {
            let _ = client.set_auth(provider_id, api_key).await;
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
