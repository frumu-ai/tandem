//! Session dispatcher — routes incoming channel messages to Tandem sessions.
//!
//! Each unique `{channel_name}:{sender_id}` pair maps to one persistent Tandem
//! session. The mapping is durably persisted under Tandem's app-data state dir
//! (for example `~/.local/share/tandem/data/channel_sessions.json` on Linux)
//! and reloaded on startup.
//!
//! ## API paths (tandem-server)
//!
//! | Action         | Path                                 |
//! |----------------|--------------------------------------|
//! | Create session | `POST /session`                      |
//! | List sessions  | `GET  /session`                      |
//! | Get session    | `GET  /session/{id}`                 |
//! | Update session | `PUT  /session/{id}`                 |
//! | Prompt (sync)  | `POST /session/{id}/prompt_sync`     |
//!
//! ## Slash commands
//!
//! `/new [name]`, `/sessions`, `/resume <query>`, `/rename <name>`,
//! `/status`, `/run`, `/cancel`, `/todos`, `/requests`, `/answer <id> <text>`,
//! `/providers`, `/models [provider]`, `/model <model_id>`, `/approve <tool_call_id>`,
//! `/deny <tool_call_id>`, `/schedule ...`, `/help [topic]`

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::config::{ChannelSecurityProfile, ChannelsConfig};
use crate::discord::DiscordChannel;
use crate::slack::SlackChannel;
use crate::telegram::TelegramChannel;
use crate::traits::{Channel, ChannelMessage, SendMessage};

#[derive(Debug)]
enum PackBuilderReplyCommand {
    Confirm,
    Cancel,
    UseConnectors(Vec<String>),
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

/// Attach both auth schemes so the dispatcher works regardless of whether the
/// Tandem server is running in headless mode (Bearer) or via the Tauri sidecar
/// (x-tandem-token).
fn add_auth(rb: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    rb.header("x-tandem-token", token).bearer_auth(token)
}

// ---------------------------------------------------------------------------
// Session map + persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelToolPreferences {
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub enabled_mcp_servers: Vec<String>,
}

