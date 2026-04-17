// Session dispatcher - routes incoming channel messages to Tandem sessions.
//
// Each unique `{channel_name}:{sender_id}` pair maps to one persistent Tandem
// session. The mapping is durably persisted under Tandem's app-data state dir
// (for example `~/.local/share/tandem/data/channel_sessions.json` on Linux)
// and reloaded on startup.
//
// ## API paths (tandem-server)
//
// | Action         | Path                                 |
// |----------------|--------------------------------------|
// | Create session | `POST /session`                      |
// | List sessions  | `GET  /session`                      |
// | Get session    | `GET  /session/{id}`                 |
// | Update session | `PUT  /session/{id}`                 |
// | Prompt (sync)  | `POST /session/{id}/prompt_sync`     |
//
// ## Slash commands
//
// `/new [name]`, `/sessions`, `/resume <query>`, `/rename <name>`,
// `/status`, `/run`, `/cancel`, `/todos`, `/requests`, `/answer <id> <text>`,
// `/providers`, `/models [provider]`, `/model <model_id>`, `/approve <tool_call_id>`,
// `/deny <tool_call_id>`, `/schedule ...`, `/help [topic]`

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tracing::{error, info, warn};

use crate::channel_registry::{
    command_capability, registered_channels, slash_command_capabilities, ChannelCommandCapability,
    ChannelRuntimeDiagnostics,
};
use crate::config::{ChannelSecurityProfile, ChannelsConfig};
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
    let scoped_key = format!("{}:{}", channel, scope_id);
    let channel_prefs = map.get(channel).cloned().unwrap_or_default();
    let Some(scoped_prefs) = map.get(&scoped_key).cloned() else {
        return channel_prefs;
    };
    merge_channel_tool_preferences(channel_prefs, scoped_prefs)
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
    let diagnostics = crate::channel_registry::new_channel_runtime_diagnostics();
    start_channel_listeners_with_diagnostics(config, diagnostics).await
}

pub async fn start_channel_listeners_with_diagnostics(
    config: ChannelsConfig,
    diagnostics: ChannelRuntimeDiagnostics,
) -> JoinSet<()> {
    let initial_map = load_session_map().await;
    info!(
        "tandem-channels: loaded {} persisted session mappings",
        initial_map.len()
    );

    let session_map: SessionMap = Arc::new(Mutex::new(initial_map));
    let mut security_profiles = HashMap::new();
    let mut set = JoinSet::new();

    let mut registry_seen = HashSet::new();
    for spec in registered_channels() {
        if !registry_seen.insert(spec.name) {
            error!("channel registry has duplicate entry for {}", spec.name);
            continue;
        }
        let Some(channel) = (spec.constructor)(&config) else {
            continue;
        };
        security_profiles.insert(
            spec.name.to_string(),
            (spec.security_profile)(&config).unwrap_or_default(),
        );
        let map = session_map.clone();
        let base_url = config.server_base_url.clone();
        let api_token = config.api_token.clone();
        let profiles = Arc::new(security_profiles.clone());
        let channel_name = spec.name.to_string();
        info!("tandem-channels: {} listener started", spec.status_label);
        set.spawn(supervise(
            channel,
            base_url,
            api_token,
            map,
            profiles,
            diagnostics.clone(),
            channel_name,
        ));
    }

    set
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Runs a channel listener with exponential-backoff restart on failure.
async fn set_channel_diagnostic_state(
    diagnostics: &ChannelRuntimeDiagnostics,
    channel_name: &str,
    update: impl FnOnce(&mut crate::channel_registry::ChannelRuntimeDiagnostic),
) {
    let mut diagnostics = diagnostics.write().await;
    let entry = diagnostics
        .entry(channel_name.to_string())
        .or_insert_with(Default::default);
    update(entry);
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn supervise(
    channel: Arc<dyn Channel>,
    base_url: String,
    api_token: String,
    session_map: SessionMap,
    security_profiles: ChannelSecurityMap,
    diagnostics: ChannelRuntimeDiagnostics,
    channel_name: String,
) {
    let mut backoff_secs: u64 = 1;
    set_channel_diagnostic_state(&diagnostics, &channel_name, |entry| {
        entry.state = "stopped";
        entry.last_error = None;
        entry.last_error_code = None;
    })
    .await;
    loop {
        let (tx, mut rx) = mpsc::channel::<ChannelMessage>(64);
        set_channel_diagnostic_state(&diagnostics, &channel_name, |entry| {
            entry.state = "starting";
            entry.listener_start_count = entry.listener_start_count.saturating_add(1);
            entry.last_error = None;
            entry.last_error_code = None;
            entry.last_reconnect_at = Some(now_unix_ms());
        })
        .await;

        let channel_listen = channel.clone();
        let diagnostics_for_listener = diagnostics.clone();
        let channel_name_for_listener = channel_name.clone();
        let listen_handle = tokio::spawn(async move {
            let result = channel_listen.listen(tx).await;
            let code = if result.is_ok() {
                None
            } else {
                Some("listener_error")
            };
            let error_message = result.err().map(|error| error.to_string());
            set_channel_diagnostic_state(
                &diagnostics_for_listener,
                &channel_name_for_listener,
                |entry| {
                    entry.last_error = error_message.clone();
                    entry.last_error_code = code;
                    if error_message.is_some() {
                        entry.state = "stopped";
                    }
                },
            )
            .await;
        });
        set_channel_diagnostic_state(&diagnostics, &channel_name, |entry| {
            entry.state = "running";
        })
        .await;

        while let Some(msg) = rx.recv().await {
            let ch = channel.clone();
            let base = base_url.clone();
            let tok = api_token.clone();
            let map = session_map.clone();
            let profiles = security_profiles.clone();
            tokio::spawn(async move {
                process_channel_message(msg, ch, &base, &tok, &map, &profiles).await;
            });
        }

        listen_handle.abort();

        let listen_ok = if let Ok(_) = listen_handle.await {
            true
        } else {
            set_channel_diagnostic_state(&diagnostics, &channel_name, |entry| {
                entry.state = "stopped";
                entry.last_error = Some("listener task panicked".to_string());
                entry.last_error_code = Some("listener_panic");
            })
            .await;
            false
        };

        if listen_ok && channel.health_check().await {
            backoff_secs = 1;
        } else {
            set_channel_diagnostic_state(&diagnostics, &channel_name, |entry| {
                entry.state = "retrying";
                entry.last_error = Some(
                    "listener exited and health check failed; attempting reconnect".to_string(),
                );
                entry.last_error_code = Some("startup_error");
            })
            .await;
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
    let mut prompt_content = msg.content.clone();
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
            &msg.content,
            persisted.as_deref(),
            msg.attachment_path.as_deref(),
            msg.attachment_url.as_deref(),
            msg.attachment_filename.as_deref(),
            msg.attachment_mime.as_deref(),
        );
    }

    let route = route_agent_for_channel_message(&msg.content);
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
        &msg.channel,
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