impl Default for ChannelToolPreferences {
    fn default() -> Self {
        Self {
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            enabled_mcp_servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub created_at_ms: u64,
    pub last_seen_at_ms: u64,
    pub channel: String,
    pub sender: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_preferences: Option<ChannelToolPreferences>,
}

/// `{channel_name}:{sender_id}` → Tandem `SessionRecord`
pub type SessionMap = Arc<Mutex<HashMap<String, SessionRecord>>>;
type SetupClarifierMap = Arc<Mutex<HashMap<String, PendingSetupClarifier>>>;
type ChannelSecurityMap = Arc<HashMap<String, ChannelSecurityProfile>>;

const PUBLIC_DEMO_ALLOWED_TOOLS: &[&str] = &[
    "websearch",
    "webfetch",
    "webfetch_html",
    "memory_search",
    "memory_store",
    "memory_list",
];

#[derive(Debug, Clone)]
struct PendingSetupClarifier {
    intent_options: Vec<String>,
    original_text: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupUnderstandRequest<'a> {
    surface: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<&'a str>,
    text: &'a str,
    channel: &'a str,
    trigger: SetupTriggerPayload<'a>,
    scope: SetupScopePayload<'a>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupTriggerPayload<'a> {
    source: &'a str,
    is_direct_message: bool,
    was_explicitly_mentioned: bool,
    is_reply_to_bot: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SetupScopePayload<'a> {
    kind: &'a str,
    id: &'a str,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
struct SetupUnderstandResponse {
    decision: SetupDecision,
    intent_kind: SetupIntentKind,
    #[allow(dead_code)]
    confidence: f32,
    #[allow(dead_code)]
    slots: SetupUnderstandSlots,
    #[allow(dead_code)]
    evidence: Vec<SetupEvidence>,
    clarifier: Option<SetupClarifier>,
    proposed_action: SetupProposedAction,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SetupDecision {
    PassThrough,
    Intercept,
    Clarify,
}

#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SetupIntentKind {
    ProviderSetup,
    IntegrationSetup,
    AutomationCreate,
    ChannelSetupHelp,
    SetupHelp,
    General,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct SetupUnderstandSlots {
    #[serde(default)]
    provider_ids: Vec<String>,
    #[serde(default)]
    model_ids: Vec<String>,
    #[serde(default)]
    integration_targets: Vec<String>,
    #[serde(default)]
    channel_targets: Vec<String>,
    goal: Option<String>,
    schedule_hint: Option<String>,
    delivery_target: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
struct SetupEvidence {
    kind: String,
    value: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SetupClarifier {
    question: String,
    options: Vec<SetupClarifierOption>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct SetupClarifierOption {
    id: String,
    label: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct SetupProposedAction {
    #[serde(rename = "type")]
    action_type: String,
    #[serde(default)]
    payload: serde_json::Value,
}

fn session_scope_kind_label(msg: &ChannelMessage) -> &'static str {
    match msg.scope.kind {
        crate::traits::ConversationScopeKind::Direct => "direct",
        crate::traits::ConversationScopeKind::Room => "room",
        crate::traits::ConversationScopeKind::Thread => "thread",
        crate::traits::ConversationScopeKind::Topic => "topic",
    }
}

fn session_map_key(msg: &ChannelMessage) -> String {
    format!("{}:{}:{}", msg.channel, msg.scope.id, msg.sender)
}

fn legacy_session_map_key(msg: &ChannelMessage) -> String {
    format!("{}:{}", msg.channel, msg.sender)
}

fn public_channel_memory_scope_key(msg: &ChannelMessage) -> String {
    let scope = msg
        .scope
        .id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    format!("channel-public::{}::{}", msg.channel, scope)
}

fn session_title_prefix(msg: &ChannelMessage) -> String {
    format!("{} — {} — {}", msg.channel, msg.sender, msg.scope.id)
}

fn tool_preferences_path() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_tool_preferences.json")
}

async fn load_tool_preferences() -> HashMap<String, ChannelToolPreferences> {
    let path = tool_preferences_path();
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

async fn save_tool_preferences(map: &HashMap<String, ChannelToolPreferences>) {
    let path = tool_preferences_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_vec_pretty(map) {
        let _ = tokio::fs::write(&path, json).await;
    }
}

async fn load_channel_tool_preferences(channel: &str, scope_id: &str) -> ChannelToolPreferences {
    let map = load_tool_preferences().await;
    map.get(&format!("{}:{}", channel, scope_id))
        .cloned()
        .unwrap_or_default()
}

async fn save_channel_tool_preferences(
    channel: &str,
    scope_id: &str,
    prefs: ChannelToolPreferences,
) {
    let mut map = load_tool_preferences().await;
    map.insert(format!("{}:{}", channel, scope_id), prefs);
    save_tool_preferences(&map).await;
}

fn persistence_path() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_sessions.json")
}

/// Load the session map from disk. Returns an empty map if the file doesn't
/// exist or cannot be parsed.
async fn load_session_map() -> HashMap<String, SessionRecord> {
    let path = persistence_path();
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return HashMap::new();
    };

    if let Ok(map) = serde_json::from_slice::<HashMap<String, SessionRecord>>(&bytes) {
        return map;
    }

    // Migration from old String format
    if let Ok(old_map) = serde_json::from_slice::<HashMap<String, String>>(&bytes) {
        let mut new_map = HashMap::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        for (key, session_id) in old_map {
            let mut parts = key.splitn(2, ':');
            let channel = parts.next().unwrap_or("unknown").to_string();
            let sender = parts.next().unwrap_or("unknown").to_string();
            new_map.insert(
                key,
                SessionRecord {
                    session_id,
                    created_at_ms: now,
                    last_seen_at_ms: now,
                    channel,
                    sender,
                    scope_id: None,
                    scope_kind: None,
                    tool_preferences: None,
                },
            );
        }
        return new_map;
    }

    HashMap::new()
}

/// Persist the session map to disk. Silently ignores I/O errors.
async fn save_session_map(map: &HashMap<String, SessionRecord>) {
    let path = persistence_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_vec_pretty(map) {
        let _ = tokio::fs::write(&path, json).await;
    }
}

async fn persist_session_map(map: &HashMap<String, SessionRecord>) {
    save_session_map(map).await;
}

// ---------------------------------------------------------------------------
// Slash command parsing
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum SlashCommand {
    New { name: Option<String> },
    ListSessions,
    Resume { query: String },
    Rename { name: String },
    Status,
    Run,
    Cancel,
    Todos,
    Requests,
    Answer { question_id: String, answer: String },
    Providers,
    Models { provider: Option<String> },
    Model { model_id: String },
    Help { topic: Option<String> },
    Approve { tool_call_id: String },
    Deny { tool_call_id: String },
    Schedule { action: ScheduleCommand },
    Automations { action: AutomationsCommand },
    Runs { action: RunsCommand },
    Memory { action: MemoryCommand },
    Workspace { action: WorkspaceCommand },
    Tools { action: ToolsCommand },
    Mcp { action: McpCommand },
    Packs { action: PacksCommand },
    Config { action: ConfigCommand },
}

#[derive(Debug)]
enum ToolsCommand {
    Help,
    List,
    Enable { tools: Vec<String> },
    Disable { tools: Vec<String> },
    Reset,
}

#[derive(Debug)]
enum ScheduleCommand {
    Help,
    Plan { prompt: String },
    Show { plan_id: String },
    Edit { plan_id: String, message: String },
    Reset { plan_id: String },
    Apply { plan_id: String },
}

#[derive(Debug)]
enum AutomationsCommand {
    Help,
    List,
    Show {
        automation_id: String,
    },
    Runs {
        automation_id: String,
        limit: usize,
    },
    Run {
        automation_id: String,
    },
    Pause {
        automation_id: String,
    },
    Resume {
        automation_id: String,
    },
    Delete {
        automation_id: String,
        confirmed: bool,
    },
}

#[derive(Debug)]
enum RunsCommand {
    Help,
    Automations { limit: usize },
    Show { run_id: String },
    Pause { run_id: String },
    Resume { run_id: String },
    Cancel { run_id: String },
    Artifacts { run_id: String },
}

#[derive(Debug)]
enum MemoryCommand {
    Help,
    Search { query: String },
    Recent { limit: usize },
    Save { text: String },
    Scopes,
    Delete { memory_id: String, confirmed: bool },
}

#[derive(Debug)]
enum WorkspaceCommand {
    Help,
    Show,
    Status,
    Files { query: String },
    Branch,
}

#[derive(Debug)]
enum McpCommand {
    Help,
    List,
    Tools { server: Option<String> },
    Resources,
    Status,
    Connect { name: String },
    Disconnect { name: String },
    Refresh { name: String },
    ChannelEnable { name: String },
    ChannelDisable { name: String },
}

#[derive(Debug)]
enum PacksCommand {
    Help,
    List,
    Show { selector: String },
    Updates { selector: String },
    Install { target: String },
    Uninstall { selector: String, confirmed: bool },
}

#[derive(Debug)]
enum ConfigCommand {
    Help,
    Show,
    Providers,
    Channels,
    Model,
    SetModel { model_id: String },
}

fn parse_slash_command(content: &str) -> Option<SlashCommand> {
    let trimmed = content.trim();
    if trimmed == "/new" {
        return Some(SlashCommand::New { name: None });
    }
    if let Some(name) = trimmed.strip_prefix("/new ") {
        return Some(SlashCommand::New {
            name: Some(name.trim().to_string()),
        });
    }
    if trimmed == "/sessions" || trimmed == "/session" {
        return Some(SlashCommand::ListSessions);
    }
    if let Some(q) = trimmed.strip_prefix("/resume ") {
        return Some(SlashCommand::Resume {
            query: q.trim().to_string(),
        });
    }
    if let Some(name) = trimmed.strip_prefix("/rename ") {
        return Some(SlashCommand::Rename {
            name: name.trim().to_string(),
        });
    }
    if trimmed == "/status" {
        return Some(SlashCommand::Status);
    }
    if trimmed == "/run" {
        return Some(SlashCommand::Run);
    }
    if trimmed == "/cancel" || trimmed == "/abort" {
        return Some(SlashCommand::Cancel);
    }
    if trimmed == "/todos" || trimmed == "/todo" {
        return Some(SlashCommand::Todos);
    }
    if trimmed == "/requests" {
        return Some(SlashCommand::Requests);
    }
    if trimmed == "/tools" || trimmed == "/tools help" {
        return Some(SlashCommand::Tools {
            action: ToolsCommand::Help,
        });
    }
    if trimmed == "/tools list" {
        return Some(SlashCommand::Tools {
            action: ToolsCommand::List,
        });
    }
    if trimmed == "/tools reset" {
        return Some(SlashCommand::Tools {
            action: ToolsCommand::Reset,
        });
    }
    if let Some(rest) = trimmed.strip_prefix("/tools enable ") {
        let tools = rest
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            return Some(SlashCommand::Tools {
                action: ToolsCommand::Enable { tools },
            });
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/tools disable ") {
        let tools = rest
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            return Some(SlashCommand::Tools {
                action: ToolsCommand::Disable { tools },
            });
        }
        return None;
    }
    if trimmed == "/providers" {
        return Some(SlashCommand::Providers);
    }
    if trimmed == "/models" {
        return Some(SlashCommand::Models { provider: None });
    }
    if let Some(provider) = trimmed.strip_prefix("/models ") {
        return Some(SlashCommand::Models {
            provider: Some(provider.trim().to_string()),
        });
    }
    if let Some(model_id) = trimmed.strip_prefix("/model ") {
        let model_id = model_id.trim();
        if !model_id.is_empty() {
            return Some(SlashCommand::Model {
                model_id: model_id.to_string(),
            });
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/answer ") {
        let mut parts = rest.trim().splitn(2, ' ');
        let question_id = parts.next().unwrap_or_default().trim();
        let answer = parts.next().unwrap_or_default().trim();
        if !question_id.is_empty() && !answer.is_empty() {
            return Some(SlashCommand::Answer {
                question_id: question_id.to_string(),
                answer: answer.to_string(),
            });
        }
        return None;
    }
    if trimmed == "/help" || trimmed == "/?" {
        return Some(SlashCommand::Help { topic: None });
    }
    if let Some(topic) = trimmed.strip_prefix("/help ") {
        let topic = topic.trim();
        if !topic.is_empty() {
            return Some(SlashCommand::Help {
                topic: Some(topic.to_string()),
            });
        }
        return Some(SlashCommand::Help { topic: None });
    }
    if let Some(id) = trimmed.strip_prefix("/approve ") {
        return Some(SlashCommand::Approve {
            tool_call_id: id.trim().to_string(),
        });
    }
    if let Some(id) = trimmed.strip_prefix("/deny ") {
        return Some(SlashCommand::Deny {
            tool_call_id: id.trim().to_string(),
        });
    }
    if trimmed == "/schedule" {
        return Some(SlashCommand::Schedule {
            action: ScheduleCommand::Help,
        });
    }
    if trimmed == "/schedule help" {
        return Some(SlashCommand::Schedule {
            action: ScheduleCommand::Help,
        });
    }
    if let Some(prompt) = trimmed.strip_prefix("/schedule plan ") {
        let prompt = prompt.trim();
        if !prompt.is_empty() {
            return Some(SlashCommand::Schedule {
                action: ScheduleCommand::Plan {
                    prompt: prompt.to_string(),
                },
            });
        }
        return None;
    }
    if let Some(plan_id) = trimmed.strip_prefix("/schedule show ") {
        let plan_id = plan_id.trim();
        if !plan_id.is_empty() {
            return Some(SlashCommand::Schedule {
                action: ScheduleCommand::Show {
                    plan_id: plan_id.to_string(),
                },
            });
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/schedule edit ") {
        let mut parts = rest.trim().splitn(2, ' ');
        let plan_id = parts.next().unwrap_or_default().trim();
        let message = parts.next().unwrap_or_default().trim();
        if !plan_id.is_empty() && !message.is_empty() {
            return Some(SlashCommand::Schedule {
                action: ScheduleCommand::Edit {
                    plan_id: plan_id.to_string(),
                    message: message.to_string(),
                },
            });
        }
        return None;
    }
    if let Some(plan_id) = trimmed.strip_prefix("/schedule reset ") {
        let plan_id = plan_id.trim();
        if !plan_id.is_empty() {
            return Some(SlashCommand::Schedule {
                action: ScheduleCommand::Reset {
                    plan_id: plan_id.to_string(),
                },
            });
        }
        return None;
    }
    if let Some(plan_id) = trimmed.strip_prefix("/schedule apply ") {
        let plan_id = plan_id.trim();
        if !plan_id.is_empty() {
            return Some(SlashCommand::Schedule {
                action: ScheduleCommand::Apply {
                    plan_id: plan_id.to_string(),
                },
            });
        }
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("/automations") {
        return parse_automations_command(rest).map(|action| SlashCommand::Automations { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/runs") {
        return parse_runs_command(rest).map(|action| SlashCommand::Runs { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/memory") {
        return parse_memory_command(rest).map(|action| SlashCommand::Memory { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/workspace") {
        return parse_workspace_command(rest).map(|action| SlashCommand::Workspace { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/mcp") {
        return parse_mcp_command(rest).map(|action| SlashCommand::Mcp { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/packs") {
        return parse_packs_command(rest).map(|action| SlashCommand::Packs { action });
    }
    if let Some(rest) = trimmed.strip_prefix("/config") {
        return parse_config_command(rest).map(|action| SlashCommand::Config { action });
    }
    None
}

fn parse_limit_token(input: Option<&str>, default: usize) -> usize {
    input
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn parse_automations_command(rest: &str) -> Option<AutomationsCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "list" {
        return Some(AutomationsCommand::List);
    }
    if rest == "help" {
        return Some(AutomationsCommand::Help);
    }
    if let Some(automation_id) = rest.strip_prefix("show ") {
        return Some(AutomationsCommand::Show {
            automation_id: automation_id.trim().to_string(),
        });
    }
    if let Some(args) = rest.strip_prefix("runs ") {
        let mut parts = args.trim().split_whitespace();
        let automation_id = parts.next()?.trim();
        let limit = parse_limit_token(parts.next(), 10);
        return Some(AutomationsCommand::Runs {
            automation_id: automation_id.to_string(),
            limit,
        });
    }
    if let Some(automation_id) = rest.strip_prefix("run ") {
        return Some(AutomationsCommand::Run {
            automation_id: automation_id.trim().to_string(),
        });
    }
    if let Some(automation_id) = rest.strip_prefix("pause ") {
        return Some(AutomationsCommand::Pause {
            automation_id: automation_id.trim().to_string(),
        });
    }
    if let Some(automation_id) = rest.strip_prefix("resume ") {
        return Some(AutomationsCommand::Resume {
            automation_id: automation_id.trim().to_string(),
        });
    }
    if let Some(args) = rest.strip_prefix("delete ") {
        let confirmed = args.contains("--yes");
        let automation_id = args.replace("--yes", "").trim().to_string();
        if !automation_id.is_empty() {
            return Some(AutomationsCommand::Delete {
                automation_id,
                confirmed,
            });
        }
    }
    None
}

fn parse_runs_command(rest: &str) -> Option<RunsCommand> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(RunsCommand::Automations { limit: 10 });
    }
    if rest == "help" {
        return Some(RunsCommand::Help);
    }
    if rest == "automations" {
        return Some(RunsCommand::Automations { limit: 10 });
    }
    if let Some(limit) = rest.strip_prefix("automations ") {
        return Some(RunsCommand::Automations {
            limit: parse_limit_token(Some(limit), 10),
        });
    }
    if let Some(run_id) = rest.strip_prefix("show ") {
        return Some(RunsCommand::Show {
            run_id: run_id.trim().to_string(),
        });
    }
    if let Some(run_id) = rest.strip_prefix("pause ") {
        return Some(RunsCommand::Pause {
            run_id: run_id.trim().to_string(),
        });
    }
    if let Some(run_id) = rest.strip_prefix("resume ") {
        return Some(RunsCommand::Resume {
            run_id: run_id.trim().to_string(),
        });
    }
    if let Some(run_id) = rest.strip_prefix("cancel ") {
        return Some(RunsCommand::Cancel {
            run_id: run_id.trim().to_string(),
        });
    }
    if let Some(run_id) = rest.strip_prefix("artifacts ") {
        return Some(RunsCommand::Artifacts {
            run_id: run_id.trim().to_string(),
        });
    }
    None
}

fn parse_memory_command(rest: &str) -> Option<MemoryCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "recent" {
        return Some(MemoryCommand::Recent { limit: 10 });
    }
    if rest == "help" {
        return Some(MemoryCommand::Help);
    }
    if let Some(limit) = rest.strip_prefix("recent ") {
        return Some(MemoryCommand::Recent {
            limit: parse_limit_token(Some(limit), 10),
        });
    }
    if let Some(query) = rest.strip_prefix("search ") {
        let query = query.trim();
        if !query.is_empty() {
            return Some(MemoryCommand::Search {
                query: query.to_string(),
            });
        }
        return None;
    }
    if let Some(text) = rest.strip_prefix("save ") {
        let text = text.trim();
        if !text.is_empty() {
            return Some(MemoryCommand::Save {
                text: text.to_string(),
            });
        }
        return None;
    }
    if rest == "scopes" {
        return Some(MemoryCommand::Scopes);
    }
    if let Some(args) = rest.strip_prefix("delete ") {
        let confirmed = args.contains("--yes");
        let memory_id = args.replace("--yes", "").trim().to_string();
        if !memory_id.is_empty() {
            return Some(MemoryCommand::Delete {
                memory_id,
                confirmed,
            });
        }
    }
    None
}

fn parse_workspace_command(rest: &str) -> Option<WorkspaceCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "show" {
        return Some(WorkspaceCommand::Show);
    }
    if rest == "help" {
        return Some(WorkspaceCommand::Help);
    }
    if rest == "status" {
        return Some(WorkspaceCommand::Status);
    }
    if rest == "branch" {
        return Some(WorkspaceCommand::Branch);
    }
    if let Some(query) = rest.strip_prefix("files ") {
        let query = query.trim();
        if !query.is_empty() {
            return Some(WorkspaceCommand::Files {
                query: query.to_string(),
            });
        }
        return None;
    }
    None
}

fn parse_mcp_command(rest: &str) -> Option<McpCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "list" {
        return Some(McpCommand::List);
    }
    if rest == "help" {
        return Some(McpCommand::Help);
    }
    if rest == "tools" {
        return Some(McpCommand::Tools { server: None });
    }
    if let Some(server) = rest.strip_prefix("tools ") {
        let server = server.trim();
        return Some(McpCommand::Tools {
            server: if server.is_empty() {
                None
            } else {
                Some(server.to_string())
            },
        });
    }
    if rest == "resources" {
        return Some(McpCommand::Resources);
    }
    if rest == "status" {
        return Some(McpCommand::Status);
    }
    if let Some(name) = rest.strip_prefix("connect ") {
        return Some(McpCommand::Connect {
            name: name.trim().to_string(),
        });
    }
    if let Some(name) = rest.strip_prefix("disconnect ") {
        return Some(McpCommand::Disconnect {
            name: name.trim().to_string(),
        });
    }
    if let Some(name) = rest.strip_prefix("enable ") {
        return Some(McpCommand::ChannelEnable {
            name: name.trim().to_string(),
        });
    }
    if let Some(name) = rest.strip_prefix("disable ") {
        return Some(McpCommand::ChannelDisable {
            name: name.trim().to_string(),
        });
    }
    if let Some(name) = rest.strip_prefix("refresh ") {
        return Some(McpCommand::Refresh {
            name: name.trim().to_string(),
        });
    }
    None
}

fn parse_packs_command(rest: &str) -> Option<PacksCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "list" {
        return Some(PacksCommand::List);
    }
    if rest == "help" {
        return Some(PacksCommand::Help);
    }
    if let Some(selector) = rest.strip_prefix("show ") {
        return Some(PacksCommand::Show {
            selector: selector.trim().to_string(),
        });
    }
    if let Some(selector) = rest.strip_prefix("updates ") {
        return Some(PacksCommand::Updates {
            selector: selector.trim().to_string(),
        });
    }
    if let Some(target) = rest.strip_prefix("install ") {
        let target = target.trim();
        if !target.is_empty() {
            return Some(PacksCommand::Install {
                target: target.to_string(),
            });
        }
        return None;
    }
    if let Some(args) = rest.strip_prefix("uninstall ") {
        let confirmed = args.contains("--yes");
        let selector = args.replace("--yes", "").trim().to_string();
        if !selector.is_empty() {
            return Some(PacksCommand::Uninstall {
                selector,
                confirmed,
            });
        }
    }
    None
}

fn parse_config_command(rest: &str) -> Option<ConfigCommand> {
    let rest = rest.trim();
    if rest.is_empty() || rest == "show" {
        return Some(ConfigCommand::Show);
    }
    if rest == "help" {
        return Some(ConfigCommand::Help);
    }
    if rest == "providers" {
        return Some(ConfigCommand::Providers);
    }
    if rest == "channels" {
        return Some(ConfigCommand::Channels);
    }
    if rest == "model" {
        return Some(ConfigCommand::Model);
    }
    if let Some(model_id) = rest.strip_prefix("set-model ") {
        let model_id = model_id.trim();
        if !model_id.is_empty() {
            return Some(ConfigCommand::SetModel {
                model_id: model_id.to_string(),
            });
        }
        return None;
    }
    None
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Start all configured channel listeners. Returns a `JoinSet` that the caller
/// can `.abort_all()` on shutdown.
pub async fn start_channel_listeners(config: ChannelsConfig) -> JoinSet<()> {
    let initial_map = load_session_map().await;
    info!(
        "tandem-channels: loaded {} persisted session mappings",
        initial_map.len()
    );

    let session_map: SessionMap = Arc::new(Mutex::new(initial_map));
    let setup_clarifiers: SetupClarifierMap = Arc::new(Mutex::new(HashMap::new()));
    let mut security_profiles = HashMap::new();
    let mut set = JoinSet::new();

    if let Some(tg) = config.telegram {
        security_profiles.insert("telegram".to_string(), tg.security_profile);
        let channel = Arc::new(TelegramChannel::new(tg));
        let map = session_map.clone();
        let clarifiers = setup_clarifiers.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        let profiles = Arc::new(security_profiles.clone());
        set.spawn(supervise(
            channel, base_url, api_token, map, clarifiers, profiles,
        ));
        info!("tandem-channels: Telegram listener started");
    }

    if let Some(dc) = config.discord {
        security_profiles.insert("discord".to_string(), dc.security_profile);
        let channel = Arc::new(DiscordChannel::new(dc));
        let map = session_map.clone();
        let clarifiers = setup_clarifiers.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        let profiles = Arc::new(security_profiles.clone());
        set.spawn(supervise(
            channel, base_url, api_token, map, clarifiers, profiles,
        ));
        info!("tandem-channels: Discord listener started");
    }

    if let Some(sl) = config.slack {
        security_profiles.insert("slack".to_string(), sl.security_profile);
        let channel = Arc::new(SlackChannel::new(sl));
        let map = session_map.clone();
        let clarifiers = setup_clarifiers.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        let profiles = Arc::new(security_profiles.clone());
        set.spawn(supervise(
            channel, base_url, api_token, map, clarifiers, profiles,
        ));
        info!("tandem-channels: Slack listener started");
    }

    set
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Runs a channel listener with exponential-backoff restart on failure.
async fn supervise(
    channel: Arc<dyn Channel>,
    base_url: String,
    api_token: String,
    session_map: SessionMap,
    setup_clarifiers: SetupClarifierMap,
    security_profiles: ChannelSecurityMap,
) {
    let mut backoff_secs: u64 = 1;
    loop {
        let (tx, mut rx) = mpsc::channel::<ChannelMessage>(64);

        let channel_listen = channel.clone();
        let listen_handle = tokio::spawn(async move {
            if let Err(e) = channel_listen.listen(tx).await {
                error!("channel listener error: {e}");
            }
        });

        while let Some(msg) = rx.recv().await {
            let ch = channel.clone();
            let base = base_url.clone();
            let tok = api_token.clone();
            let map = session_map.clone();
            let clarifiers = setup_clarifiers.clone();
            let profiles = security_profiles.clone();
            tokio::spawn(async move {
                process_channel_message(msg, ch, &base, &tok, &map, &clarifiers, &profiles).await;
            });
        }

        listen_handle.abort();

        if channel.health_check().await {
            backoff_secs = 1;
        } else {
            warn!(
                "channel '{}' unhealthy — restarting in {}s",
                channel.name(),
                backoff_secs
            );
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(60);
        }
    }
}

// ---------------------------------------------------------------------------
// Message processing
// ---------------------------------------------------------------------------

/// Process a single incoming channel message: handle slash commands or forward
/// to the Tandem session HTTP API.
async fn process_channel_message(
    msg: ChannelMessage,
    channel: Arc<dyn Channel>,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    setup_clarifiers: &SetupClarifierMap,
    security_profiles: &ChannelSecurityMap,
) {
    let security_profile = channel_security_profile(&msg.channel, security_profiles);
    // --- Slash command intercept ---
    if msg.content.starts_with('/') {
        if let Some(cmd) = parse_slash_command(&msg.content) {
            let response = handle_slash_command(
                cmd,
                &msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await;
            if let Err(e) = channel
                .send(&SendMessage {
                    content: response,
                    recipient: msg.reply_target.clone(),
                    image_urls: Vec::new(),
                })
                .await
            {
                error!(
                    "failed to send slash-command response via channel '{}': {e}",
                    channel.name()
                );
            }
            return;
        }
    }

    let thread_key = format!("{}:{}", msg.channel, msg.reply_target);

    if let Some(cmd) = parse_pack_builder_reply_command(&msg.content) {
        let map_key = session_map_key(&msg);
        let session_id =
            get_or_create_session(&msg, base_url, api_token, session_map, security_profile).await;
        let session_id = match session_id {
            Some(id) => id,
            None => {
                error!("failed to get or create session for {}", map_key);
                return;
            }
        };
        let response = match cmd {
            PackBuilderReplyCommand::Confirm => {
                apply_pending_pack_builder(
                    base_url,
                    api_token,
                    &session_id,
                    &thread_key,
                    None,
                    false,
                )
                .await
            }
            PackBuilderReplyCommand::Cancel => {
                cancel_pending_pack_builder(base_url, api_token, &session_id, &thread_key).await
            }
            PackBuilderReplyCommand::UseConnectors(connectors) => {
                apply_pending_pack_builder(
                    base_url,
                    api_token,
                    &session_id,
                    &thread_key,
                    Some(connectors),
                    true,
                )
                .await
            }
        };
        if let Some(reply) = response {
            if let Err(e) = channel
                .send(&SendMessage {
                    content: reply,
                    recipient: msg.reply_target,
                    image_urls: Vec::new(),
                })
                .await
            {
                error!(
                    "failed to send pack-builder channel reply via '{}': {e}",
                    channel.name()
                );
            }
            return;
        }
    }

    let conversation_key = session_map_key(&msg);
    let clarified_text =
        consume_setup_clarifier_reply(&conversation_key, &msg.content, setup_clarifiers).await;
    let effective_text = clarified_text.as_deref().unwrap_or(&msg.content);
    let setup_response =
        match understand_setup_request(base_url, api_token, &msg, None, effective_text).await {
            Ok(response) => response,
            Err(err) => {
                warn!(
                    "setup understanding failed for channel '{}': {err}",
                    channel.name()
                );
                SetupUnderstandResponse {
                    decision: SetupDecision::PassThrough,
                    intent_kind: SetupIntentKind::General,
                    confidence: 0.0,
                    slots: SetupUnderstandSlots::default(),
                    evidence: Vec::new(),
                    clarifier: None,
                    proposed_action: SetupProposedAction::default(),
                }
            }
        };

    if setup_response.decision == SetupDecision::Clarify {
        if let Some(clarifier) = &setup_response.clarifier {
            remember_setup_clarifier(
                conversation_key.clone(),
                clarifier,
                effective_text.to_string(),
                setup_clarifiers,
            )
            .await;
            let reply = format_setup_clarifier_message(clarifier);
            if let Err(e) = channel
                .send(&SendMessage {
                    content: reply,
                    recipient: msg.reply_target,
                    image_urls: Vec::new(),
                })
                .await
            {
                error!(
                    "failed to send setup clarifier via '{}': {e}",
                    channel.name()
                );
            }
            return;
        }
    }

    if setup_response.decision == SetupDecision::Intercept {
        match setup_response.intent_kind {
            SetupIntentKind::AutomationCreate => {
                let map_key = session_map_key(&msg);
                let session_id =
                    get_or_create_session(&msg, base_url, api_token, session_map, security_profile)
                        .await;
                let session_id = match session_id {
                    Some(id) => id,
                    None => {
                        error!("failed to get or create session for {}", map_key);
                        return;
                    }
                };
                match preview_setup_automation(
                    base_url,
                    api_token,
                    &session_id,
                    &thread_key,
                    effective_text,
                )
                .await
                {
                    Some(reply) => {
                        if let Err(e) = channel
                            .send(&SendMessage {
                                content: reply,
                                recipient: msg.reply_target,
                                image_urls: Vec::new(),
                            })
                            .await
                        {
                            error!(
                                "failed to send setup automation preview via '{}': {e}",
                                channel.name()
                            );
                        }
                    }
                    None => warn!("pack builder preview did not return a reply"),
                }
                return;
            }
            SetupIntentKind::ProviderSetup
            | SetupIntentKind::IntegrationSetup
            | SetupIntentKind::ChannelSetupHelp
            | SetupIntentKind::SetupHelp => {
                let reply = format_setup_guidance_message(&setup_response);
                if let Err(e) = channel
                    .send(&SendMessage {
                        content: reply,
                        recipient: msg.reply_target,
                        image_urls: Vec::new(),
                    })
                    .await
                {
                    error!(
                        "failed to send setup guidance via '{}': {e}",
                        channel.name()
                    );
                }
                return;
            }
            SetupIntentKind::General => {}
        }
    }

    if let Err(e) = channel.start_typing(&msg.reply_target).await {
        warn!(
            "failed to start typing indicator for channel '{}': {e}",
            channel.name()
        );
    }

    let map_key = session_map_key(&msg);
    let session_id =
        get_or_create_session(&msg, base_url, api_token, session_map, security_profile).await;
    let session_id = match session_id {
        Some(id) => id,
        None => {
            error!("failed to get or create session for {}", map_key);
            let _ = channel.stop_typing(&msg.reply_target).await;
            return;
        }
    };
    let mut prompt_content = effective_text.to_string();
    if let Some(attachment) = msg.attachment.as_deref() {
        if is_zip_attachment(&msg) {
            if let Some(pack_reply) =
                handle_pack_attachment_if_present(&msg, base_url, api_token).await
            {
                if let Err(e) = channel.stop_typing(&msg.reply_target).await {
                    warn!(
                        "failed to stop typing indicator for channel '{}': {e}",
                        channel.name()
                    );
                }
                if let Err(e) = channel
                    .send(&SendMessage {
                        content: pack_reply,
                        recipient: msg.reply_target.clone(),
                        image_urls: Vec::new(),
                    })
                    .await
                {
                    error!(
                        "failed to send pack ingestion reply via '{}': {e}",
                        channel.name()
                    );
                }
                return;
            }
        }
        let persisted = persist_channel_attachment_reference(
            base_url,
            api_token,
            &session_id,
            &msg,
            attachment,
        )
        .await;
        prompt_content = synthesize_attachment_prompt(
            &msg.channel,
            attachment,
            effective_text,
            persisted.as_deref(),
            msg.attachment_path.as_deref(),
            msg.attachment_url.as_deref(),
            msg.attachment_filename.as_deref(),
            msg.attachment_mime.as_deref(),
        );
    }

    let route = route_agent_for_channel_message(effective_text);
    let tool_prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
    let effective_allowlist =
        build_channel_tool_allowlist(route.tool_allowlist.as_ref(), &tool_prefs, security_profile);

    let response = run_in_session(
        &session_id,
        &prompt_content,
        base_url,
        api_token,
        msg.attachment_path.as_deref(),
        msg.attachment_url.as_deref(),
        msg.attachment_mime.as_deref(),
        msg.attachment_filename.as_deref(),
        route.agent.as_deref(),
        effective_allowlist.as_ref(),
    )
    .await;
    if let Err(e) = channel.stop_typing(&msg.reply_target).await {
        warn!(
            "failed to stop typing indicator for channel '{}': {e}",
            channel.name()
        );
    }

    let reply = response.unwrap_or_else(|e| format!("⚠️ Error: {e}"));
    let (reply_text, image_urls) = extract_image_urls_and_clean_text(&reply);
    if let Err(e) = channel
        .send(&SendMessage {
            content: reply_text,
            recipient: msg.reply_target,
            image_urls,
        })
        .await
    {
        error!("failed to send channel reply via '{}': {e}", channel.name());
    }
}

fn parse_pack_builder_reply_command(content: &str) -> Option<PackBuilderReplyCommand> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "ok" | "okay"
            | "yes"
            | "y"
            | "confirm"
            | "confirmed"
            | "approve"
            | "approved"
            | "go"
            | "go ahead"
            | "proceed"
            | "do it"
            | "run it"
            | "apply"
    ) {
        return Some(PackBuilderReplyCommand::Confirm);
    }
    if matches!(lower.as_str(), "cancel" | "stop" | "abort") {
        return Some(PackBuilderReplyCommand::Cancel);
    }
    if let Some(rest) = trimmed
        .to_ascii_lowercase()
        .strip_prefix("use connectors:")
        .map(ToString::to_string)
    {
        let connectors = rest
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if !connectors.is_empty() {
            return Some(PackBuilderReplyCommand::UseConnectors(connectors));
        }
    }
    None
}

fn trigger_source_label(source: &crate::traits::TriggerSource) -> &'static str {
    match source {
        crate::traits::TriggerSource::SlashCommand => "slash_command",
        crate::traits::TriggerSource::DirectMessage => "direct_message",
        crate::traits::TriggerSource::Mention => "mention",
        crate::traits::TriggerSource::ReplyToBot => "reply_to_bot",
        crate::traits::TriggerSource::Ambient => "ambient",
    }
}

fn scope_kind_label(scope: &crate::traits::ConversationScopeKind) -> &'static str {
    match scope {
        crate::traits::ConversationScopeKind::Direct => "direct",
        crate::traits::ConversationScopeKind::Room => "room",
        crate::traits::ConversationScopeKind::Thread => "thread",
        crate::traits::ConversationScopeKind::Topic => "topic",
    }
}

async fn understand_setup_request(
    base_url: &str,
    api_token: &str,
    msg: &ChannelMessage,
    session_id: Option<&str>,
    text: &str,
) -> anyhow::Result<SetupUnderstandResponse> {
    let client = reqwest::Client::new();
    let request = SetupUnderstandRequest {
        surface: "channel",
        session_id,
        text,
        channel: &msg.channel,
        trigger: SetupTriggerPayload {
            source: trigger_source_label(&msg.trigger.source),
            is_direct_message: msg.trigger.is_direct_message,
            was_explicitly_mentioned: msg.trigger.was_explicitly_mentioned,
            is_reply_to_bot: msg.trigger.is_reply_to_bot,
        },
        scope: SetupScopePayload {
            kind: scope_kind_label(&msg.scope.kind),
            id: &msg.scope.id,
        },
    };
    let resp = add_auth(
        client
            .post(format!("{base_url}/setup/understand"))
            .json(&request),
        api_token,
    )
    .send()
    .await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("setup/understand failed ({}): {}", status, body);
    }
    Ok(serde_json::from_str(&body)?)
}

async fn remember_setup_clarifier(
    conversation_key: String,
    clarifier: &SetupClarifier,
    original_text: String,
    setup_clarifiers: &SetupClarifierMap,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let pending = PendingSetupClarifier {
        intent_options: clarifier.options.iter().map(|row| row.id.clone()).collect(),
        original_text,
        expires_at_ms: now + 5 * 60 * 1000,
    };
    let mut guard = setup_clarifiers.lock().await;
    guard.insert(conversation_key, pending);
}

async fn consume_setup_clarifier_reply(
    conversation_key: &str,
    reply: &str,
    setup_clarifiers: &SetupClarifierMap,
) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut guard = setup_clarifiers.lock().await;
    guard.retain(|_, value| value.expires_at_ms > now);
    let pending = guard.get(conversation_key)?.clone();
    let selected = parse_setup_clarifier_selection(reply, &pending.intent_options)?;
    guard.remove(conversation_key);
    Some(format!("{} {}", pending.original_text, selected))
}

fn parse_setup_clarifier_selection(reply: &str, options: &[String]) -> Option<String> {
    let normalized = reply.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    match normalized.as_str() {
        "1" => return options.first().cloned(),
        "2" => return options.get(1).cloned(),
        "3" => return options.get(2).cloned(),
        _ => {}
    }
    options.iter().find_map(|option| {
        let normalized_option = option.to_ascii_lowercase();
        if normalized == normalized_option
            || normalized.contains(&normalized_option.replace('_', " "))
        {
            Some(option.clone())
        } else {
            None
        }
    })
}

fn format_setup_clarifier_message(clarifier: &SetupClarifier) -> String {
    let mut lines = vec![clarifier.question.clone()];
    for (index, option) in clarifier.options.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, option.label));
    }
    lines.join("\n")
}

async fn preview_setup_automation(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
    goal: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = add_auth(
        client
            .post(format!("{base_url}/pack-builder/preview"))
            .json(&serde_json::json!({
                "session_id": session_id,
                "thread_key": thread_key,
                "goal": goal,
                "auto_apply": false
            })),
        api_token,
    )
    .send()
    .await
    .ok()?;
    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some("I understood that as an automation setup request, but I couldn't build a preview right now.".to_string());
    }
    Some(format_pack_builder_preview_message(&payload))
}

fn format_pack_builder_preview_message(payload: &serde_json::Value) -> String {
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("preview_pending");
    let goal = payload
        .get("goal")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Create a useful automation");
    let connectors = payload
        .get("selected_connectors")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let next_actions = payload
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut lines = vec![
        "Automation setup preview".to_string(),
        format!("Goal: {goal}"),
        format!("Status: {status}"),
    ];
    if !connectors.is_empty() {
        lines.push(format!("Connectors: {}", connectors.join(", ")));
    }
    if !next_actions.is_empty() {
        lines.push("Next steps:".to_string());
        for action in next_actions {
            lines.push(format!("- {action}"));
        }
    } else {
        lines.push("Reply `confirm` to apply this preview or `cancel` to discard it.".to_string());
    }
    lines.join("\n")
}

fn format_setup_guidance_message(response: &SetupUnderstandResponse) -> String {
    match response.intent_kind {
        SetupIntentKind::ProviderSetup => {
            let provider = response
                .slots
                .provider_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "a provider".to_string());
            let model = response.slots.model_ids.first().cloned();
            if let Some(model_id) = model {
                format!(
                    "This looks like provider setup. Configure `{provider}` and set the model to `{model_id}` in Settings. Do not paste API keys into channel chat."
                )
            } else {
                format!(
                    "This looks like provider setup. Configure `{provider}` in Settings. Do not paste API keys into channel chat."
                )
            }
        }
        SetupIntentKind::IntegrationSetup => {
            let target = response
                .slots
                .integration_targets
                .first()
                .cloned()
                .unwrap_or_else(|| "that integration".to_string());
            format!(
                "This looks like an MCP or integration setup request. Open the MCP settings for `{target}` and connect or authorize it there."
            )
        }
        SetupIntentKind::ChannelSetupHelp => {
            let target = response
                .slots
                .channel_targets
                .first()
                .cloned()
                .unwrap_or_else(|| "the channel".to_string());
            format!(
                "This looks like channel setup help. Open the channel settings for `{target}` and confirm the bot token, allowed users, and mention-only settings."
            )
        }
        SetupIntentKind::SetupHelp => {
            "I can help with three setup paths here: provider setup, connecting external tools, or creating an automation. Reply with `1`, `2`, or `3`.".to_string()
        }
        SetupIntentKind::AutomationCreate | SetupIntentKind::General => {
            "I couldn't map that setup request cleanly.".to_string()
        }
    }
}

async fn apply_pending_pack_builder(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
    connectors_override: Option<Vec<String>>,
    secret_refs_confirmed: bool,
) -> Option<String> {
    let client = reqwest::Client::new();
    let pending_resp = add_auth(
        client.get(format!("{base_url}/pack-builder/pending")),
        api_token,
    )
    .query(&[("session_id", session_id), ("thread_key", thread_key)])
    .send()
    .await
    .ok()?;
    let pending_status = pending_resp.status();
    let pending_payload: serde_json::Value = pending_resp.json().await.unwrap_or_default();
    if !pending_status.is_success() {
        return Some("No pending pack-builder plan found for this thread.".to_string());
    }
    let plan_id = pending_payload
        .get("pending")
        .and_then(|v| v.get("plan_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if plan_id.is_empty() {
        return Some("No pending pack-builder plan found for this thread.".to_string());
    }

    let apply_resp = add_auth(
        client.post(format!("{base_url}/pack-builder/apply")),
        api_token,
    )
    .json(&serde_json::json!({
        "plan_id": plan_id,
        "session_id": session_id,
        "thread_key": thread_key,
        "selected_connectors": connectors_override.unwrap_or_default(),
        "approvals": {
            "approve_connector_registration": true,
            "approve_pack_install": true,
            "approve_enable_routines": false
        },
        "secret_refs_confirmed": secret_refs_confirmed
    }))
    .send()
    .await
    .ok()?;
    let status = apply_resp.status();
    let payload: serde_json::Value = apply_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack Builder apply failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    Some(format_pack_builder_apply_message(&payload))
}

async fn cancel_pending_pack_builder(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    thread_key: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = add_auth(
        client.post(format!("{base_url}/pack-builder/cancel")),
        api_token,
    )
    .json(&serde_json::json!({
        "session_id": session_id,
        "thread_key": thread_key
    }))
    .send()
    .await
    .ok()?;
    let status = resp.status();
    let payload: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack Builder cancel failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    Some("Pack Builder plan cancelled for this thread.".to_string())
}

fn format_pack_builder_apply_message(payload: &serde_json::Value) -> String {
    let status = payload.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "apply_blocked_missing_secrets" {
        let mut lines = vec![
            "Pack Builder Apply Blocked".to_string(),
            "- Missing required secrets.".to_string(),
        ];
        for row in payload
            .get("required_secrets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            if let Some(secret) = row.as_str() {
                lines.push(format!("  - {}", secret));
            }
        }
        lines.push("- Set secrets, then reply `confirm` again.".to_string());
        return lines.join("\n");
    }
    if status == "apply_blocked_auth" {
        return "Pack Builder Apply Blocked\n- Connector authentication/setup is required.\n- Complete auth and reply `confirm` again.".to_string();
    }
    let pack_name = payload
        .get("pack_installed")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown-pack");
    let pack_version = payload
        .get("pack_installed")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!(
        "Pack Builder Apply Complete\n- Installed: {} {}\n- Routine state: paused by default",
        pack_name, pack_version
    )
}

#[derive(Debug, Default)]
struct AgentRouteDecision {
    agent: Option<String>,
    tool_allowlist: Option<Vec<String>>,
}

fn route_agent_for_channel_message(content: &str) -> AgentRouteDecision {
    if !is_pack_builder_intent(content) {
        return AgentRouteDecision::default();
    }
    AgentRouteDecision {
        agent: Some("pack_builder".to_string()),
        tool_allowlist: Some(vec![
            "pack_builder".to_string(),
            "question".to_string(),
            "websearch".to_string(),
            "webfetch".to_string(),
        ]),
    }
}

fn build_channel_tool_allowlist(
    route_allowlist: Option<&Vec<String>>,
    tool_prefs: &ChannelToolPreferences,
    security_profile: ChannelSecurityProfile,
) -> Option<Vec<String>> {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut allowed = PUBLIC_DEMO_ALLOWED_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .collect::<Vec<_>>();
        if let Some(route_override) = route_allowlist {
            allowed.retain(|tool| route_override.iter().any(|candidate| candidate == tool));
        }
        allowed.retain(|tool| {
            !tool_prefs
                .disabled_tools
                .iter()
                .any(|disabled| disabled == tool)
        });
        if !tool_prefs.enabled_tools.is_empty() {
            allowed.retain(|tool| {
                tool_prefs
                    .enabled_tools
                    .iter()
                    .any(|enabled| enabled == tool)
            });
        }
        return Some(allowed);
    }

    let pack_builder_override = route_allowlist;
    if let Some(pb) = pack_builder_override {
        return Some(pb.clone());
    }

    if tool_prefs.enabled_tools.is_empty() && tool_prefs.disabled_tools.is_empty() {
        return None;
    }

    let all_builtin = [
        "read",
        "glob",
        "ls",
        "list",
        "grep",
        "codesearch",
        "search",
        "websearch",
        "webfetch",
        "webfetch_html",
        "bash",
        "write",
        "edit",
        "apply_patch",
        "todowrite",
        "memory_search",
        "memory_store",
        "memory_list",
        "skill",
        "task",
        "question",
        "pack_builder",
    ];

    let disabled: std::collections::HashSet<&str> = tool_prefs
        .disabled_tools
        .iter()
        .map(|s| s.as_str())
        .collect();

    let explicit_enabled: std::collections::HashSet<&str> = tool_prefs
        .enabled_tools
        .iter()
        .map(|s| s.as_str())
        .collect();

    let has_explicit_enable = !tool_prefs.enabled_tools.is_empty();
    let mut result = Vec::new();

    for tool in all_builtin {
        if disabled.contains(tool) {
            continue;
        }
        if has_explicit_enable && !explicit_enabled.contains(tool) {
            continue;
        }
        result.push(tool.to_string());
    }

    for server in &tool_prefs.enabled_mcp_servers {
        result.push(format!("mcp.{}.*", server));
    }

    if result.is_empty() {
        return None;
    }
    Some(result)
}

fn is_pack_builder_intent(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let mentions_pack =
        lower.contains("pack") || lower.contains("automation") || lower.contains("workflow");
    let mentions_create = lower.contains("create")
        || lower.contains("build")
        || lower.contains("make")
        || lower.contains("generate")
        || lower.contains("setup");
    let mentions_external = lower.contains("notion")
        || lower.contains("slack")
        || lower.contains("stripe")
        || lower.contains("mcp")
        || lower.contains("connector")
        || lower.contains("headline")
        || lower.contains("news");
    mentions_pack && mentions_create && mentions_external
}

fn is_zip_attachment(msg: &ChannelMessage) -> bool {
    let candidates = [
        msg.attachment_filename.as_deref(),
        msg.attachment_path.as_deref(),
        msg.attachment_url.as_deref(),
        msg.attachment.as_deref(),
    ];
    candidates.into_iter().flatten().any(has_zip_suffix)
}

fn has_zip_suffix(value: &str) -> bool {
    value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
        .ends_with(".zip")
}

fn parse_trusted_pack_sources(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn source_is_trusted_for_auto_install(msg: &ChannelMessage, trusted: &[String]) -> bool {
    if trusted.is_empty() {
        return false;
    }
    let channel = msg.channel.to_ascii_lowercase();
    let reply_target = msg.reply_target.to_ascii_lowercase();
    let sender = msg.sender.to_ascii_lowercase();
    trusted.iter().any(|rule| {
        if rule == "*" {
            return true;
        }
        let rule = rule.to_ascii_lowercase();
        if rule == channel || rule == sender {
            return true;
        }
        let combined_channel_room = format!("{channel}:{reply_target}");
        if rule == combined_channel_room {
            return true;
        }
        let combined_full = format!("{channel}:{reply_target}:{sender}");
        rule == combined_full
    })
}

async fn handle_pack_attachment_if_present(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> Option<String> {
    let Some(path) = msg.attachment_path.as_deref() else {
        return Some(
            "Detected a .zip attachment. Pack detection requires a local attachment path; this upload did not provide one."
                .to_string(),
        );
    };
    let client = reqwest::Client::new();
    let detect_resp = add_auth(client.post(format!("{base_url}/packs/detect")), api_token)
        .json(&serde_json::json!({
            "path": path,
            "attachment_id": msg.id,
            "connector": msg.channel,
            "channel_id": msg.reply_target,
            "sender_id": msg.sender,
        }))
        .send()
        .await;
    let detect_resp = match detect_resp {
        Ok(resp) => resp,
        Err(err) => {
            warn!("pack detect request failed: {}", err);
            return Some(format!("Pack detection failed: {err}"));
        }
    };
    let status = detect_resp.status();
    let payload: serde_json::Value = detect_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Pack detection failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    let is_pack = payload
        .get("is_pack")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_pack {
        return None;
    }

    let trusted_raw = std::env::var("TANDEM_PACK_AUTO_INSTALL_TRUSTED_SOURCES").unwrap_or_default();
    let trusted = parse_trusted_pack_sources(&trusted_raw);
    let auto_install = source_is_trusted_for_auto_install(msg, &trusted);
    if !auto_install {
        return Some(format!(
            "Tandem Pack detected in attachment `{}`. Auto-install is disabled for this source.\n\nInstall manually from UI or call `/packs/install_from_attachment` with `attachment_id={}` and `path={}`.",
            msg.attachment_filename
                .as_deref()
                .or(msg.attachment.as_deref())
                .unwrap_or("upload.zip"),
            msg.id,
            path
        ));
    }

    let install_resp = add_auth(
        client.post(format!("{base_url}/packs/install_from_attachment")),
        api_token,
    )
    .json(&serde_json::json!({
        "attachment_id": msg.id,
        "path": path,
        "connector": msg.channel,
        "channel_id": msg.reply_target,
        "sender_id": msg.sender,
    }))
    .send()
    .await;
    let install_resp = match install_resp {
        Ok(resp) => resp,
        Err(err) => {
            warn!("pack install_from_attachment request failed: {}", err);
            return Some(format!("Tandem Pack detected but install failed: {err}"));
        }
    };
    let status = install_resp.status();
    let payload: serde_json::Value = install_resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Some(format!(
            "Tandem Pack detected but install failed ({status}): {}",
            payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        ));
    }
    let pack_name = payload
        .get("installed")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let pack_version = payload
        .get("installed")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Some(format!(
        "Tandem Pack detected and installed: `{pack_name}` `{pack_version}`."
    ))
}

fn synthesize_attachment_prompt(
    channel: &str,
    attachment: &str,
    user_text: &str,
    resource_key: Option<&str>,
    attachment_path: Option<&str>,
    attachment_url: Option<&str>,
    attachment_filename: Option<&str>,
    attachment_mime: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "Channel upload received from `{channel}`: `{attachment}`."
    )];
    if let Some(name) = attachment_filename {
        lines.push(format!("Attachment filename: `{name}`."));
    }
    if let Some(mime) = attachment_mime {
        lines.push(format!("Attachment MIME type: `{mime}`."));
    }
    if let Some(path) = attachment_path {
        lines.push(format!("Stored local attachment path: `{path}`."));
        lines.push(
            "Use the `read` tool on the local path when the file is text-like or parseable."
                .to_string(),
        );
    }
    if let Some(url) = attachment_url {
        lines.push(format!("Attachment source URL: `{url}`."));
    }
    if let Some(key) = resource_key {
        lines.push(format!("Stored upload reference: `{key}`."));
    }
    if !user_text.trim().is_empty() {
        lines.push(format!("User caption/message: {}", user_text.trim()));
    }
    lines.push(
        "Analyze the attachment directly when your model and tools support this MIME type."
            .to_string(),
    );
    lines.push(
        "If this file type is unsupported, explain what format/model capability is required."
            .to_string(),
    );
    lines.join("\n")
}

fn sanitize_resource_segment(raw: &str) -> String {
    let sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

async fn persist_channel_attachment_reference(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    msg: &ChannelMessage,
    attachment: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let resource_key = format!(
        "run/{}/channel_uploads/{}",
        sanitize_resource_segment(session_id),
        sanitize_resource_segment(&msg.id)
    );
    let stored_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let resource_value = serde_json::json!({
        "session_id": session_id,
        "channel": msg.channel,
        "sender": msg.sender,
        "reply_target": msg.reply_target,
        "message_id": msg.id,
        "attachment": attachment,
        "attachment_url": msg.attachment_url,
        "attachment_path": msg.attachment_path,
        "attachment_mime": msg.attachment_mime,
        "attachment_filename": msg.attachment_filename,
        "user_text": msg.content,
        "received_at": msg.timestamp.to_rfc3339(),
        "stored_at_ms": stored_at_ms
    });

    let resource_resp = add_auth(
        client.put(format!("{base_url}/resource/{resource_key}")),
        api_token,
    )
    .json(&serde_json::json!({
        "value": resource_value,
        "updated_by": format!("channels:{}", msg.channel)
    }))
    .send()
    .await;

    match resource_resp {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "failed to persist upload resource '{}' ({}): {}",
                resource_key, status, body
            );
            return None;
        }
        Err(e) => {
            warn!(
                "failed to persist upload resource '{}': {}",
                resource_key, e
            );
            return None;
        }
    }

    let memory_content = format!(
        "Channel upload recorded: channel={}, attachment={}, session={}, sender={}, resource_key={}, file_path={}, file_url={}",
        msg.channel,
        attachment,
        session_id,
        msg.sender,
        resource_key,
        msg.attachment_path.as_deref().unwrap_or("n/a"),
        msg.attachment_url.as_deref().unwrap_or("n/a")
    );
    let memory_resp = add_auth(client.post(format!("{base_url}/memory/put")), api_token)
        .json(&serde_json::json!({
            "run_id": format!("channel-upload-{}", session_id),
            "partition": {
                "org_id": "local",
                "workspace_id": "channels",
                "project_id": session_id,
                "tier": "session"
            },
            "kind": "note",
            "content": memory_content,
            "artifact_refs": [format!("resource:{}", resource_key)],
            "classification": "internal",
            "metadata": {
                "channel": msg.channel,
                "sender": msg.sender,
                "message_id": msg.id
            }
        }))
        .send()
        .await;

    match memory_resp {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                "upload resource saved but memory.put failed for '{}' ({}): {}",
                resource_key, status, body
            );
        }
        Err(e) => {
            warn!(
                "upload resource saved but memory.put request failed for '{}': {}",
                resource_key, e
            );
        }
    }

    Some(resource_key)
}

fn extract_image_urls_and_clean_text(input: &str) -> (String, Vec<String>) {
    let (without_markdown_images, markdown_urls) = strip_markdown_image_links(input);
    let mut urls = markdown_urls;
    for token in without_markdown_images.split_whitespace() {
        let candidate = trim_wrapping_punctuation(token);
        if is_image_url(candidate) && !urls.iter().any(|u| u == candidate) {
            urls.push(candidate.to_string());
        }
    }

    let cleaned = without_markdown_images
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    (cleaned, urls)
}

fn strip_markdown_image_links(input: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut urls = Vec::new();
    let mut i = 0usize;

    while i < input.len() {
        let Some(rel) = input[i..].find("![") else {
            out.push_str(&input[i..]);
            break;
        };
        let start = i + rel;
        out.push_str(&input[i..start]);

        let Some(alt_end_rel) = input[start + 2..].find("](") else {
            out.push_str("![");
            i = start + 2;
            continue;
        };
        let alt_end = start + 2 + alt_end_rel;

        let Some(url_end_rel) = input[alt_end + 2..].find(')') else {
            out.push_str("![");
            i = start + 2;
            continue;
        };
        let url_end = alt_end + 2 + url_end_rel;
        let url = input[alt_end + 2..url_end].trim();

        if is_image_url(url) && !urls.iter().any(|u| u == url) {
            urls.push(url.to_string());
        } else {
            out.push_str(&input[start..=url_end]);
        }

        i = url_end + 1;
    }

    (out, urls)
}

fn trim_wrapping_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    })
}

fn is_image_url(url: &str) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let lower = base.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn channel_security_profile(
    channel: &str,
    security_profiles: &ChannelSecurityMap,
) -> ChannelSecurityProfile {
    security_profiles
        .get(channel)
        .copied()
        .unwrap_or(ChannelSecurityProfile::Operator)
}

// ---------------------------------------------------------------------------
// Session management helpers
// ---------------------------------------------------------------------------

fn build_channel_session_permissions(
    security_profile: ChannelSecurityProfile,
) -> Vec<serde_json::Value> {
    match security_profile {
        ChannelSecurityProfile::PublicDemo => vec![
            serde_json::json!({ "permission": "memory_search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_store", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "websearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch_html", "pattern": "*", "action": "allow" }),
        ],
        _ => vec![
            serde_json::json!({ "permission": "ls", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "glob", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "grep", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "codesearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "read", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_search", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_store", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "memory_list", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "websearch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "webfetch_html", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_status", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_open", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_navigate", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_snapshot", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_click", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_type", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_press", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_wait", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_extract", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_screenshot", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "browser_close", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "mcp*", "pattern": "*", "action": "allow" }),
            serde_json::json!({ "permission": "bash", "pattern": "*", "action": "allow" }),
        ],
    }
}

fn build_channel_session_create_body(
    title: &str,
    security_profile: ChannelSecurityProfile,
    project_id: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "title": title,
        "permission": build_channel_session_permissions(security_profile),
    });
    if let Some(project_id) = project_id {
        payload["project_id"] = serde_json::json!(project_id);
    }
    if security_profile != ChannelSecurityProfile::PublicDemo {
        payload["directory"] = serde_json::json!(".");
    }
    payload
}

/// Look up an existing session or create a new one via `POST /session`.
async fn get_or_create_session(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> Option<String> {
    let map_key = session_map_key(msg);
    let legacy_key = legacy_session_map_key(msg);
    {
        let mut guard = session_map.lock().await;
        if let Some(record) = guard.get_mut(&map_key) {
            record.last_seen_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let sid = record.session_id.clone();
            // Persist the updated last_seen_at_ms
            persist_session_map(&guard).await;
            return Some(sid);
        }
        if let Some(mut legacy_record) = guard.remove(&legacy_key) {
            legacy_record.last_seen_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            legacy_record.scope_id = Some(msg.scope.id.clone());
            legacy_record.scope_kind = Some(session_scope_kind_label(msg).to_string());
            let sid = legacy_record.session_id.clone();
            guard.insert(map_key.clone(), legacy_record);
            persist_session_map(&guard).await;
            return Some(sid);
        }
    }

    let client = reqwest::Client::new();
    let title = session_title_prefix(msg);
    let public_memory_project_id = if security_profile == ChannelSecurityProfile::PublicDemo {
        Some(public_channel_memory_scope_key(msg))
    } else {
        None
    };
    let body = build_channel_session_create_body(
        &title,
        security_profile,
        public_memory_project_id.as_deref(),
    );

    let resp = add_auth(client.post(format!("{base_url}/session")), api_token)
        .json(&body)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            error!("failed to create session: {e}");
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            error!("session create response parse error: {e}");
            return None;
        }
    };

    let session_id = json
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;

    let mut guard = session_map.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    guard.insert(
        map_key,
        SessionRecord {
            session_id: session_id.clone(),
            created_at_ms: now,
            last_seen_at_ms: now,
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            scope_id: Some(msg.scope.id.clone()),
            scope_kind: Some(session_scope_kind_label(msg).to_string()),
            tool_preferences: None,
        },
    );
    persist_session_map(&guard).await;

    Some(session_id)
}

/// Submit a message to a Tandem session using `prompt_async` and stream
/// the result via the SSE event bus (`GET /event?sessionID=...&runID=...`).
///
/// Falls back to an error string if the initial fire fails or the stream
/// never completes within `timeout_secs`.
async fn run_in_session(
    session_id: &str,
    content: &str,
    base_url: &str,
    api_token: &str,
    attachment_path: Option<&str>,
    attachment_url: Option<&str>,
    attachment_mime: Option<&str>,
    attachment_filename: Option<&str>,
    agent: Option<&str>,
    tool_allowlist: Option<&Vec<String>>,
) -> anyhow::Result<String> {
    let timeout_secs: u64 = std::env::var("TANDEM_CHANNEL_MAX_WAIT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs + 30))
        .build()?;

    let mut parts = Vec::new();
    let attachment_source = attachment_path.or(attachment_url);
    if let (Some(source), Some(mime)) = (attachment_source, attachment_mime) {
        parts.push(serde_json::json!({
            "type": "file",
            "mime": mime,
            "filename": attachment_filename,
            "url": source
        }));
    }
    parts.push(serde_json::json!({ "type": "text", "text": content }));
    let mut body = serde_json::json!({ "parts": parts });
    if let Some(agent) = agent {
        body["agent"] = serde_json::json!(agent);
    }
    if let Some(allowlist) = tool_allowlist {
        body["tool_allowlist"] = serde_json::json!(allowlist);
    }
    if let Ok(Some(model)) = fetch_default_model_spec(&client, base_url, api_token).await {
        body["model"] = model;
    }

    // Request run metadata so we can bind SSE to this specific run.
    let submit_prompt = || {
        add_auth(
            client.post(format!(
                "{base_url}/session/{session_id}/prompt_async?return=run"
            )),
            api_token,
        )
        .json(&body)
    };
    let mut resp = submit_prompt().send().await?;
    if resp.status() == reqwest::StatusCode::CONFLICT {
        let conflict_text = resp.text().await.unwrap_or_default();
        let conflict_json: serde_json::Value =
            serde_json::from_str(&conflict_text).unwrap_or_default();
        let active_run_id = conflict_json
            .get("activeRun")
            .and_then(|v| v.get("runID").or_else(|| v.get("run_id")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let retry_after_ms = conflict_json
            .get("retryAfterMs")
            .and_then(|v| v.as_u64())
            .unwrap_or(500)
            .clamp(100, 5_000);
        if active_run_id.is_empty() {
            anyhow::bail!("prompt_async failed (409 Conflict): {conflict_text}");
        }
        let cancel_url = format!("{base_url}/session/{session_id}/run/{active_run_id}/cancel");
        let _ = add_auth(client.post(cancel_url), api_token)
            .json(&serde_json::json!({}))
            .send()
            .await;
        tokio::time::sleep(Duration::from_millis(retry_after_ms)).await;
        resp = submit_prompt().send().await?;
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("prompt_async failed ({status}): {err}");
    }

    // Newer engines may return 204/empty when no run payload is emitted.
    // Treat empty as "no run id" rather than surfacing a decode failure.
    let fire_text = resp.text().await?;
    let fire_json: serde_json::Value = if fire_text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&fire_text).map_err(|e| {
            anyhow::anyhow!("prompt_async run payload parse failed: {e}: {fire_text}")
        })?
    };
    let _run_id = fire_json
        .get("runID")
        .or_else(|| fire_json.get("run_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Stream the SSE event bus until the run finishes or we timeout.
    // Run-filtered streams can miss events when engines emit session-scoped updates.
    // Subscribe by session for robust delivery in channels.
    let event_url = format!("{base_url}/event?sessionID={session_id}");

    use futures_util::StreamExt;
    let mut content_buf = String::new();
    let mut last_error: Option<String> = None;
    let mut line_buf = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut reconnect_attempts = 0usize;
    let mut body_stream = open_channel_event_stream(&client, &event_url, api_token)
        .await?
        .bytes_stream();

    'outer: loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(60), body_stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                line_buf.push_str(&String::from_utf8_lossy(&chunk));
            }
            Ok(Some(Err(e))) => {
                let err_text = e.to_string();
                let recoverable =
                    should_retry_channel_event_stream(&err_text, &content_buf, deadline)
                        && reconnect_attempts < 2;
                if err_text.contains("error decoding response body") {
                    tracing::warn!(
                        "Channel SSE stream closed while reading response body: {err_text}"
                    );
                } else {
                    tracing::warn!("Channel SSE stream error: {err_text}");
                }
                if recoverable {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                } else if !err_text.trim().is_empty() {
                    last_error = Some(err_text);
                }
                break 'outer;
            }
            Ok(None) => {
                if should_retry_channel_event_stream("eof", &content_buf, deadline)
                    && reconnect_attempts < 2
                {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                }
                break 'outer;
            }
            Err(_) => {
                if should_retry_channel_event_stream("timeout", &content_buf, deadline)
                    && reconnect_attempts < 2
                {
                    reconnect_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(250 * reconnect_attempts as u64))
                        .await;
                    match open_channel_event_stream(&client, &event_url, api_token).await {
                        Ok(resp) => {
                            body_stream = resp.bytes_stream();
                            continue 'outer;
                        }
                        Err(err) => {
                            last_error = Some(err.to_string());
                        }
                    }
                } else {
                    last_error = Some(
                        "channel event stream timed out while waiting for updates".to_string(),
                    );
                }
                break 'outer;
            }
        }

        // Process complete SSE lines
        while let Some(pos) = line_buf.find('\n') {
            let raw = line_buf[..pos].trim_end_matches('\r').to_string();
            line_buf = line_buf[pos + 1..].to_string();

            let data = raw.strip_prefix("data:").map(str::trim);
            let Some(data) = data else { continue };
            if data == "[DONE]" {
                break 'outer;
            }

            let Ok(evt) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            let event_type = evt
                .get("type")
                .or_else(|| evt.get("event"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if event_type == "message.part.updated" {
                if let Some(props) = evt.get("properties") {
                    let is_text = props
                        .get("part")
                        .and_then(|p| p.get("type"))
                        .and_then(|v| v.as_str())
                        .map(|v| v == "text")
                        .unwrap_or(false);
                    if is_text {
                        if let Some(delta) = props.get("delta").and_then(|v| v.as_str()) {
                            content_buf.push_str(delta);
                        }
                    }
                }
                continue;
            }

            if event_type == "session.error" {
                if let Some(message) = extract_event_error_message(&evt) {
                    last_error = Some(message);
                }
                continue;
            }

            match event_type {
                "session.message.delta" | "content" => {
                    if let Some(delta) = evt
                        .get("delta")
                        .and_then(|v| v.as_str())
                        .or_else(|| evt.get("text").and_then(|v| v.as_str()))
                    {
                        content_buf.push_str(delta);
                    }
                }
                "session.run.finished"
                | "session.run.completed"
                | "session.run.failed"
                | "session.run.cancelled"
                | "session.run.canceled"
                | "done" => {
                    if let Some(err) = extract_event_error_message(&evt) {
                        last_error = Some(err);
                    }
                    break 'outer;
                }
                _ => {}
            }
        }
    }

    if content_buf.is_empty() {
        // Fast runs may complete before we attach SSE, and persisted assistant
        // messages can lag slightly behind run completion. Retry briefly.
        for _ in 0..20 {
            if let Ok(Some(fallback)) =
                fetch_latest_assistant_message(&client, base_url, api_token, session_id).await
            {
                return Ok(fallback);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if let Some(error_message) = last_error {
            return Ok(format!(
                "⚠️ Error: {}",
                truncate_for_channel(&error_message, 320)
            ));
        }
        return Ok("(no response)".to_string());
    }

    Ok(content_buf)
}

async fn open_channel_event_stream(
    client: &reqwest::Client,
    event_url: &str,
    api_token: &str,
) -> anyhow::Result<reqwest::Response> {
    let resp = add_auth(client.get(event_url), api_token)
        .header("Accept", "text/event-stream")
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("event stream request failed ({status}): {err}");
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
    {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "event stream returned unexpected content-type '{}' ({status}): {}",
            content_type,
            truncate_for_channel(&body, 400)
        );
    }
    Ok(resp)
}

fn should_retry_channel_event_stream(
    reason: &str,
    content_buf: &str,
    deadline: tokio::time::Instant,
) -> bool {
    let before_deadline = tokio::time::Instant::now() < deadline;
    let empty_content = content_buf.trim().is_empty();
    empty_content
        && before_deadline
        && (matches!(reason, "eof" | "timeout") || reason.contains("error decoding response body"))
}

fn truncate_for_channel(input: &str, max_chars: usize) -> String {
    let mut out = input.trim().chars().take(max_chars).collect::<String>();
    if input.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn extract_event_error_message(evt: &serde_json::Value) -> Option<String> {
    let paths = [
        evt.get("error").and_then(|e| e.get("message")),
        evt.get("error"),
        evt.get("message"),
        evt.get("properties")
            .and_then(|p| p.get("error"))
            .and_then(|e| e.get("message")),
        evt.get("properties").and_then(|p| p.get("error")),
        evt.get("properties").and_then(|p| p.get("message")),
    ];

    for value in paths.into_iter().flatten() {
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
            continue;
        }
        if let Some(obj) = value.as_object() {
            if let Some(text) = obj.get("message").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

async fn fetch_default_model_spec(
    client: &reqwest::Client,
    base_url: &str,
    api_token: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let url = format!("{base_url}/config/providers");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }

    let cfg: serde_json::Value = resp.json().await?;
    let default_provider = cfg
        .get("default")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if default_provider.is_empty() {
        return Ok(None);
    }

    let default_model = cfg
        .get("providers")
        .and_then(|v| v.get(default_provider))
        .and_then(|v| v.get("default_model").or_else(|| v.get("defaultModel")))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if default_model.is_empty() {
        return Ok(None);
    }

    Ok(Some(serde_json::json!({
        "provider_id": default_provider,
        "model_id": default_model
    })))
}

/// Fallback for channel delivery: if the SSE stream did not emit text deltas,
/// fetch persisted session history and return the latest assistant text.
async fn fetch_latest_assistant_message(
    client: &reqwest::Client,
    base_url: &str,
    api_token: &str,
    session_id: &str,
) -> anyhow::Result<Option<String>> {
    let url = format!("{base_url}/session/{session_id}/message");
    let resp = add_auth(client.get(&url), api_token).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("session message fallback failed ({status}): {err}");
    }

    let messages: serde_json::Value = resp.json().await?;
    let Some(items) = messages.as_array() else {
        return Ok(None);
    };

    for msg in items.iter().rev() {
        let role = msg
            .get("info")
            .and_then(|info| info.get("role"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if role != "assistant" {
            continue;
        }

        let Some(parts) = msg.get("parts").and_then(|v| v.as_array()) else {
            continue;
        };

        let mut text = String::new();
        for part in parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type == "text" || part_type == "reasoning" || part_type.is_empty() {
                if let Some(chunk) = part.get("text").and_then(|v| v.as_str()) {
                    if !chunk.trim().is_empty() {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(chunk);
                    }
                }
            }
        }

        if !text.trim().is_empty() {
            return Ok(Some(text));
        }
    }

    Ok(None)
}

/// Send an approve or deny decision to the tandem-server tool approval endpoint.
/// Path: POST /sessions/{session_id}/tools/{tool_call_id}/approve|deny
async fn relay_tool_decision(
    base_url: &str,
    api_token: &str,
    session_id: &str,
    tool_call_id: &str,
    approved: bool,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let action = if approved { "approve" } else { "deny" };
    let url = format!("{base_url}/sessions/{session_id}/tools/{tool_call_id}/{action}");
    let resp = add_auth(client.post(&url), api_token).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("relay_tool_decision failed ({status})");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Slash command handler dispatch
// ---------------------------------------------------------------------------

async fn handle_slash_command(
    cmd: SlashCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if let Some(reason) = blocked_command_reason(&cmd, security_profile) {
        return format!(
            "🔒 This command is disabled in this channel for security.\n{}\nUse `/help` to see which Tandem capabilities are available here versus disabled for this public integration.",
            reason
        );
    }
    match cmd {
        SlashCommand::Help { topic } => help_text(topic.as_deref(), security_profile),
        SlashCommand::ListSessions => list_sessions_text(msg, base_url, api_token).await,
        SlashCommand::New { name } => {
            new_session_text(
                name,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        SlashCommand::Resume { query } => {
            resume_session_text(query, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Status => status_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Run => run_status_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Cancel => cancel_run_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Todos => todos_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Requests => requests_text(msg, base_url, api_token, session_map).await,
        SlashCommand::Answer {
            question_id,
            answer,
        } => answer_question_text(question_id, answer, msg, base_url, api_token, session_map).await,
        SlashCommand::Providers => providers_text(base_url, api_token).await,
        SlashCommand::Models { provider } => models_text(provider, base_url, api_token).await,
        SlashCommand::Model { model_id } => set_model_text(model_id, base_url, api_token).await,
        SlashCommand::Rename { name } => {
            rename_session_text(name, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Approve { tool_call_id } => {
            let session_id = active_session_id(msg, session_map).await;
            match session_id {
                None => "⚠️ No active session — nothing to approve.".to_string(),
                Some(sid) => {
                    match relay_tool_decision(base_url, api_token, &sid, &tool_call_id, true).await
                    {
                        Ok(()) => format!("✅ Approved tool call `{tool_call_id}`."),
                        Err(e) => format!("⚠️ Could not approve: {e}"),
                    }
                }
            }
        }
        SlashCommand::Deny { tool_call_id } => {
            let session_id = active_session_id(msg, session_map).await;
            match session_id {
                None => "⚠️ No active session — nothing to deny.".to_string(),
                Some(sid) => {
                    match relay_tool_decision(base_url, api_token, &sid, &tool_call_id, false).await
                    {
                        Ok(()) => format!("🚫 Denied tool call `{tool_call_id}`."),
                        Err(e) => format!("⚠️ Could not deny: {e}"),
                    }
                }
            }
        }
        SlashCommand::Schedule { action } => {
            schedule_command_text(action, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Automations { action } => {
            automations_command_text(action, base_url, api_token).await
        }
        SlashCommand::Runs { action } => runs_command_text(action, base_url, api_token).await,
        SlashCommand::Memory { action } => {
            memory_command_text(
                action,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        SlashCommand::Workspace { action } => {
            workspace_command_text(action, msg, base_url, api_token, session_map).await
        }
        SlashCommand::Tools { action } => {
            tools_command_text(action, msg, base_url, api_token).await
        }
        SlashCommand::Mcp { action } => mcp_command_text(action, msg, base_url, api_token).await,
        SlashCommand::Packs { action } => packs_command_text(action, base_url, api_token).await,
        SlashCommand::Config { action } => config_command_text(action, base_url, api_token).await,
    }
}

fn blocked_command_reason(
    cmd: &SlashCommand,
    security_profile: ChannelSecurityProfile,
) -> Option<&'static str> {
    if security_profile != ChannelSecurityProfile::PublicDemo {
        return None;
    }
    match cmd {
        SlashCommand::Providers
        | SlashCommand::Models { .. }
        | SlashCommand::Model { .. }
        | SlashCommand::Schedule { .. }
        | SlashCommand::Automations { .. }
        | SlashCommand::Runs { .. }
        | SlashCommand::Workspace { .. }
        | SlashCommand::Mcp { .. }
        | SlashCommand::Packs { .. }
        | SlashCommand::Config { .. }
        | SlashCommand::Tools { .. }
        | SlashCommand::Approve { .. }
        | SlashCommand::Deny { .. }
        | SlashCommand::Answer { .. } => Some(
            "Public demo channels do not expose operator controls, workspace access, MCP access, or runtime reconfiguration.",
        ),
        SlashCommand::Todos | SlashCommand::Requests => Some(
            "Public demo channels keep internal execution and approval queues hidden to avoid leaking runtime details.",
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Individual slash command implementations
// ---------------------------------------------------------------------------

fn help_text(topic: Option<&str>, security_profile: ChannelSecurityProfile) -> String {
    match topic.map(|value| value.trim().to_ascii_lowercase()) {
        Some(topic) if topic == "schedule" || topic == "workflow" || topic == "automation" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "schedule",
                    "Workflow planning and automation setup are disabled in this public channel for security.",
                )
            } else {
                schedule_help_text()
            }
        }
        Some(topic) if topic == "automations" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "automations",
                    "Automation control commands are disabled in this public channel for security.",
                )
            } else {
                automations_help_text()
            }
        }
        Some(topic) if topic == "runs" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "runs",
                    "Run control commands are disabled in this public channel for security.",
                )
            } else {
                runs_help_text()
            }
        }
        Some(topic) if topic == "memory" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_memory_help_text()
            } else {
                memory_help_text()
            }
        }
        Some(topic) if topic == "workspace" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "workspace",
                    "Workspace and file access commands are disabled in this public channel for security.",
                )
            } else {
                workspace_help_text()
            }
        }
        Some(topic) if topic == "tools" => tools_help_text(security_profile),
        Some(topic) if topic == "mcp" => mcp_help_text(security_profile),
        Some(topic) if topic == "packs" => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                disabled_help_text(
                    "packs",
                    "Pack install and inspection commands are disabled in this public channel for security.",
                )
            } else {
                packs_help_text()
            }
        }
        Some(topic) if topic == "config" => config_help_text(security_profile),
        Some(topic) => format!(
            "⚠️ Unknown help topic `{topic}`.\nUse `/help` to list command groups or `/help automations`, `/help memory`, `/help workspace`, `/help mcp`, `/help packs`, `/help config`, or `/help schedule`."
        ),
        None => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_help_text()
            } else {
                "🤖 *Tandem Commands*\n\
Core session commands:\n\
/new [name] — start a fresh session\n\
/sessions — list your recent sessions\n\
/resume <id or name> — switch to a previous session\n\
/rename <name> — rename the current session\n\
/status — show current session info\n\
/run — show active run state\n\
/cancel — cancel the active run\n\
\n\
Session ops:\n\
/todos — list current session todos\n\
/requests — list pending tool/question requests\n\
/answer <question_id> <text> — answer a pending question\n\
/approve <tool_call_id> — approve a pending tool call\n\
/deny <tool_call_id> — deny a pending tool call\n\
\n\
Model controls:\n\
/providers — list available providers\n\
/models [provider] — list models by provider\n\
/model <model_id> — set model for current default provider\n\
\n\
Workflow planning:\n\
/schedule help — show workflow-plan commands for scheduling and automation setup\n\
\n\
Operator commands:\n\
/automations — list saved automations\n\
/runs — list recent automation runs\n\
/memory — list recent memory entries\n\
/workspace — show current workspace binding\n\
/mcp — list MCP servers\n\
/packs — list installed packs\n\
/config — show runtime config summary\n\
\n\
Help:\n\
/help — show this message\n\
/help automations — automation command guide\n\
/help runs — automation run command guide\n\
/help memory — memory command guide\n\
/help workspace — workspace command guide\n\
/help mcp — MCP command guide\n\
/help packs — pack command guide\n\
/help config — config command guide\n\
/help schedule — explain workflow-planning commands"
                    .to_string()
            }
        }
    }
}

fn disabled_help_text(topic: &str, reason: &str) -> String {
    format!(
        "🔒 *{topic} commands are disabled in this channel*\n{reason}\n\nThis Tandem integration supports those capabilities in trusted/operator channels, but they are intentionally blocked here."
    )
}

fn public_demo_help_text() -> String {
    "🤖 *Tandem Public Demo Commands*\n\
Available here:\n\
/new [name] — start a fresh session\n\
/sessions — list your recent sessions\n\
/resume <id or name> — switch to a previous session\n\
/rename <name> — rename the current session\n\
/status — show current session info\n\
/run — show active run state\n\
/cancel — cancel the active run\n\
/memory — search and store channel-scoped public memory\n\
/help — show this message\n\
\n\
Disabled in this public channel for security:\n\
/providers, /models, /model — runtime and model reconfiguration\n\
/workspace — file and repo access\n\
/mcp — external connector access\n\
/tools — tool-scope override controls\n\
/config — runtime configuration access\n\
/schedule, /automations, /runs — operator workflow control\n\
/packs — pack install and inspection controls\n\
\n\
These are real Tandem capabilities, but this integration is intentionally hardened so you can explore it safely in public."
        .to_string()
}

fn schedule_help_text() -> String {
    "🗓️ *Workflow Planning Commands*\n\
/schedule help — show this guide\n\
/schedule plan <prompt> — create a workflow draft from a plain-English goal\n\
/schedule show <plan_id> — inspect the current draft\n\
/schedule edit <plan_id> <message> — revise the draft conversationally\n\
/schedule reset <plan_id> — reset the draft back to its initial preview\n\
/schedule apply <plan_id> — turn the draft into a saved automation\n\
\n\
Examples:\n\
/schedule plan Every weekday at 9am summarize GitHub notifications and email me the blockers\n\
/schedule edit wfplan-123 change this to every Monday and Friday at 8am\n\
/schedule apply wfplan-123\n\
\n\
Tip: `/schedule plan` uses the current session workspace when available so the planner can target the right repo."
        .to_string()
}

fn automations_help_text() -> String {
    "⚙️ *Automation Commands*\n\
/automations — list saved automations\n\
/automations show <id> — inspect one automation\n\
/automations runs <id> [limit] — list recent runs for one automation\n\
/automations run <id> — trigger an automation now\n\
/automations pause <id> — pause an automation\n\
/automations resume <id> — resume a paused automation\n\
/automations delete <id> --yes — delete an automation"
        .to_string()
}

fn runs_help_text() -> String {
    "🏃 *Run Commands*\n\
/runs — list recent automation runs\n\
/runs show <run_id> — inspect a run\n\
/runs pause <run_id> — pause a run\n\
/runs resume <run_id> — resume a paused run\n\
/runs cancel <run_id> — cancel a run\n\
/runs artifacts <run_id> — list run artifacts"
        .to_string()
}

fn memory_help_text() -> String {
    "🧠 *Memory Commands*\n\
/memory — list recent memory entries\n\
/memory search <query> — search across available memory\n\
/memory recent [limit] — list recent entries\n\
/memory save <text> — store a global note\n\
/memory scopes — show the current session/project/global scope binding\n\
/memory delete <id> --yes — delete a memory entry"
        .to_string()
}

fn public_demo_memory_help_text() -> String {
    "🧠 *Public Channel Memory Commands*\n\
/memory — list recent memory entries for this channel scope\n\
/memory search <query> — search channel-scoped public memory\n\
/memory recent [limit] — list recent channel-scoped entries\n\
/memory save <text> — store a note in this channel's public memory namespace\n\
/memory scopes — show the quarantined public memory scope for this channel\n\
/memory delete <id> --yes — delete a memory entry from this channel scope\n\
\n\
This memory is quarantined to the current public channel scope and does not read from Tandem's normal trusted project/global memory."
        .to_string()
}

fn workspace_help_text() -> String {
    "📁 *Workspace Commands*\n\
/workspace — show the current workspace binding\n\
/workspace status — show session/project/workspace status\n\
/workspace files <query> — find files by name in the workspace\n\
/workspace branch — show the current git branch"
        .to_string()
}

fn tools_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "tools",
            "Tool-scope override commands are disabled because this channel uses an enforced public security profile.",
        );
    }
    "🛠 *Tool Scope Commands*\n\
/tools — show this help\n\
/tools list — list available tools and their current state\n\
/tools enable <tool1,tool2> — enable tools for this channel\n\
/tools disable <tool1,tool2> — disable tools for this channel\n\
/tools reset — reset to default tool scope\n\
\n\
Available built-in tools: read, glob, ls, list, grep, codesearch, websearch,\nwebfetch, webfetch_html, bash, write, edit, apply_patch, todowrite, memory_search,\nmemory_store, memory_list, skill, task, question\n\n\
Use `/mcp` commands to manage MCP server access."
        .to_string()
}

fn mcp_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "mcp",
            "MCP connector commands are disabled in this public channel to avoid exposing external integrations.",
        );
    }
    "🔌 *MCP Commands*\n\
/mcp — list MCP servers\n\
/mcp tools [server] — list discovered tools\n\
/mcp resources — list discovered resources\n\
/mcp status — summarize connected servers\n\
/mcp connect <name> — connect a server\n\
/mcp disconnect <name> — disconnect a server\n\
/mcp refresh <name> — refresh a server\n\
/mcp enable <name> — enable an MCP server for this channel\n\
/mcp disable <name> — disable an MCP server for this channel"
        .to_string()
}

fn packs_help_text() -> String {
    "📦 *Pack Commands*\n\
/packs — list installed packs\n\
/packs show <selector> — inspect a pack\n\
/packs updates <selector> — check for updates\n\
/packs install <path-or-url> — install a pack\n\
/packs uninstall <selector> --yes — uninstall a pack"
        .to_string()
}

fn config_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "config",
            "Runtime configuration and model-management commands are disabled in this public channel for security.",
        );
    }
    "🛠️ *Config Commands*\n\
/config — show a runtime config summary\n\
/config providers — show provider summary\n\
/config channels — show channel status/config summary\n\
/config model — show the active default model\n\
/config set-model <model_id> — update the default model"
        .to_string()
}

async fn schedule_command_text(
    action: ScheduleCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    match action {
        ScheduleCommand::Help => schedule_help_text(),
        ScheduleCommand::Plan { prompt } => {
            schedule_plan_text(prompt, msg, base_url, api_token, session_map).await
        }
        ScheduleCommand::Show { plan_id } => schedule_show_text(plan_id, base_url, api_token).await,
        ScheduleCommand::Edit { plan_id, message } => {
            schedule_edit_text(plan_id, message, base_url, api_token).await
        }
        ScheduleCommand::Reset { plan_id } => {
            schedule_reset_text(plan_id, base_url, api_token).await
        }
        ScheduleCommand::Apply { plan_id } => {
            schedule_apply_text(plan_id, base_url, api_token).await
        }
    }
}

async fn workflow_planner_workspace_root(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> Option<String> {
    let sid = active_session_id(msg, session_map).await?;
    let client = reqwest::Client::new();
    let resp = add_auth(client.get(format!("{base_url}/session/{sid}")), api_token)
        .send()
        .await
        .ok()?;
    let json = resp.json::<serde_json::Value>().await.ok()?;
    json.get("workspace_root")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            json.get("directory")
                .and_then(|value| value.as_str())
                .filter(|value| value.starts_with('/'))
                .map(ToOwned::to_owned)
        })
}

fn workflow_plan_summary(plan: &serde_json::Value) -> String {
    let plan_id = plan
        .get("plan_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let title = plan
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("Untitled workflow");
    let workspace_root = plan
        .get("workspace_root")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let confidence = plan
        .get("confidence")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let step_count = plan
        .get("steps")
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let schedule = plan
        .get("schedule")
        .map(compact_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "Plan `{}`\nTitle: {}\nSteps: {}\nConfidence: {}\nWorkspace: {}\nSchedule: {}",
        plan_id, title, step_count, confidence, workspace_root, schedule
    )
}

fn workflow_plan_change_summary(value: &serde_json::Value) -> Option<String> {
    let items = value
        .get("change_summary")
        .and_then(|entry| entry.as_array())
        .filter(|entries| !entries.is_empty())?;
    let lines = items
        .iter()
        .take(6)
        .filter_map(|item| item.as_str())
        .map(|item| format!("• {item}"))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(format!("Changes:\n{}", lines.join("\n")))
    }
}

fn assistant_message_text(value: &serde_json::Value) -> Option<String> {
    value
        .get("assistant_message")
        .and_then(|entry| entry.get("text"))
        .and_then(|entry| entry.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn value_string<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_str()))
}

fn value_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_u64()))
}

fn yes_required_text(noun: &str, id: &str, example: &str) -> String {
    format!("⚠️ Refusing to {noun} `{id}` without confirmation.\nRun `{example} --yes` if you really want to continue.")
}

fn extract_tool_output(json: &serde_json::Value) -> String {
    json.get("output")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn json_request(
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let mut request = add_auth(
        client.request(method, format!("{base_url}{path}")),
        api_token,
    );
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = resp.status();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    if status.is_success() {
        Ok(json)
    } else {
        let detail = json
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("request failed");
        Err(format!("{detail} (HTTP {status})"))
    }
}

async fn tool_execute(
    tool: &str,
    args: serde_json::Value,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    json_request(
        reqwest::Method::POST,
        "/tool/execute",
        Some(serde_json::json!({ "tool": tool, "args": args })),
        base_url,
        api_token,
    )
    .await
}

async fn active_session_details(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> Option<serde_json::Value> {
    let sid = active_session_id(msg, session_map).await?;
    json_request(
        reqwest::Method::GET,
        &format!("/session/{sid}"),
        None,
        base_url,
        api_token,
    )
    .await
    .ok()
}

async fn workflow_plan_post(
    path: &str,
    body: serde_json::Value,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let resp = add_auth(client.post(format!("{base_url}{path}")), api_token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = resp.status();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("could not parse server response: {error}"))?;
    if status.is_success() {
        Ok(json)
    } else {
        let detail = json
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("workflow planner request failed");
        Err(format!("{detail} (HTTP {status})"))
    }
}

async fn automations_command_text(
    action: AutomationsCommand,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        AutomationsCommand::Help => automations_help_text(),
        AutomationsCommand::List => automations_list_text(base_url, api_token).await,
        AutomationsCommand::Show { automation_id } => {
            automation_show_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Runs {
            automation_id,
            limit,
        } => automation_runs_text(automation_id, limit, base_url, api_token).await,
        AutomationsCommand::Run { automation_id } => {
            automation_run_now_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Pause { automation_id } => {
            automation_pause_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Resume { automation_id } => {
            automation_resume_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Delete {
            automation_id,
            confirmed,
        } => automation_delete_text(automation_id, confirmed, base_url, api_token).await,
    }
}

async fn runs_command_text(action: RunsCommand, base_url: &str, api_token: &str) -> String {
    match action {
        RunsCommand::Help => runs_help_text(),
        RunsCommand::Automations { limit } => runs_list_text(limit, base_url, api_token).await,
        RunsCommand::Show { run_id } => run_show_text(run_id, base_url, api_token).await,
        RunsCommand::Pause { run_id } => run_pause_text(run_id, base_url, api_token).await,
        RunsCommand::Resume { run_id } => run_resume_text(run_id, base_url, api_token).await,
        RunsCommand::Cancel { run_id } => run_cancel_text(run_id, base_url, api_token).await,
        RunsCommand::Artifacts { run_id } => run_artifacts_text(run_id, base_url, api_token).await,
    }
}

async fn memory_command_text(
    action: MemoryCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    match action {
        MemoryCommand::Help => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_memory_help_text()
            } else {
                memory_help_text()
            }
        }
        MemoryCommand::Search { query } => {
            memory_search_text(
                query,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Recent { limit } => {
            memory_recent_text(
                limit,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Save { text } => {
            memory_save_text(
                text,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Scopes => {
            memory_scopes_text(msg, base_url, api_token, session_map, security_profile).await
        }
        MemoryCommand::Delete {
            memory_id,
            confirmed,
        } => {
            memory_delete_text(
                memory_id,
                confirmed,
                msg,
                base_url,
                api_token,
                security_profile,
            )
            .await
        }
    }
}

async fn workspace_command_text(
    action: WorkspaceCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    match action {
        WorkspaceCommand::Help => workspace_help_text(),
        WorkspaceCommand::Show => workspace_show_text(msg, base_url, api_token, session_map).await,
        WorkspaceCommand::Status => {
            workspace_status_text(msg, base_url, api_token, session_map).await
        }
        WorkspaceCommand::Files { query } => {
            workspace_files_text(query, msg, base_url, api_token, session_map).await
        }
        WorkspaceCommand::Branch => {
            workspace_branch_text(msg, base_url, api_token, session_map).await
        }
    }
}

async fn mcp_command_text(
    action: McpCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        McpCommand::Help => mcp_help_text(ChannelSecurityProfile::Operator),
        McpCommand::List => mcp_list_text(base_url, api_token).await,
        McpCommand::Tools { server } => mcp_tools_text(server, base_url, api_token).await,
        McpCommand::Resources => mcp_resources_text(base_url, api_token).await,
        McpCommand::Status => mcp_status_text(base_url, api_token).await,
        McpCommand::Connect { name } => mcp_connect_text(name, base_url, api_token).await,
        McpCommand::Disconnect { name } => mcp_disconnect_text(name, base_url, api_token).await,
        McpCommand::Refresh { name } => mcp_refresh_text(name, base_url, api_token).await,
        McpCommand::ChannelEnable { name } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            if !prefs.enabled_mcp_servers.contains(&name) {
                prefs.enabled_mcp_servers.push(name.clone());
            }
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            format!("✅ MCP server `{}` enabled for this channel.", name)
        }
        McpCommand::ChannelDisable { name } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            prefs.enabled_mcp_servers.retain(|s| s != &name);
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            format!("🚫 MCP server `{}` disabled for this channel.", name)
        }
    }
}

async fn packs_command_text(action: PacksCommand, base_url: &str, api_token: &str) -> String {
    match action {
        PacksCommand::Help => packs_help_text(),
        PacksCommand::List => packs_list_text(base_url, api_token).await,
        PacksCommand::Show { selector } => packs_show_text(selector, base_url, api_token).await,
        PacksCommand::Updates { selector } => {
            packs_updates_text(selector, base_url, api_token).await
        }
        PacksCommand::Install { target } => packs_install_text(target, base_url, api_token).await,
        PacksCommand::Uninstall {
            selector,
            confirmed,
        } => packs_uninstall_text(selector, confirmed, base_url, api_token).await,
    }
}

async fn config_command_text(action: ConfigCommand, base_url: &str, api_token: &str) -> String {
    match action {
        ConfigCommand::Help => config_help_text(ChannelSecurityProfile::Operator),
        ConfigCommand::Show => config_show_text(base_url, api_token).await,
        ConfigCommand::Providers => providers_text(base_url, api_token).await,
        ConfigCommand::Channels => config_channels_text(base_url, api_token).await,
        ConfigCommand::Model => config_model_text(base_url, api_token).await,
        ConfigCommand::SetModel { model_id } => set_model_text(model_id, base_url, api_token).await,
    }
}

async fn automations_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/automations/v2",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let items = json
                .get("automations")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return "ℹ️ No automations found.".to_string();
            }
            let lines = items
                .iter()
                .take(12)
                .map(|item| {
                    let id = value_string(item, &["id", "automationId", "automation_id"])
                        .unwrap_or("unknown");
                    let name = value_string(item, &["name"]).unwrap_or("Untitled");
                    let status = value_string(item, &["status"]).unwrap_or("unknown");
                    format!("• `{}` {} ({})", short_id(id), name, status)
                })
                .collect::<Vec<_>>();
            format!(
                "⚙️ Automations ({} total):\n{}\nUse `/automations show <id>` for details.",
                items.len(),
                lines.join("\n")
            )
        }
        Err(error) => format!("⚠️ Could not list automations: {error}"),
    }
}

async fn automation_show_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/{}",
            sanitize_resource_segment(&automation_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let automation = json.get("automation").unwrap_or(&json);
            let name = value_string(automation, &["name"]).unwrap_or("Untitled");
            let status = value_string(automation, &["status"]).unwrap_or("unknown");
            let workspace =
                value_string(automation, &["workspace_root", "workspaceRoot"]).unwrap_or("-");
            let schedule = automation
                .get("schedule")
                .map(compact_json)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "⚙️ Automation `{}`\nName: {}\nStatus: {}\nWorkspace: {}\nSchedule: {}",
                automation_id, name, status, workspace, schedule
            )
        }
        Err(error) => format!("⚠️ Could not load automation `{automation_id}`: {error}"),
    }
}

async fn automation_runs_text(
    automation_id: String,
    limit: usize,
    base_url: &str,
    api_token: &str,
) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/{}/runs?limit={}",
            sanitize_resource_segment(&automation_id),
            limit
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format_runs_list(&json, &format!("Runs for `{automation_id}`")),
        Err(error) => format!("⚠️ Could not list runs for `{automation_id}`: {error}"),
    }
}

async fn automation_run_now_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/run_now",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let run_id = json
                .get("run")
                .and_then(|value| value.get("runId").or_else(|| value.get("run_id")))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            format!(
                "▶️ Started automation `{automation_id}`.\nRun: `{}`",
                short_id(run_id)
            )
        }
        Err(error) => format!("⚠️ Could not run automation `{automation_id}`: {error}"),
    }
}

async fn automation_pause_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/pause",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("⏸️ Paused automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not pause automation `{automation_id}`: {error}"),
    }
}

async fn automation_resume_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/resume",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("▶️ Resumed automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not resume automation `{automation_id}`: {error}"),
    }
}

async fn automation_delete_text(
    automation_id: String,
    confirmed: bool,
    base_url: &str,
    api_token: &str,
) -> String {
    if !confirmed {
        return yes_required_text(
            "delete automation",
            &automation_id,
            &format!("/automations delete {automation_id}"),
        );
    }
    match json_request(
        reqwest::Method::DELETE,
        &format!(
            "/automations/v2/{}",
            sanitize_resource_segment(&automation_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🗑️ Deleted automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not delete automation `{automation_id}`: {error}"),
    }
}

fn format_runs_list(json: &serde_json::Value, title: &str) -> String {
    let runs = json
        .get("runs")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if runs.is_empty() {
        return format!("ℹ️ {title}: no runs found.");
    }
    let lines = runs
        .iter()
        .take(12)
        .map(|run| {
            let run_id = value_string(run, &["runId", "run_id", "id"]).unwrap_or("unknown");
            let status = value_string(run, &["status"]).unwrap_or("unknown");
            format!("• `{}` {}", short_id(run_id), status)
        })
        .collect::<Vec<_>>();
    format!("{title}\n{}", lines.join("\n"))
}

async fn runs_list_text(limit: usize, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/automations/v2/runs?limit={limit}"),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format_runs_list(&json, "🏃 Recent automation runs"),
        Err(error) => format!("⚠️ Could not list runs: {error}"),
    }
}

async fn run_show_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/runs/{}",
            sanitize_resource_segment(&run_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let run = json.get("run").unwrap_or(&json);
            let status = value_string(run, &["status"]).unwrap_or("unknown");
            let automation_id =
                value_string(run, &["automationId", "automation_id"]).unwrap_or("-");
            let active_sessions = run
                .get("activeSessionIds")
                .or_else(|| run.get("active_session_ids"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            format!(
                "🏃 Run `{}`\nStatus: {}\nAutomation: {}\nActive sessions: {}",
                run_id, status, automation_id, active_sessions
            )
        }
        Err(error) => format!("⚠️ Could not load run `{run_id}`: {error}"),
    }
}

async fn run_pause_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/pause",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("⏸️ Paused run `{run_id}`."),
        Err(error) => format!("⚠️ Could not pause run `{run_id}`: {error}"),
    }
}

async fn run_resume_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/resume",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("▶️ Resumed run `{run_id}`."),
        Err(error) => format!("⚠️ Could not resume run `{run_id}`: {error}"),
    }
}

async fn run_cancel_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/cancel",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🛑 Cancelled run `{run_id}`."),
        Err(error) => format!("⚠️ Could not cancel run `{run_id}`: {error}"),
    }
}

async fn run_artifacts_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/runs/{}/artifacts",
            sanitize_resource_segment(&run_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let artifacts = json
                .get("artifacts")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if artifacts.is_empty() {
                return format!("ℹ️ Run `{run_id}` has no artifacts.");
            }
            let lines = artifacts
                .iter()
                .take(12)
                .map(|artifact| {
                    let kind = value_string(artifact, &["kind"]).unwrap_or("artifact");
                    let uri = value_string(artifact, &["uri"]).unwrap_or("-");
                    format!("• {} — {}", kind, truncate_for_channel(uri, 90))
                })
                .collect::<Vec<_>>();
            format!("📎 Artifacts for `{run_id}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list artifacts for `{run_id}`: {error}"),
    }
}

async fn memory_search_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        args["query"] = serde_json::json!(query);
        args["limit"] = serde_json::json!(5);
        args["tier"] = serde_json::json!("project");
        return match tool_execute("memory_search", args, base_url, api_token).await {
            Ok(json) => {
                let results = parse_tool_output_rows(&json);
                if results.is_empty() {
                    return "ℹ️ No matching memory entries found.".to_string();
                }
                let lines = results
                    .iter()
                    .take(5)
                    .map(|item| {
                        let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                        let content = value_string(item, &["content", "text"]).unwrap_or("");
                        format!(
                            "• `{}` {}",
                            short_id(id),
                            truncate_for_channel(content, 120)
                        )
                    })
                    .collect::<Vec<_>>();
                format!("🧠 Memory search results:\n{}", lines.join("\n"))
            }
            Err(error) => format!("⚠️ Could not search memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::POST,
        "/memory/search",
        Some(serde_json::json!({ "query": query, "limit": 5 })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let results = json
                .get("results")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if results.is_empty() {
                return "ℹ️ No matching memory entries found.".to_string();
            }
            let lines = results
                .iter()
                .take(5)
                .map(|item| {
                    let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                    let content = value_string(item, &["content", "text"]).unwrap_or("");
                    format!(
                        "• `{}` {}",
                        short_id(id),
                        truncate_for_channel(content, 120)
                    )
                })
                .collect::<Vec<_>>();
            format!("🧠 Memory search results:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not search memory: {error}"),
    }
}

async fn public_channel_memory_tool_args(
    msg: &ChannelMessage,
    session_map: &SessionMap,
) -> serde_json::Value {
    let mut args = serde_json::json!({
        "__project_id": public_channel_memory_scope_key(msg),
        "__memory_max_visible_scope": "project"
    });
    if let Some(session_id) = active_session_id(msg, session_map).await {
        args["__session_id"] = serde_json::json!(session_id);
    }
    args
}

fn parse_tool_output_rows(json: &serde_json::Value) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&extract_tool_output(json))
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

async fn memory_recent_text(
    limit: usize,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        args["limit"] = serde_json::json!(limit);
        args["tier"] = serde_json::json!("project");
        return match tool_execute("memory_list", args, base_url, api_token).await {
            Ok(json) => {
                let items = parse_tool_output_rows(&json);
                if items.is_empty() {
                    return "ℹ️ No memory entries found.".to_string();
                }
                let lines = items
                    .iter()
                    .take(limit)
                    .map(|item| {
                        let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                        let text = value_string(item, &["content", "text"]).unwrap_or("");
                        format!("• `{}` {}", short_id(id), truncate_for_channel(text, 120))
                    })
                    .collect::<Vec<_>>();
                format!("🧠 Recent memory:\n{}", lines.join("\n"))
            }
            Err(error) => format!("⚠️ Could not list memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::GET,
        &format!("/memory?limit={limit}"),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let items = json
                .get("items")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return "ℹ️ No memory entries found.".to_string();
            }
            let lines = items
                .iter()
                .take(limit)
                .map(|item| {
                    let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                    let text = value_string(item, &["content", "text"]).unwrap_or("");
                    format!("• `{}` {}", short_id(id), truncate_for_channel(text, 120))
                })
                .collect::<Vec<_>>();
            format!("🧠 Recent memory:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list memory: {error}"),
    }
}

async fn memory_save_text(
    text: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        let session_id = active_session_id(msg, session_map).await;
        args["content"] = serde_json::json!(text);
        args["tier"] = serde_json::json!("project");
        args["source"] = serde_json::json!("public_channel_memory");
        args["metadata"] = serde_json::json!({
            "security_profile": "public_demo",
            "channel": msg.channel,
            "scope_id": msg.scope.id,
            "scope_kind": format!("{:?}", msg.scope.kind).to_ascii_lowercase(),
            "sender": msg.sender,
            "active_session_id": session_id,
        });
        return match tool_execute("memory_store", args, base_url, api_token).await {
            Ok(json) => {
                let id = json
                    .get("metadata")
                    .and_then(|v| v.get("chunk_ids"))
                    .and_then(|v| v.as_array())
                    .and_then(|v| v.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("💾 Saved memory entry `{}`.", short_id(id))
            }
            Err(error) => format!("⚠️ Could not save memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::POST,
        "/memory/put",
        Some(serde_json::json!({ "text": text })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let id = value_string(&json, &["id", "chunk_id"]).unwrap_or("unknown");
            format!("💾 Saved memory entry `{}`.", short_id(id))
        }
        Err(error) => format!("⚠️ Could not save memory: {error}"),
    }
}

async fn memory_scopes_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    let sid = active_session_id(msg, session_map).await;
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let project_id = public_channel_memory_scope_key(msg);
        return format!(
            "🧠 Memory scopes\nSession: {}\nPublic channel scope: {}\nWorkspace: disabled\nGlobal: disabled\n\nThis public memory is quarantined to the current channel scope.",
            sid.as_deref().unwrap_or("-"),
            project_id,
        );
    }
    let details = active_session_details(msg, base_url, api_token, session_map).await;
    let project_id = details
        .as_ref()
        .and_then(|value| value.get("project_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let workspace_root = details
        .as_ref()
        .and_then(|value| value.get("workspace_root"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    format!(
        "🧠 Memory scopes\nSession: {}\nProject: {}\nWorkspace: {}\nGlobal: enabled via default memory search behavior",
        sid.as_deref().unwrap_or("-"),
        project_id,
        workspace_root
    )
}

async fn memory_delete_text(
    memory_id: String,
    confirmed: bool,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    security_profile: ChannelSecurityProfile,
) -> String {
    if !confirmed {
        return yes_required_text(
            "delete memory",
            &memory_id,
            &format!("/memory delete {memory_id}"),
        );
    }
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let args = serde_json::json!({
            "chunk_id": memory_id,
            "tier": "project",
            "__project_id": public_channel_memory_scope_key(msg),
            "__memory_max_visible_scope": "project"
        });
        return match tool_execute("memory_delete", args, base_url, api_token).await {
            Ok(json) => {
                let deleted = json
                    .get("metadata")
                    .and_then(|v| v.get("deleted"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if deleted {
                    format!("🗑️ Deleted memory `{memory_id}`.")
                } else {
                    let detail = extract_tool_output(&json);
                    format!("⚠️ Could not delete memory `{memory_id}`: {detail}")
                }
            }
            Err(error) => format!("⚠️ Could not delete memory `{memory_id}`: {error}"),
        };
    }

    match json_request(
        reqwest::Method::DELETE,
        &format!("/memory/{}", sanitize_resource_segment(&memory_id)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🗑️ Deleted memory `{memory_id}`."),
        Err(error) => format!("⚠️ Could not delete memory `{memory_id}`: {error}"),
    }
}

async fn workspace_show_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let session_id = value_string(&details, &["id"]).unwrap_or("-");
    let title = value_string(&details, &["title"]).unwrap_or("Untitled");
    let project_id = value_string(&details, &["project_id"]).unwrap_or("-");
    let workspace_root = value_string(&details, &["workspace_root", "directory"]).unwrap_or("-");
    format!(
        "📁 Workspace binding\nSession: `{}`\nTitle: {}\nProject: {}\nWorkspace: {}",
        short_id(session_id),
        title,
        project_id,
        workspace_root
    )
}

async fn workspace_status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let message_count = details
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let project_id = value_string(&details, &["project_id"]).unwrap_or("-");
    let workspace_root = value_string(&details, &["workspace_root", "directory"]).unwrap_or("-");
    format!(
        "📁 Workspace status\nMessages: {}\nProject: {}\nWorkspace: {}",
        message_count, project_id, workspace_root
    )
}

async fn workspace_files_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let Some(workspace_root) = value_string(&details, &["workspace_root", "directory"]) else {
        return "ℹ️ No workspace root is bound to this session.".to_string();
    };
    let pattern = format!("**/*{query}*");
    match tool_execute(
        "glob",
        serde_json::json!({
            "pattern": pattern,
            "__workspace_root": workspace_root,
            "__effective_cwd": workspace_root,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let output = extract_tool_output(&json);
            if output.is_empty() {
                return format!("ℹ️ No files matching `{query}`.");
            }
            let lines = output.lines().take(12).collect::<Vec<_>>();
            format!("📁 Files matching `{query}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not search workspace files: {error}"),
    }
}

async fn workspace_branch_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let Some(workspace_root) = value_string(&details, &["workspace_root", "directory"]) else {
        return "ℹ️ No workspace root is bound to this session.".to_string();
    };
    match tool_execute(
        "bash",
        serde_json::json!({
            "command": "git rev-parse --abbrev-ref HEAD",
            "__workspace_root": workspace_root,
            "__effective_cwd": workspace_root,
            "timeout_ms": 5000,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let branch = extract_tool_output(&json);
            if branch.is_empty() {
                "ℹ️ No git branch information found.".to_string()
            } else {
                format!("🌿 Current branch: `{}`", branch.trim())
            }
        }
        Err(error) => format!("⚠️ Could not read workspace branch: {error}"),
    }
}

async fn tools_command_text(
    action: ToolsCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        ToolsCommand::Help => tools_help_text(ChannelSecurityProfile::Operator),
        ToolsCommand::List => {
            let prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let enabled: std::collections::HashSet<String> =
                prefs.enabled_tools.iter().cloned().collect();
            let disabled: std::collections::HashSet<String> =
                prefs.disabled_tools.iter().cloned().collect();

            let all_tools = [
                "read",
                "glob",
                "ls",
                "list",
                "grep",
                "codesearch",
                "search",
                "websearch",
                "webfetch",
                "webfetch_html",
                "bash",
                "write",
                "edit",
                "apply_patch",
                "todowrite",
                "memory_search",
                "memory_store",
                "memory_list",
                "skill",
                "task",
                "question",
                "pack_builder",
            ];

            let mut default_lines: Vec<String> = Vec::new();
            let mut disabled_lines: Vec<String> = Vec::new();

            for tool in all_tools {
                if disabled.contains(tool) {
                    disabled_lines.push(tool.to_string());
                } else if !prefs.enabled_tools.is_empty() && !enabled.contains(tool) {
                    disabled_lines.push(tool.to_string());
                } else {
                    default_lines.push(tool.to_string());
                }
            }

            let mut lines = Vec::new();
            if !default_lines.is_empty() {
                lines.push(format!("*Enabled:* {}", default_lines.join(", ")));
            }
            if !disabled_lines.is_empty() {
                lines.push(format!("*Disabled:* {}", disabled_lines.join(", ")));
            }

            let mcp_servers = mcp_servers_for_channel(base_url, api_token).await;
            if !mcp_servers.is_empty() {
                let enabled_mcp: std::collections::HashSet<String> =
                    prefs.enabled_mcp_servers.iter().cloned().collect();
                let mut mcp_lines = Vec::new();
                for server in &mcp_servers {
                    if !prefs.enabled_mcp_servers.is_empty() && !enabled_mcp.contains(server) {
                        mcp_lines.push(format!("{} (disabled)", server));
                    } else {
                        mcp_lines.push(format!("{} (enabled)", server));
                    }
                }
                lines.push(format!("\n*MCP servers:*\n{}", mcp_lines.join(", ")));
            }

            if lines.is_empty() {
                "ℹ️ No tool preferences set. All built-in tools are available by default."
                    .to_string()
            } else {
                format!("🛠 *Tool Scope for this channel*\n\n{}", lines.join("\n\n"))
            }
        }
        ToolsCommand::Enable { tools } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let mut added = Vec::new();
            for tool in &tools {
                if !prefs.enabled_tools.contains(tool) {
                    prefs.enabled_tools.push(tool.clone());
                    added.push(tool.clone());
                }
                prefs.disabled_tools.retain(|t| t != tool);
            }
            if added.is_empty() {
                "ℹ️ No new tools were enabled.".to_string()
            } else {
                save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
                format!("✅ Enabled for this channel: {}", added.join(", "))
            }
        }
        ToolsCommand::Disable { tools } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let mut added = Vec::new();
            for tool in &tools {
                if !prefs.disabled_tools.contains(tool) {
                    prefs.disabled_tools.push(tool.clone());
                    added.push(tool.clone());
                }
                prefs.enabled_tools.retain(|t| t != tool);
            }
            if added.is_empty() {
                "ℹ️ No new tools were disabled.".to_string()
            } else {
                save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
                format!("🚫 Disabled for this channel: {}", added.join(", "))
            }
        }
        ToolsCommand::Reset => {
            let prefs = ChannelToolPreferences::default();
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            "🔄 Tool preferences reset. All built-in tools are now available by default."
                .to_string()
        }
    }
}

async fn mcp_servers_for_channel(base_url: &str, api_token: &str) -> Vec<String> {
    match json_request(reqwest::Method::GET, "/mcp", None, base_url, api_token).await {
        Ok(json) => {
            let obj = json.as_object();
            obj.map(|m| m.keys().cloned().collect()).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

async fn mcp_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/mcp", None, base_url, api_token).await {
        Ok(json) => {
            let Some(obj) = json.as_object() else {
                return "ℹ️ No MCP servers configured.".to_string();
            };
            if obj.is_empty() {
                return "ℹ️ No MCP servers configured.".to_string();
            }
            let mut lines = obj
                .iter()
                .take(20)
                .map(|(name, value)| {
                    let enabled = value
                        .get("enabled")
                        .and_then(|entry| entry.as_bool())
                        .unwrap_or(true);
                    format!(
                        "• {} ({})",
                        name,
                        if enabled { "enabled" } else { "disabled" }
                    )
                })
                .collect::<Vec<_>>();
            lines.sort();
            format!("🔌 MCP servers:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP servers: {error}"),
    }
}

async fn mcp_tools_text(server: Option<String>, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/mcp/tools",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let tools = json.as_array().cloned().unwrap_or_default();
            if tools.is_empty() {
                return "ℹ️ No MCP tools discovered.".to_string();
            }
            let filtered = tools
                .iter()
                .filter(|tool| {
                    if let Some(server_name) = server.as_ref() {
                        value_string(tool, &["server", "server_name", "mcp_server"])
                            .map(|name| name == server_name)
                            .unwrap_or(false)
                    } else {
                        true
                    }
                })
                .take(20)
                .map(|tool| {
                    let name = value_string(tool, &["name", "tool", "tool_name"]).unwrap_or("tool");
                    let srv =
                        value_string(tool, &["server", "server_name", "mcp_server"]).unwrap_or("?");
                    format!("• {} ({})", name, srv)
                })
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return format!(
                    "ℹ️ No MCP tools found{}.",
                    server
                        .as_ref()
                        .map(|name| format!(" for `{name}`"))
                        .unwrap_or_default()
                );
            }
            format!("🔧 MCP tools:\n{}", filtered.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP tools: {error}"),
    }
}

async fn mcp_resources_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/mcp/resources",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let resources = json.as_array().cloned().unwrap_or_default();
            if resources.is_empty() {
                return "ℹ️ No MCP resources discovered.".to_string();
            }
            let lines = resources
                .iter()
                .take(12)
                .map(|value| truncate_for_channel(&compact_json(value), 120))
                .collect::<Vec<_>>();
            format!("📚 MCP resources:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP resources: {error}"),
    }
}

async fn mcp_status_text(base_url: &str, api_token: &str) -> String {
    mcp_list_text(base_url, api_token).await
}

async fn mcp_connect_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/connect", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🔌 Connected MCP server `{name}`."),
        Err(error) => format!("⚠️ Could not connect `{name}`: {error}"),
    }
}

async fn mcp_disconnect_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/disconnect", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🔌 Disconnected MCP server `{name}`."),
        Err(error) => format!("⚠️ Could not disconnect `{name}`: {error}"),
    }
}

async fn mcp_refresh_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/refresh", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let count = value_u64(&json, &["count"]).unwrap_or(0);
            format!("🔄 Refreshed MCP server `{name}` ({} tool(s)).", count)
        }
        Err(error) => format!("⚠️ Could not refresh `{name}`: {error}"),
    }
}

async fn packs_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/packs", None, base_url, api_token).await {
        Ok(json) => {
            let packs = json
                .get("packs")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if packs.is_empty() {
                return "ℹ️ No packs installed.".to_string();
            }
            let lines = packs
                .iter()
                .take(12)
                .map(|pack| {
                    let name = value_string(pack, &["name"]).unwrap_or("pack");
                    let version = value_string(pack, &["version"]).unwrap_or("?");
                    format!("• {} ({})", name, version)
                })
                .collect::<Vec<_>>();
            format!("📦 Installed packs:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list packs: {error}"),
    }
}

async fn packs_show_text(selector: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/packs/{}", sanitize_resource_segment(&selector)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let installed = json
                .get("pack")
                .and_then(|value| value.get("installed"))
                .unwrap_or(&json);
            let name = value_string(installed, &["name"]).unwrap_or("pack");
            let version = value_string(installed, &["version"]).unwrap_or("?");
            let pack_id = value_string(installed, &["pack_id", "packId"]).unwrap_or("-");
            format!(
                "📦 Pack `{}`\nName: {}\nVersion: {}\nPack ID: {}",
                selector, name, version, pack_id
            )
        }
        Err(error) => format!("⚠️ Could not inspect pack `{selector}`: {error}"),
    }
}

async fn packs_updates_text(selector: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/packs/{}/updates", sanitize_resource_segment(&selector)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let updates = json
                .get("updates")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if updates.is_empty() {
                return format!("ℹ️ No updates available for `{selector}`.");
            }
            let lines = updates
                .iter()
                .take(10)
                .map(|item| truncate_for_channel(&compact_json(item), 120))
                .collect::<Vec<_>>();
            format!("📦 Updates for `{selector}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not check updates for `{selector}`: {error}"),
    }
}

async fn packs_install_text(target: String, base_url: &str, api_token: &str) -> String {
    let body = if target.starts_with("http://") || target.starts_with("https://") {
        serde_json::json!({ "url": target })
    } else {
        serde_json::json!({ "path": target })
    };
    match json_request(
        reqwest::Method::POST,
        "/packs/install",
        Some(body),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let installed = json.get("installed").unwrap_or(&json);
            let name = value_string(installed, &["name"]).unwrap_or("pack");
            let version = value_string(installed, &["version"]).unwrap_or("?");
            format!("📦 Installed pack {} ({}).", name, version)
        }
        Err(error) => format!("⚠️ Could not install pack: {error}"),
    }
}

async fn packs_uninstall_text(
    selector: String,
    confirmed: bool,
    base_url: &str,
    api_token: &str,
) -> String {
    if !confirmed {
        return yes_required_text(
            "uninstall pack",
            &selector,
            &format!("/packs uninstall {selector}"),
        );
    }
    match json_request(
        reqwest::Method::POST,
        "/packs/uninstall",
        Some(serde_json::json!({ "name": selector })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let removed = json.get("removed").unwrap_or(&json);
            let name = value_string(removed, &["name"]).unwrap_or("pack");
            format!("🗑️ Uninstalled pack {}.", name)
        }
        Err(error) => format!("⚠️ Could not uninstall pack: {error}"),
    }
}

async fn config_show_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/config", None, base_url, api_token).await {
        Ok(json) => {
            let default_provider = json
                .get("providers")
                .and_then(|value| value.get("default"))
                .and_then(|value| value.as_str())
                .or_else(|| json.get("default").and_then(|value| value.as_str()))
                .unwrap_or("-");
            let provider_count =
                json.get("providers")
                    .and_then(|value| value.get("providers").or_else(|| value.get("all")))
                    .map(|value| {
                        value.as_object().map(|obj| obj.len()).unwrap_or_else(|| {
                            value.as_array().map(|items| items.len()).unwrap_or(0)
                        })
                    })
                    .unwrap_or(0);
            format!(
                "🛠️ Config summary\nDefault provider: {}\nConfigured providers: {}\nUse `/config providers`, `/config channels`, or `/config model` for details.",
                default_provider, provider_count
            )
        }
        Err(error) => format!("⚠️ Could not load config: {error}"),
    }
}

async fn config_channels_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/channels/status",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format!(
            "📡 Channel status\n{}",
            truncate_for_channel(&compact_json(&json), 500)
        ),
        Err(error) => format!("⚠️ Could not load channel status: {error}"),
    }
}

async fn config_model_text(base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    match fetch_default_model_spec(&client, base_url, api_token).await {
        Ok(Some(spec)) => {
            let provider = value_string(&spec, &["provider_id"]).unwrap_or("-");
            let model = value_string(&spec, &["model_id"]).unwrap_or("-");
            format!("🧠 Default model\nProvider: {}\nModel: {}", provider, model)
        }
        Ok(None) => "ℹ️ No default model is configured.".to_string(),
        Err(error) => format!("⚠️ Could not load default model: {error}"),
    }
}

async fn workflow_plan_get_request(
    plan_id: &str,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let resp = add_auth(
        client.get(format!(
            "{base_url}/workflow-plans/{}",
            sanitize_resource_segment(plan_id)
        )),
        api_token,
    )
    .send()
    .await
    .map_err(|error| format!("request failed: {error}"))?;
    let status = resp.status();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("could not parse server response: {error}"))?;
    if status.is_success() {
        Ok(json)
    } else {
        let detail = json
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("workflow planner request failed");
        Err(format!("{detail} (HTTP {status})"))
    }
}

async fn schedule_plan_text(
    prompt: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let mut body = serde_json::json!({
        "prompt": prompt,
        "plan_source": "channel_slash_command",
    });
    if let Some(workspace_root) =
        workflow_planner_workspace_root(msg, base_url, api_token, session_map).await
    {
        body["workspace_root"] = serde_json::json!(workspace_root);
    }
    match workflow_plan_post("/workflow-plans/chat/start", body, base_url, api_token).await {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no plan.".to_string();
            };
            let mut sections = vec![format!(
                "🗓️ Workflow draft created.\n{}",
                workflow_plan_summary(plan)
            )];
            if let Some(text) = assistant_message_text(&json) {
                sections.push(format!("Planner notes:\n{text}"));
            }
            sections.push(
                "Next steps:\nUse `/schedule edit <plan_id> <message>` to refine it or `/schedule apply <plan_id>` to save it."
                    .to_string(),
            );
            sections.join("\n\n")
        }
        Err(error) => format!("⚠️ Could not create workflow draft: {error}"),
    }
}

async fn schedule_show_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_get_request(&plan_id, base_url, api_token).await {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no plan.".to_string();
            };
            let conversation_count = json
                .get("conversation")
                .and_then(|value| value.get("messages"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            format!(
                "🗓️ Current workflow draft\n{}\nConversation messages: {}",
                workflow_plan_summary(plan),
                conversation_count
            )
        }
        Err(error) => format!("⚠️ Could not load workflow draft `{plan_id}`: {error}"),
    }
}

async fn schedule_edit_text(
    plan_id: String,
    message: String,
    base_url: &str,
    api_token: &str,
) -> String {
    match workflow_plan_post(
        "/workflow-plans/chat/message",
        serde_json::json!({
            "plan_id": plan_id,
            "message": message,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no revised plan.".to_string();
            };
            let mut sections = vec![format!(
                "📝 Workflow draft updated.\n{}",
                workflow_plan_summary(plan)
            )];
            if let Some(change_summary) = workflow_plan_change_summary(&json) {
                sections.push(change_summary);
            }
            if let Some(text) = assistant_message_text(&json) {
                sections.push(format!("Planner notes:\n{text}"));
            }
            sections.join("\n\n")
        }
        Err(error) => format!("⚠️ Could not revise workflow draft: {error}"),
    }
}

async fn schedule_reset_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_post(
        "/workflow-plans/chat/reset",
        serde_json::json!({ "plan_id": plan_id }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no reset plan.".to_string();
            };
            format!(
                "↩️ Workflow draft reset to its initial version.\n{}",
                workflow_plan_summary(plan)
            )
        }
        Err(error) => format!("⚠️ Could not reset workflow draft: {error}"),
    }
}

async fn schedule_apply_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_post(
        "/workflow-plans/apply",
        serde_json::json!({
            "plan_id": plan_id,
            "creator_id": "channel_slash_command",
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let automation_id = json
                .get("automation")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let automation_name = json
                .get("automation")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or("saved automation");
            format!(
                "✅ Workflow draft `{}` applied.\nCreated automation `{}` ({automation_name}).",
                plan_id, automation_id
            )
        }
        Err(error) => format!("⚠️ Could not apply workflow draft: {error}"),
    }
}

async fn active_session_id(msg: &ChannelMessage, session_map: &SessionMap) -> Option<String> {
    let map_key = session_map_key(msg);
    let legacy_key = legacy_session_map_key(msg);
    let mut guard = session_map.lock().await;
    if let Some(record) = guard.get(&map_key) {
        return Some(record.session_id.clone());
    }
    if let Some(mut record) = guard.remove(&legacy_key) {
        record.scope_id = Some(msg.scope.id.clone());
        record.scope_kind = Some(session_scope_kind_label(msg).to_string());
        let session_id = record.session_id.clone();
        guard.insert(map_key, record);
        persist_session_map(&guard).await;
        return Some(session_id);
    }
    None
}

async fn list_sessions_text(msg: &ChannelMessage, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let source_title_prefix = session_title_prefix(msg);

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach Tandem server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    // Filter to sessions whose title starts with the scoped channel prefix.
    let matching: Vec<_> = sessions
        .iter()
        .filter(|s| {
            s.get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.starts_with(&source_title_prefix))
                .unwrap_or(false)
        })
        .take(5)
        .enumerate()
        .map(|(i, s)| {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let msg_count = s
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!(
                "{}. `{}` — {} ({} msgs)",
                i + 1,
                &id[..8.min(id.len())],
                title,
                msg_count
            )
        })
        .collect();

    if matching.is_empty() {
        "📋 No previous sessions found.".to_string()
    } else {
        format!("📋 Your sessions:\n{}", matching.join("\n"))
    }
}

async fn new_session_text(
    name: Option<String>,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    let map_key = session_map_key(msg);
    let display_name = name.clone().unwrap_or_else(|| session_title_prefix(msg));
    let client = reqwest::Client::new();
    let public_memory_project_id = if security_profile == ChannelSecurityProfile::PublicDemo {
        Some(public_channel_memory_scope_key(msg))
    } else {
        None
    };
    let body = build_channel_session_create_body(
        &display_name,
        security_profile,
        public_memory_project_id.as_deref(),
    );

    let Ok(resp) = add_auth(client.post(format!("{base_url}/session")), api_token)
        .json(&body)
        .send()
        .await
    else {
        return "⚠️ Could not create session.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let session_id = match json.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return "⚠️ Server returned no session ID.".to_string(),
    };

    let mut guard = session_map.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    guard.insert(
        map_key,
        SessionRecord {
            session_id: session_id.clone(),
            created_at_ms: now,
            last_seen_at_ms: now,
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            scope_id: Some(msg.scope.id.clone()),
            scope_kind: Some(session_scope_kind_label(msg).to_string()),
            tool_preferences: None,
        },
    );
    persist_session_map(&guard).await;

    format!(
        "✅ Started new session \"{}\" (`{}`)\nFresh context — what would you like to work on?",
        display_name,
        &session_id[..8.min(session_id.len())]
    )
}

async fn resume_session_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = session_map_key(msg);
    let source_prefix = session_title_prefix(msg);
    let client = reqwest::Client::new();

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    let found = sessions.iter().find(|s| {
        // Only search sessions belonging to this sender
        let title_ok = s
            .get("title")
            .and_then(|t| t.as_str())
            .map(|t| t.starts_with(&source_prefix))
            .unwrap_or(false);
        if !title_ok {
            return false;
        }
        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let title = s
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        id.starts_with(&query) || title.contains(&query.to_lowercase())
    });

    match found {
        Some(s) => {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");

            let mut guard = session_map.lock().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            guard.insert(
                map_key,
                SessionRecord {
                    session_id: id.to_string(),
                    created_at_ms: now,
                    last_seen_at_ms: now,
                    channel: msg.channel.clone(),
                    sender: msg.sender.clone(),
                    scope_id: Some(msg.scope.id.clone()),
                    scope_kind: Some(session_scope_kind_label(msg).to_string()),
                    tool_preferences: None,
                },
            );
            persist_session_map(&guard).await;

            format!(
                "✅ Resumed session \"{}\" (`{}`)\n→ Ready to continue.",
                title,
                &id[..8.min(id.len())]
            )
        }
        None => format!(
            "⚠️ No session matching \"{}\" found. Use /sessions to list yours.",
            query
        ),
    }
}

async fn status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let session_id = active_session_id(msg, session_map).await;
    let Some(sid) = session_id else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/session/{sid}")), api_token)
        .send()
        .await
    else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let msgs = json
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    format!(
        "ℹ️ Session: \"{}\" (`{}`) | {} messages",
        title,
        &sid[..8.min(sid.len())],
        msgs
    )
}

async fn rename_session_text(
    name: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let session_id = active_session_id(msg, session_map).await;
    let Some(sid) = session_id else {
        return "⚠️ No active session to rename. Send a message first.".to_string();
    };

    let client = reqwest::Client::new();
    let resp = add_auth(client.patch(format!("{base_url}/session/{sid}")), api_token)
        .json(&serde_json::json!({ "title": name }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => format!("✅ Session renamed to \"{name}\"."),
        Ok(r) => format!("⚠️ Rename failed (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Rename failed: {e}"),
    }
}

async fn run_status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/run")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch run status.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected run status response.".to_string();
    };
    let active = json
        .get("active")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if active.is_null() {
        return "ℹ️ No active run.".to_string();
    }

    let run_id = active
        .get("run_id")
        .or_else(|| active.get("runID"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!(
        "🏃 Active run: `{}` on session `{}`",
        &run_id[..8.min(run_id.len())],
        &sid[..8.min(sid.len())]
    )
}

async fn cancel_run_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — nothing to cancel.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.post(format!("{base_url}/session/{sid}/cancel")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not reach server to cancel.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Cancel requested, but response could not be parsed.".to_string();
    };
    let cancelled = json
        .get("cancelled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if cancelled {
        "🛑 Cancelled active run.".to_string()
    } else {
        "ℹ️ No active run to cancel.".to_string()
    }
}

async fn todos_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/todo")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch todos.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected todos response.".to_string();
    };

    let Some(items) = json.as_array() else {
        return "⚠️ Todos response was not a list.".to_string();
    };
    if items.is_empty() {
        return "✅ No todos in this session.".to_string();
    }

    let lines = items
        .iter()
        .take(12)
        .enumerate()
        .map(|(i, item)| {
            let content = item
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let icon = if status.eq_ignore_ascii_case("completed") {
                "✅"
            } else if status.eq_ignore_ascii_case("in_progress") {
                "⏳"
            } else {
                "⬜"
            };
            format!("{}. {} {} ({})", i + 1, icon, content, status)
        })
        .collect::<Vec<_>>();
    format!("🧾 Session todos:\n{}", lines.join("\n"))
}

fn value_str<'a>(obj: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| obj.get(*key).and_then(|v| v.as_str()))
}

fn value_bool(obj: &serde_json::Value, key: &str) -> Option<bool> {
    obj.get(key).and_then(|v| v.as_bool())
}

fn session_matches(value: &serde_json::Value, session_id: &str) -> bool {
    value_str(value, &["session_id", "sessionID", "sessionId"])
        .map(|v| v == session_id)
        .unwrap_or(false)
}

async fn requests_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let sid = active_session_id(msg, session_map).await;
    let client = reqwest::Client::new();

    let permissions = match add_auth(client.get(format!("{base_url}/permission")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("requests").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let questions = match add_auth(client.get(format!("{base_url}/question")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let filtered_permissions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        permissions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        permissions
    };
    let filtered_questions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        questions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        questions
    };

    if filtered_permissions.is_empty() && filtered_questions.is_empty() {
        return "✅ No pending requests.".to_string();
    }

    let mut lines = Vec::new();
    for req in filtered_permissions.iter().take(8) {
        let id = value_str(req, &["id", "requestID", "request_id"]).unwrap_or("?");
        let tool = value_str(req, &["tool", "tool_name", "name"]).unwrap_or("tool");
        let approved = value_bool(req, "approved");
        let status = if approved == Some(true) {
            "approved"
        } else {
            "pending"
        };
        lines.push(format!(
            "🔐 `{}` {} ({})",
            &id[..8.min(id.len())],
            tool,
            status
        ));
    }
    for q in filtered_questions.iter().take(8) {
        let id = value_str(q, &["id", "questionID", "question_id"]).unwrap_or("?");
        let prompt = value_str(q, &["prompt", "question", "text"]).unwrap_or("question");
        lines.push(format!(
            "❓ `{}` {}",
            &id[..8.min(id.len())],
            prompt.chars().take(80).collect::<String>()
        ));
    }

    format!(
        "🧷 Pending requests ({} tool, {} question):\n{}",
        filtered_permissions.len(),
        filtered_questions.len(),
        lines.join("\n")
    )
}

async fn answer_question_text(
    question_id: String,
    answer: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — cannot answer question.".to_string();
    };
    let client = reqwest::Client::new();
    let url = format!("{base_url}/sessions/{sid}/questions/{question_id}/answer");
    let resp = add_auth(client.post(url), api_token)
        .json(&serde_json::json!({ "answer": answer }))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            format!("✅ Answer submitted for question `{question_id}`.")
        }
        Ok(r) => format!("⚠️ Could not answer question (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not answer question: {e}"),
    }
}

async fn providers_text(base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch providers.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected providers response.".to_string();
    };
    let default = json
        .get("default")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers available.".to_string();
    }
    let lines = all
        .iter()
        .take(20)
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let model_count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {} ({} models)", id, model_count)
        })
        .collect::<Vec<_>>();
    format!("🧠 Providers (default: `{default}`):\n{}", lines.join("\n"))
}

async fn models_text(provider: Option<String>, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch models.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected models response.".to_string();
    };
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers/models available.".to_string();
    }

    if let Some(provider_id) = provider {
        let target = all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.eq_ignore_ascii_case(&provider_id))
                .unwrap_or(false)
        });
        let Some(entry) = target else {
            return format!("⚠️ Provider `{provider_id}` not found. Use /providers.");
        };
        let models = entry
            .get("models")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if models.is_empty() {
            return format!("ℹ️ Provider `{provider_id}` has no models listed.");
        }
        let mut model_ids = models.keys().cloned().collect::<Vec<_>>();
        model_ids.sort();
        let lines = model_ids
            .iter()
            .take(30)
            .map(|m| format!("• {m}"))
            .collect::<Vec<_>>();
        return format!("🧠 Models for `{provider_id}`:\n{}", lines.join("\n"));
    }

    let lines = all
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {}: {} models", id, count)
        })
        .collect::<Vec<_>>();
    format!(
        "🧠 Model catalog by provider:\n{}\nUse `/models <provider>` to list model IDs.",
        lines.join("\n")
    )
}

async fn set_model_text(model_id: String, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch provider catalog.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected provider catalog response.".to_string();
    };

    let Some(default_provider) = json.get("default").and_then(|v| v.as_str()) else {
        return "⚠️ No default provider configured. Use desktop/TUI provider setup first."
            .to_string();
    };

    let provider_entry = json.get("all").and_then(|v| v.as_array()).and_then(|all| {
        all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == default_provider)
                .unwrap_or(false)
        })
    });

    if let Some(entry) = provider_entry {
        let known = entry
            .get("models")
            .and_then(|v| v.as_object())
            .map(|models| models.contains_key(&model_id))
            .unwrap_or(true);
        if !known {
            return format!(
                "⚠️ Model `{}` not found for provider `{}`. Use `/models {}` first.",
                model_id, default_provider, default_provider
            );
        }
    }

    let mut provider_patch = serde_json::Map::new();
    provider_patch.insert(
        "default_model".to_string(),
        serde_json::json!(model_id.clone()),
    );
    let mut providers_patch = serde_json::Map::new();
    providers_patch.insert(
        default_provider.to_string(),
        serde_json::Value::Object(provider_patch),
    );
    let mut patch_map = serde_json::Map::new();
    patch_map.insert(
        "providers".to_string(),
        serde_json::Value::Object(providers_patch),
    );
    let patch = serde_json::Value::Object(patch_map);

    let resp = add_auth(client.patch(format!("{base_url}/config")), api_token)
        .json(&patch)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            format!(
                "✅ Model set to `{}` for default provider `{}`.",
                model_id, default_provider
            )
        }
        Ok(r) => format!("⚠️ Could not set model (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not set model: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Slash command parser ──────────────────────────────────────────────

    #[test]
    fn parse_new_no_name() {
        assert!(matches!(
            parse_slash_command("/new"),
            Some(SlashCommand::New { name: None })
        ));
    }

    #[test]
    fn parse_new_with_name() {
        let cmd = parse_slash_command("/new my session");
        assert!(matches!(
            cmd,
            Some(SlashCommand::New { name: Some(ref n) }) if n == "my session"
        ));
    }

    #[test]
    fn parse_sessions() {
        assert!(matches!(
            parse_slash_command("/sessions"),
            Some(SlashCommand::ListSessions)
        ));
        assert!(matches!(
            parse_slash_command("/session"),
            Some(SlashCommand::ListSessions)
        ));
    }

    #[test]
    fn parse_resume() {
        let cmd = parse_slash_command("/resume abc123");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Resume { ref query }) if query == "abc123"
        ));
    }

    #[test]
    fn parse_rename() {
        let cmd = parse_slash_command("/rename new name");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Rename { ref name }) if name == "new name"
        ));
    }

    #[test]
    fn parse_status() {
        assert!(matches!(
            parse_slash_command("/status"),
            Some(SlashCommand::Status)
        ));
    }

    #[test]
    fn parse_run() {
        assert!(matches!(
            parse_slash_command("/run"),
            Some(SlashCommand::Run)
        ));
    }

    #[test]
    fn parse_cancel_aliases() {
        assert!(matches!(
            parse_slash_command("/cancel"),
            Some(SlashCommand::Cancel)
        ));
        assert!(matches!(
            parse_slash_command("/abort"),
            Some(SlashCommand::Cancel)
        ));
    }

    #[test]
    fn parse_todos_aliases() {
        assert!(matches!(
            parse_slash_command("/todos"),
            Some(SlashCommand::Todos)
        ));
        assert!(matches!(
            parse_slash_command("/todo"),
            Some(SlashCommand::Todos)
        ));
    }

    #[test]
    fn parse_requests() {
        assert!(matches!(
            parse_slash_command("/requests"),
            Some(SlashCommand::Requests)
        ));
    }

    #[test]
    fn parse_answer() {
        let cmd = parse_slash_command("/answer q123 continue with option A");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Answer { ref question_id, ref answer })
            if question_id == "q123" && answer == "continue with option A"
        ));
    }

    #[test]
    fn parse_providers() {
        assert!(matches!(
            parse_slash_command("/providers"),
            Some(SlashCommand::Providers)
        ));
    }

    #[test]
    fn parse_models() {
        assert!(matches!(
            parse_slash_command("/models"),
            Some(SlashCommand::Models { provider: None })
        ));
        let cmd = parse_slash_command("/models openrouter");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Models { provider: Some(ref p) }) if p == "openrouter"
        ));
    }

    #[test]
    fn parse_model_set() {
        let cmd = parse_slash_command("/model gpt-5-mini");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Model { ref model_id }) if model_id == "gpt-5-mini"
        ));
    }

    #[test]
    fn parse_help() {
        assert!(matches!(
            parse_slash_command("/help"),
            Some(SlashCommand::Help { topic: None })
        ));
        assert!(matches!(
            parse_slash_command("/?"),
            Some(SlashCommand::Help { topic: None })
        ));
        assert!(matches!(
            parse_slash_command("/help schedule"),
            Some(SlashCommand::Help { topic: Some(ref topic) }) if topic == "schedule"
        ));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert!(parse_slash_command("/unknown").is_none());
        assert!(parse_slash_command("not a command").is_none());
        assert!(parse_slash_command("").is_none());
    }

    #[test]
    fn parse_trims_whitespace() {
        assert!(matches!(
            parse_slash_command("  /help  "),
            Some(SlashCommand::Help { topic: None })
        ));
    }

    #[test]
    fn parse_schedule_help_and_default() {
        assert!(matches!(
            parse_slash_command("/schedule"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Help
            })
        ));
        assert!(matches!(
            parse_slash_command("/schedule help"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Help
            })
        ));
    }

    #[test]
    fn parse_schedule_plan() {
        let cmd = parse_slash_command("/schedule plan daily repo summary at 9am");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Plan { ref prompt }
            }) if prompt == "daily repo summary at 9am"
        ));
    }

    #[test]
    fn parse_schedule_show() {
        let cmd = parse_slash_command("/schedule show wfplan-123");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Show { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
    }

    #[test]
    fn parse_schedule_edit() {
        let cmd = parse_slash_command("/schedule edit wfplan-123 change this to every monday");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Edit {
                    ref plan_id,
                    ref message
                }
            }) if plan_id == "wfplan-123" && message == "change this to every monday"
        ));
    }

    #[test]
    fn parse_schedule_reset_and_apply() {
        assert!(matches!(
            parse_slash_command("/schedule reset wfplan-123"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Reset { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
        assert!(matches!(
            parse_slash_command("/schedule apply wfplan-123"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Apply { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
    }

    #[test]
    fn parse_automations_commands() {
        assert!(matches!(
            parse_slash_command("/automations"),
            Some(SlashCommand::Automations {
                action: AutomationsCommand::List
            })
        ));
        assert!(matches!(
            parse_slash_command("/automations delete auto-1 --yes"),
            Some(SlashCommand::Automations {
                action: AutomationsCommand::Delete {
                    ref automation_id,
                    confirmed: true
                }
            }) if automation_id == "auto-1"
        ));
    }

    #[test]
    fn parse_runs_memory_workspace_commands() {
        assert!(matches!(
            parse_slash_command("/runs artifacts run-1"),
            Some(SlashCommand::Runs {
                action: RunsCommand::Artifacts { ref run_id }
            }) if run_id == "run-1"
        ));
        assert!(matches!(
            parse_slash_command("/memory search deployment notes"),
            Some(SlashCommand::Memory {
                action: MemoryCommand::Search { ref query }
            }) if query == "deployment notes"
        ));
        assert!(matches!(
            parse_slash_command("/workspace files dispatcher"),
            Some(SlashCommand::Workspace {
                action: WorkspaceCommand::Files { ref query }
            }) if query == "dispatcher"
        ));
    }

    #[test]
    fn parse_mcp_packs_and_config_commands() {
        assert!(matches!(
            parse_slash_command("/mcp refresh github-only"),
            Some(SlashCommand::Mcp {
                action: McpCommand::Refresh { ref name }
            }) if name == "github-only"
        ));
        assert!(matches!(
            parse_slash_command("/packs uninstall starter-pack --yes"),
            Some(SlashCommand::Packs {
                action: PacksCommand::Uninstall {
                    ref selector,
                    confirmed: true
                }
            }) if selector == "starter-pack"
        ));
        assert!(matches!(
            parse_slash_command("/config set-model gpt-5-mini"),
            Some(SlashCommand::Config {
                action: ConfigCommand::SetModel { ref model_id }
            }) if model_id == "gpt-5-mini"
        ));
    }

    #[test]
    fn help_text_lists_schedule_topic() {
        let help = help_text(None, ChannelSecurityProfile::Operator);
        assert!(help.contains("/schedule help"));
        assert!(help.contains("/help schedule"));
        assert!(help.contains("/automations"));
        assert!(help.contains("/memory"));
    }

    #[test]
    fn schedule_help_text_lists_subcommands() {
        let help = help_text(Some("schedule"), ChannelSecurityProfile::Operator);
        assert!(help.contains("/schedule plan <prompt>"));
        assert!(help.contains("/schedule apply <plan_id>"));
    }

    #[test]
    fn topic_help_for_new_namespaces() {
        assert!(
            help_text(Some("automations"), ChannelSecurityProfile::Operator)
                .contains("/automations run <id>")
        );
        assert!(help_text(Some("memory"), ChannelSecurityProfile::Operator)
            .contains("/memory save <text>"));
        assert!(
            help_text(Some("workspace"), ChannelSecurityProfile::Operator)
                .contains("/workspace branch")
        );
        assert!(help_text(Some("mcp"), ChannelSecurityProfile::Operator)
            .contains("/mcp tools [server]"));
        assert!(help_text(Some("packs"), ChannelSecurityProfile::Operator)
            .contains("/packs install <path-or-url>"));
        assert!(help_text(Some("config"), ChannelSecurityProfile::Operator)
            .contains("/config set-model <model_id>"));
    }

    #[test]
    fn detects_pack_builder_intent() {
        let text = "create me a pack that checks latest headline news and posts to slack";
        assert!(is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert_eq!(route.agent.as_deref(), Some("pack_builder"));
        assert!(route
            .tool_allowlist
            .as_ref()
            .is_some_and(|v| v.iter().any(|t| t == "pack_builder")));
    }

    #[test]
    fn non_pack_intent_uses_default_route() {
        let text = "what model am I using?";
        assert!(!is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert!(route.agent.is_none());
        assert!(route.tool_allowlist.is_none());
    }

    #[test]
    fn parses_pack_builder_confirm_cancel_and_connector_override() {
        assert!(matches!(
            parse_pack_builder_reply_command("confirm"),
            Some(PackBuilderReplyCommand::Confirm)
        ));
        assert!(matches!(
            parse_pack_builder_reply_command("ok"),
            Some(PackBuilderReplyCommand::Confirm)
        ));
        assert!(matches!(
            parse_pack_builder_reply_command("cancel"),
            Some(PackBuilderReplyCommand::Cancel)
        ));
        let parsed = parse_pack_builder_reply_command("use connectors: notion, slack");
        match parsed {
            Some(PackBuilderReplyCommand::UseConnectors(rows)) => {
                assert_eq!(rows, vec!["notion".to_string(), "slack".to_string()]);
            }
            _ => panic!("expected connector override parse"),
        }
    }

    // ── SessionRecord ─────────────────────────────────────────────────────

    #[test]
    fn session_record_roundtrip() {
        let record = SessionRecord {
            session_id: "s1".to_string(),
            created_at_ms: 1000,
            last_seen_at_ms: 2000,
            channel: "telegram".to_string(),
            sender: "user1".to_string(),
            scope_id: Some("chat:42".to_string()),
            scope_kind: Some("room".to_string()),
            tool_preferences: None,
        };
        let serialized = serde_json::to_string(&record).unwrap();
        let deserialized: SessionRecord = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.session_id, "s1");
        assert_eq!(deserialized.created_at_ms, 1000);
        assert_eq!(deserialized.last_seen_at_ms, 2000);
        assert_eq!(deserialized.channel, "telegram");
        assert_eq!(deserialized.sender, "user1");
    }

    fn test_channel_message(scope_id: &str) -> ChannelMessage {
        ChannelMessage {
            id: "m1".to_string(),
            sender: "user1".to_string(),
            reply_target: "room1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: scope_id.to_string(),
            },
        }
    }

    #[test]
    fn session_map_key_includes_scope() {
        let room_a = test_channel_message("channel:room-a");
        let room_b = test_channel_message("channel:room-b");
        assert_ne!(session_map_key(&room_a), session_map_key(&room_b));
    }

    #[test]
    fn channel_session_create_body_allows_memory_and_browser_tools() {
        let body = build_channel_session_create_body(
            "Channel Session",
            ChannelSecurityProfile::Operator,
            None,
        );
        let permissions = body
            .get("permission")
            .and_then(|value| value.as_array())
            .expect("permission array");

        for permission_name in ["memory_search", "memory_store", "memory_list"] {
            assert!(permissions.iter().any(|value| {
                value.get("permission").and_then(|row| row.as_str()) == Some(permission_name)
                    && value.get("action").and_then(|row| row.as_str()) == Some("allow")
            }));
        }

        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("mcp*")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));

        for permission_name in [
            "browser_status",
            "browser_open",
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_press",
            "browser_wait",
            "browser_extract",
            "browser_screenshot",
            "browser_close",
        ] {
            assert!(permissions.iter().any(|value| {
                value.get("permission").and_then(|row| row.as_str()) == Some(permission_name)
                    && value.get("action").and_then(|row| row.as_str()) == Some("allow")
            }));
        }
    }

    #[test]
    fn public_demo_session_create_body_disables_workspace_and_shell_access() {
        let body = build_channel_session_create_body(
            "Public Demo Session",
            ChannelSecurityProfile::PublicDemo,
            Some("channel-public::discord::room-a"),
        );
        let permissions = body
            .get("permission")
            .and_then(|value| value.as_array())
            .expect("permission array");

        assert!(body.get("directory").is_none());
        assert_eq!(
            body.get("project_id").and_then(|value| value.as_str()),
            Some("channel-public::discord::room-a")
        );
        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("websearch")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));
        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("memory_search")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));
        assert!(!permissions.iter().any(|value| {
            matches!(
                value.get("permission").and_then(|row| row.as_str()),
                Some("read" | "bash" | "browser_open" | "mcp*")
            )
        }));
    }

    #[test]
    fn public_demo_help_lists_disabled_commands_for_security() {
        let help = help_text(None, ChannelSecurityProfile::PublicDemo);
        assert!(help.contains("Disabled In This Public Channel For Security"));
        assert!(help.contains("/workspace"));
        assert!(help.contains("/memory"));
        assert!(help.contains("real Tandem capabilities"));
    }

    #[test]
    fn public_demo_memory_help_is_disabled() {
        let help = help_text(Some("memory"), ChannelSecurityProfile::PublicDemo);
        assert!(help.contains("Public Channel Memory Commands"));
        assert!(help.contains("quarantined"));
    }

    #[test]
    fn public_demo_allows_memory_commands() {
        let reason = blocked_command_reason(
            &SlashCommand::Memory {
                action: MemoryCommand::Help,
            },
            ChannelSecurityProfile::PublicDemo,
        );
        assert!(reason.is_none());
    }

    #[test]
    fn public_demo_tool_allowlist_cannot_be_widened_by_route_override() {
        let prefs = ChannelToolPreferences::default();
        let route_allowlist = vec!["pack_builder".to_string(), "websearch".to_string()];
        let result = build_channel_tool_allowlist(
            Some(&route_allowlist),
            &prefs,
            ChannelSecurityProfile::PublicDemo,
        )
        .expect("public demo allowlist");

        assert_eq!(result, vec!["websearch".to_string()]);
    }

    #[tokio::test]
    async fn active_session_id_migrates_legacy_key_to_scoped_key() {
        let msg = test_channel_message("channel:room-a");
        let legacy_key = legacy_session_map_key(&msg);
        let scoped_key = session_map_key(&msg);
        let mut map = std::collections::HashMap::new();
        map.insert(
            legacy_key,
            SessionRecord {
                session_id: "s-legacy".to_string(),
                created_at_ms: 1,
                last_seen_at_ms: 2,
                channel: msg.channel.clone(),
                sender: msg.sender.clone(),
                scope_id: None,
                scope_kind: None,
                tool_preferences: None,
            },
        );
        let session_map = std::sync::Arc::new(tokio::sync::Mutex::new(map));

        let active = active_session_id(&msg, &session_map).await;

        assert_eq!(active.as_deref(), Some("s-legacy"));
        let guard = session_map.lock().await;
        assert!(guard.get(&scoped_key).is_some());
        assert!(guard.get(&legacy_session_map_key(&msg)).is_none());
    }

    #[test]
    fn extracts_markdown_image_and_cleans_text() {
        let input = "Here is the render:\n![chart](https://cdn.example.com/chart.png)\nLooks good.";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://cdn.example.com/chart.png"]);
        assert!(text.contains("Here is the render:"));
        assert!(text.contains("Looks good."));
        assert!(!text.contains("![chart]"));
    }

    #[test]
    fn extracts_direct_image_url_token() {
        let input = "Generated image: https://example.com/out/final.webp";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://example.com/out/final.webp"]);
        assert!(text.contains("Generated image:"));
    }

    #[test]
    fn synthesize_attachment_prompt_includes_reference_when_present() {
        let out = synthesize_attachment_prompt(
            "telegram",
            "photo",
            "please analyze",
            Some("run/s1/channel_uploads/u1"),
            Some("/tmp/photo.jpg"),
            Some("https://example.com/photo.jpg"),
            Some("photo.jpg"),
            Some("image/jpeg"),
        );
        assert!(out.contains("Channel upload received"));
        assert!(out.contains("Stored upload reference"));
        assert!(out.contains("Stored local attachment path"));
        assert!(out.contains("please analyze"));
    }

    #[test]
    fn sanitize_resource_segment_replaces_invalid_chars() {
        assert_eq!(
            sanitize_resource_segment("abc/def:ghi"),
            "abc_def_ghi".to_string()
        );
    }

    #[test]
    fn zip_attachment_detection_handles_filename_path_and_url() {
        let mut msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "u1".to_string(),
            reply_target: "c1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: Some("pack.zip".to_string()),
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: "channel:c1".to_string(),
            },
        };
        assert!(is_zip_attachment(&msg));
        msg.attachment_filename = None;
        msg.attachment_path = Some("/tmp/upload.PACK.ZIP".to_string());
        assert!(is_zip_attachment(&msg));
        msg.attachment_path = None;
        msg.attachment_url = Some("https://example.com/x/y/pack.zip?sig=1".to_string());
        assert!(is_zip_attachment(&msg));
    }

    #[test]
    fn trusted_source_matching_supports_channel_room_sender_scopes() {
        let msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "userA".to_string(),
            reply_target: "room1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: "channel:room1".to_string(),
            },
        };
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1:usera".to_string()]
        ));
        assert!(!source_is_trusted_for_auto_install(
            &msg,
            &["slack".to_string()]
        ));
    }

    #[test]
    fn retries_empty_channel_event_stream_on_decode_error() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(should_retry_channel_event_stream(
            "error decoding response body",
            "",
            deadline
        ));
    }

    #[test]
    fn does_not_retry_channel_event_stream_after_content_arrives() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(!should_retry_channel_event_stream(
            "error decoding response body",
            "partial reply",
            deadline
        ));
    }
}
