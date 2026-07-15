// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SetupUnderstandRequest {
    pub surface: Option<String>,
    pub session_id: Option<String>,
    pub text: String,
    pub channel: Option<String>,
    #[serde(default)]
    pub trigger: SetupTriggerInfo,
    #[serde(default)]
    pub scope: SetupScopeInfo,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SetupTriggerInfo {
    pub source: Option<String>,
    pub is_direct_message: bool,
    pub was_explicitly_mentioned: bool,
    pub is_reply_to_bot: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SetupScopeInfo {
    pub kind: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupDecision {
    PassThrough,
    Intercept,
    Clarify,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupIntentKind {
    ProviderSetup,
    IntegrationSetup,
    AutomationCreate,
    WorkflowPlannerCreate,
    ChannelSetupHelp,
    SetupHelp,
    General,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SetupUnderstandSlots {
    #[serde(default)]
    pub provider_ids: Vec<String>,
    #[serde(default)]
    pub model_ids: Vec<String>,
    #[serde(default)]
    pub integration_targets: Vec<String>,
    #[serde(default)]
    pub channel_targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_target: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct SetupEvidence {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct SetupClarifierOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct SetupClarifier {
    pub question: String,
    pub options: Vec<SetupClarifierOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(super) struct SetupProposedAction {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct SetupUnderstandResponse {
    pub decision: SetupDecision,
    pub intent_kind: SetupIntentKind,
    pub confidence: f32,
    pub slots: SetupUnderstandSlots,
    #[serde(default)]
    pub evidence: Vec<SetupEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarifier: Option<SetupClarifier>,
    pub proposed_action: SetupProposedAction,
}

#[derive(Debug, Clone, Default)]
struct ProviderSetupState {
    default_provider: Option<String>,
    configured_ids: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct IntegrationSetupState {
    configured: HashSet<String>,
    connected: HashSet<String>,
    catalog: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default)]
struct SetupUnderstandingState {
    providers: ProviderSetupState,
    integrations: IntegrationSetupState,
}

#[derive(Debug, Clone, Default)]
struct IntentScore {
    score: i32,
    slots: SetupUnderstandSlots,
    evidence: Vec<SetupEvidence>,
}

const PROVIDER_ALIASES: &[(&str, &[&str])] = &[
    ("openai", &["openai", "gpt"]),
    ("openrouter", &["openrouter"]),
    ("anthropic", &["anthropic", "claude"]),
    ("ollama", &["ollama"]),
    ("groq", &["groq"]),
    ("mistral", &["mistral"]),
    ("together", &["together", "together ai"]),
    ("cohere", &["cohere"]),
    ("gemini", &["gemini", "google ai", "google"]),
];

const MODEL_ALIASES: &[(&str, &[&str])] = &[
    ("gpt-5", &["gpt-5", "gpt5"]),
    ("gpt-4.1", &["gpt-4.1", "gpt 4.1"]),
    (
        "claude-sonnet",
        &["sonnet", "claude sonnet", "3.7 sonnet", "4 sonnet"],
    ),
    ("claude-opus", &["opus", "claude opus"]),
    ("claude-haiku", &["haiku", "claude haiku"]),
    ("llama", &["llama", "llama 3", "llama3"]),
    ("gemini", &["gemini"]),
];

const INTEGRATION_ALIASES: &[(&str, &[&str])] = &[
    ("notion", &["notion"]),
    ("github", &["github"]),
    ("slack", &["slack"]),
    ("gmail", &["gmail", "google mail"]),
    ("calendar", &["calendar", "google calendar"]),
    ("stripe", &["stripe"]),
    ("linear", &["linear"]),
    ("airtable", &["airtable"]),
    ("jira", &["jira"]),
];

const CHANNEL_ALIASES: &[&str] = &["discord", "telegram", "slack"];
const SETUP_VERBS: &[&str] = &[
    "set up",
    "setup",
    "configure",
    "connect",
    "integrate",
    "link",
    "add",
    "switch",
    "use",
    "enable",
];
const AUTOMATION_VERBS: &[&str] = &[
    "monitor",
    "watch",
    "summarize",
    "digest",
    "post",
    "send",
    "report",
    "check every",
    "every morning",
    "every day",
    "daily",
    "weekly",
    "hourly",
    "when ",
];

pub(super) async fn setup_understand(
    State(state): State<AppState>,
    Json(request): Json<SetupUnderstandRequest>,
) -> Json<SetupUnderstandResponse> {
    let response = resolve_setup_request(&state, request).await;
    Json(response)
}

pub(super) async fn resolve_setup_request(
    state: &AppState,
    request: SetupUnderstandRequest,
) -> SetupUnderstandResponse {
    let setup_state = load_setup_understanding_state(state).await;
    let normalized = normalize_input_text(&request.text);
    if looks_like_plain_url_share(&normalized) {
        return pass_through_response();
    }
    let broad_setup = is_broad_setup_request(&normalized);
    let provider = score_provider_setup(&normalized, &setup_state.providers);
    let integration = score_integration_setup(&normalized, &setup_state.integrations);
    let automation = score_automation_create(&normalized, &setup_state.integrations);
    let workflow_planner = score_workflow_planner_create(&normalized);
    let channel_help = score_channel_setup_help(&normalized);
    let setup_help = score_setup_help(&normalized, broad_setup);

    let mut candidates = BTreeMap::new();
    candidates.insert(SetupIntentKind::ProviderSetup, provider);
    candidates.insert(SetupIntentKind::IntegrationSetup, integration);
    candidates.insert(SetupIntentKind::AutomationCreate, automation);
    candidates.insert(SetupIntentKind::WorkflowPlannerCreate, workflow_planner);
    candidates.insert(SetupIntentKind::ChannelSetupHelp, channel_help);
    candidates.insert(SetupIntentKind::SetupHelp, setup_help);

    let mut ranked = candidates
        .into_iter()
        .filter(|(_, score)| score.score > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.score.cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));

    let Some((top_kind, top_score)) = ranked.first().cloned() else {
        return pass_through_response();
    };
    let second_score = ranked
        .get(1)
        .map(|(_, score)| score.score)
        .unwrap_or_default();
    if top_kind == SetupIntentKind::WorkflowPlannerCreate
        && workflow_planner_needs_clarification(&normalized)
    {
        let prompt = top_score
            .slots
            .goal
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| normalized.trim().to_string());
        return clarify_response_with_action(
            SetupIntentKind::WorkflowPlannerCreate,
            top_score.clone(),
            &workflow_planner_clarifier_question(&normalized),
            workflow_planner_clarifier_options(&normalized),
            "workflow_plan_preview",
            json!({
                "prompt": prompt,
                "plan_source": request.surface.as_deref().unwrap_or("chat_setup"),
            }),
        );
    }

    let strong_automation = top_kind == SetupIntentKind::AutomationCreate
        && top_score.score >= 8
        && (top_score.slots.schedule_hint.is_some()
            || top_score.slots.delivery_target.is_some()
            || top_score.slots.integration_targets.len() >= 2);

    if broad_setup
        || (!strong_automation
            && top_score.score >= 4
            && second_score > 0
            && top_score.score - second_score <= 2)
    {
        return clarify_response(
            top_kind,
            top_score,
            "What are you trying to set up?",
            default_clarifier_options(),
        );
    }

    if top_score.score < 4 {
        return pass_through_response();
    }

    match top_kind {
        SetupIntentKind::ProviderSetup => intercept_response(
            SetupIntentKind::ProviderSetup,
            top_score.clone(),
            "open_provider_setup",
            json!({
                "provider_id": top_score.slots.provider_ids.first().cloned(),
                "model_id": top_score.slots.model_ids.first().cloned(),
            }),
        ),
        SetupIntentKind::IntegrationSetup => intercept_response(
            SetupIntentKind::IntegrationSetup,
            top_score.clone(),
            "open_mcp_setup",
            json!({
                "integration_target": top_score.slots.integration_targets.first().cloned(),
            }),
        ),
        SetupIntentKind::AutomationCreate => intercept_response(
            SetupIntentKind::AutomationCreate,
            top_score.clone(),
            "workflow_plan_preview",
            json!({
                "prompt": top_score.slots.goal.clone(),
                "schedule_hint": top_score.slots.schedule_hint.clone(),
                "delivery_target": top_score.slots.delivery_target.clone(),
                "integration_targets": top_score.slots.integration_targets.clone(),
                "plan_source": "chat_setup",
            }),
        ),
        SetupIntentKind::WorkflowPlannerCreate => intercept_response(
            SetupIntentKind::WorkflowPlannerCreate,
            top_score.clone(),
            "workflow_plan_preview",
            json!({
                "prompt": top_score.slots.goal.clone(),
                "plan_source": request.surface.as_deref().unwrap_or("chat_setup"),
            }),
        ),
        SetupIntentKind::ChannelSetupHelp => intercept_response(
            SetupIntentKind::ChannelSetupHelp,
            top_score.clone(),
            "show_setup_help",
            json!({
                "section": "channels",
                "channel_targets": top_score.slots.channel_targets.clone(),
            }),
        ),
        SetupIntentKind::SetupHelp => clarify_response(
            SetupIntentKind::SetupHelp,
            top_score,
            "What do you want Tandem to help you set up?",
            default_clarifier_options(),
        ),
        SetupIntentKind::General => pass_through_response(),
    }
}

async fn load_setup_understanding_state(state: &AppState) -> SetupUnderstandingState {
    let cfg = state.config.get_effective_value().await;
    let default_provider = cfg
        .get("default_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let configured_ids = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|row| row.id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let provider_state = ProviderSetupState {
        default_provider,
        configured_ids,
    };

    let mcp_servers = state.mcp.list().await;
    let mut configured = HashSet::new();
    let mut connected = HashSet::new();
    for (name, server) in mcp_servers {
        configured.insert(name.to_ascii_lowercase());
        if server.connected {
            connected.insert(name.to_ascii_lowercase());
        }
    }

    let mut catalog = HashMap::new();
    if let Some(index) = mcp_catalog::index() {
        if let Some(servers) = index.get("servers").and_then(Value::as_array) {
            for server in servers {
                let slug = server
                    .get("slug")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if slug.is_empty() {
                    continue;
                }
                let mut aliases = vec![slug.clone()];
                if let Some(name) = server.get("name").and_then(Value::as_str) {
                    aliases.push(name.to_ascii_lowercase());
                }
                catalog.insert(slug, aliases);
            }
        }
    }

    SetupUnderstandingState {
        providers: provider_state,
        integrations: IntegrationSetupState {
            configured,
            connected,
            catalog,
        },
    }
}

fn normalize_input_text(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

fn contains_any(input: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| input.contains(phrase))
}

fn matched_aliases(input: &str, aliases: &[(&str, &[&str])]) -> Vec<String> {
    let mut out = Vec::new();
    for (canonical, words) in aliases {
        if words.iter().any(|word| input.contains(word)) {
            out.push((*canonical).to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn is_informational_only(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.starts_with("what is ")
        || trimmed.starts_with("what are ")
        || trimmed.starts_with("tell me about ")
        || trimmed.starts_with("explain ")
}

fn is_comparison_or_evaluation_request(input: &str) -> bool {
    contains_any(
        input,
        &[
            "compare",
            "compared",
            "comparison",
            "versus",
            " vs ",
            "difference",
            "better",
            "best",
            "rank",
            "recommend",
            "which is better",
            "how does",
            "how do they compare",
            "what's the difference",
            "what is the difference",
        ],
    )
}

fn is_broad_setup_request(input: &str) -> bool {
    (contains_any(
        input,
        &[
            "set up tandem",
            "setup tandem",
            "configure tandem",
            "my workflow",
        ],
    ) || contains_any(input, &["set me up", "make this work"]))
        && !contains_any(input, AUTOMATION_VERBS)
}

fn contains_url(input: &str) -> bool {
    input.contains("http://") || input.contains("https://") || input.contains("www.")
}

fn looks_like_plain_url_share(input: &str) -> bool {
    contains_url(input)
        && !contains_any(input, SETUP_VERBS)
        && !contains_any(input, AUTOMATION_VERBS)
        && !contains_any(
            input,
            &[
                "integration",
                "tool",
                "mcp",
                "connector",
                "provider",
                "model",
                "api key",
                "channel",
                "automation",
            ],
        )
}

fn score_provider_setup(input: &str, state: &ProviderSetupState) -> IntentScore {
    let mut score = IntentScore::default();
    if is_informational_only(input) {
        return score;
    }
    let providers = matched_aliases(input, PROVIDER_ALIASES);
    let models = matched_aliases(input, MODEL_ALIASES);
    let has_setup = contains_any(input, SETUP_VERBS);
    let has_provider_words = contains_any(input, &["provider", "model", "api key"]);

    if !has_setup && is_comparison_or_evaluation_request(input) {
        return score;
    }

    if has_setup {
        score.score += 2;
        score.evidence.push(SetupEvidence {
            kind: "keyword".to_string(),
            value: "setup_verb".to_string(),
        });
    }
    if has_provider_words {
        score.score += 1;
    }
    if !providers.is_empty() {
        score.score += 3;
        score.slots.provider_ids = providers.clone();
        for provider in &providers {
            score.evidence.push(SetupEvidence {
                kind: "entity".to_string(),
                value: provider.clone(),
            });
            if !state.configured_ids.contains(provider) {
                score.score += 3;
                score.evidence.push(SetupEvidence {
                    kind: "state_gap".to_string(),
                    value: format!("provider_missing:{provider}"),
                });
            }
        }
    }
    if !models.is_empty() {
        score.score += 2;
        score.slots.model_ids = models.clone();
        for model in models {
            score.evidence.push(SetupEvidence {
                kind: "entity".to_string(),
                value: model,
            });
        }
    }
    if !providers.is_empty() && !score.slots.model_ids.is_empty() {
        score.score += 4;
    }
    if state.default_provider.is_none() {
        score.score += 2;
        score.evidence.push(SetupEvidence {
            kind: "state_gap".to_string(),
            value: "default_provider_missing".to_string(),
        });
    }
    if providers.is_empty() && !has_provider_words {
        score.score = 0;
    }
    score
}

fn score_integration_setup(input: &str, state: &IntegrationSetupState) -> IntentScore {
    let mut score = IntentScore::default();
    if is_informational_only(input) {
        return score;
    }
    let integrations = matched_aliases(input, INTEGRATION_ALIASES);
    let has_setup = contains_any(input, SETUP_VERBS);
    let has_integration_words = contains_any(input, &["integration", "tool", "mcp", "connector"]);
    if has_setup {
        score.score += 2;
    }
    if has_integration_words {
        score.score += 1;
    }
    if !integrations.is_empty() {
        score.score += 3;
        score.slots.integration_targets = integrations.clone();
        for target in &integrations {
            score.evidence.push(SetupEvidence {
                kind: "entity".to_string(),
                value: target.clone(),
            });
            if state.catalog.contains_key(target) {
                score.score += 4;
                score.evidence.push(SetupEvidence {
                    kind: "pattern".to_string(),
                    value: format!("catalog_match:{target}"),
                });
            }
            if !state.configured.contains(target) {
                score.score += 3;
                score.evidence.push(SetupEvidence {
                    kind: "state_gap".to_string(),
                    value: format!("integration_missing:{target}"),
                });
            } else if !state.connected.contains(target) {
                score.score += 2;
                score.evidence.push(SetupEvidence {
                    kind: "state_gap".to_string(),
                    value: format!("integration_disconnected:{target}"),
                });
            }
        }
    }
    if contains_any(
        input,
        &[
            "workflow plan",
            "workflow draft",
            "draft a plan",
            "draft the plan",
            "turn this into a plan",
            "build a plan",
            "make a plan",
        ],
    ) {
        score.score -= 8;
    }
    if contains_any(input, AUTOMATION_VERBS)
        && (extract_schedule_hint(input).is_some() || integrations.len() >= 2)
    {
        score.score -= 8;
    }
    if has_workflow_authoring_signal(input) {
        score.score -= 10;
    }
    if integrations.is_empty() && !has_integration_words {
        score.score = 0;
    }
    score
}

fn score_automation_create(input: &str, state: &IntegrationSetupState) -> IntentScore {
    let mut score = IntentScore::default();
    if is_informational_only(input) {
        return score;
    }
    let integrations = matched_aliases(input, INTEGRATION_ALIASES);
    let has_automation = contains_any(input, AUTOMATION_VERBS);
    let schedule = extract_schedule_hint(input);
    if has_automation {
        score.score += 4;
        score.evidence.push(SetupEvidence {
            kind: "keyword".to_string(),
            value: "automation_verb".to_string(),
        });
    }
    if let Some(schedule_hint) = schedule.clone() {
        score.score += 4;
        score.slots.schedule_hint = Some(schedule_hint.clone());
        score.evidence.push(SetupEvidence {
            kind: "pattern".to_string(),
            value: schedule_hint,
        });
    }
    if integrations.len() >= 2 {
        score.score += 4;
    }
    if !integrations.is_empty() {
        score.slots.integration_targets = integrations.clone();
        for target in integrations {
            if state.catalog.contains_key(&target) {
                score.evidence.push(SetupEvidence {
                    kind: "entity".to_string(),
                    value: target.clone(),
                });
            }
        }
    }
    score.slots.goal = Some(input.trim().to_string());
    score.slots.delivery_target = extract_delivery_target(input);
    if score.slots.schedule_hint.is_some() && score.slots.delivery_target.is_some() {
        score.score += 6;
    }
    let has_workflow_shape = score.slots.schedule_hint.is_some()
        || score.slots.delivery_target.is_some()
        || score.slots.integration_targets.len() >= 2
        || input.contains("alert")
        || input.contains("notify");
    if !has_automation || !has_workflow_shape {
        score.score = 0;
    }
    score
}

fn score_workflow_planner_create(input: &str) -> IntentScore {
    let mut score = IntentScore::default();
    if is_informational_only(input) {
        return score;
    }

    let has_workflow_words = contains_any(
        input,
        &[
            "workflow",
            "workflows",
            "pipeline",
            "handoff",
            "handoffs",
            "orchestrate",
            "orchestration",
            "roadmap",
            "road map",
            "timeline",
            "milestone",
            "decompose",
            "break down",
            "multi-step",
            "long-horizon",
            "long term",
        ],
    );
    let has_plan_words = contains_any(
        input,
        &[
            "plan",
            "planner",
            "draft",
            "design",
            "sequence",
            "structure",
            "blueprint",
        ],
    );
    let has_creation_words = contains_any(input, &["create", "build", "make", "draft", "schedule"]);
    let has_planning_phrase = contains_any(
        input,
        &[
            "workflow plan",
            "workflow draft",
            "plan this workflow",
            "draft a plan",
            "draft the plan",
            "turn this into a plan",
            "build a plan",
            "make a plan",
        ],
    );
    let has_workflow_authoring = has_workflow_authoring_signal(input);

    if has_workflow_words {
        score.score += 2;
        score.evidence.push(SetupEvidence {
            kind: "keyword".to_string(),
            value: "workflow_words".to_string(),
        });
    }
    if has_plan_words {
        score.score += 1;
        score.evidence.push(SetupEvidence {
            kind: "keyword".to_string(),
            value: "plan_words".to_string(),
        });
    }
    if has_creation_words {
        score.score += 2;
        score.evidence.push(SetupEvidence {
            kind: "keyword".to_string(),
            value: "creation_words".to_string(),
        });
    }
    if has_planning_phrase {
        score.score += 10;
        score.evidence.push(SetupEvidence {
            kind: "pattern".to_string(),
            value: "workflow_planning_phrase".to_string(),
        });
    }
    if has_workflow_words && has_plan_words {
        score.score += 2;
    }
    if has_workflow_authoring {
        score.score += 8;
        score.evidence.push(SetupEvidence {
            kind: "pattern".to_string(),
            value: "workflow_authoring_signal".to_string(),
        });
    }
    if contains_any(input, SETUP_VERBS) {
        score.score += 1;
    }
    if contains_any(input, AUTOMATION_VERBS)
        && (extract_schedule_hint(input).is_some() || extract_delivery_target(input).is_some())
        && !has_workflow_authoring
    {
        score.score -= 5;
    }
    if contains_any(input, &["automation", "automate", "automations"]) && !has_planning_phrase {
        score.score -= 2;
    }

    if score.score >= 4 {
        score.slots.goal = Some(input.trim().to_string());
        score.slots.schedule_hint = extract_schedule_hint(input);
        score.slots.delivery_target = extract_delivery_target(input);
        score.slots.integration_targets = matched_aliases(input, INTEGRATION_ALIASES);
    } else {
        score.score = 0;
    }
    score
}

fn has_workflow_authoring_signal(input: &str) -> bool {
    contains_any(input, &["workflow", "workflows"])
        && contains_any(
            input,
            &[
                "create", "build", "make", "draft", "design", "plan", "schedule", "every ", "when ",
            ],
        )
}

#[derive(Debug, Default)]
struct WorkflowPlannerClarifyProfile {
    has_schedule: bool,
    has_source_hint: bool,
    has_output_hint: bool,
    has_tools_hint: bool,
    has_approval_hint: bool,
    has_memory_or_files_hint: bool,
}

fn workflow_planner_clarify_profile(input: &str) -> WorkflowPlannerClarifyProfile {
    let lowered = input.to_ascii_lowercase();
    WorkflowPlannerClarifyProfile {
        has_schedule: extract_schedule_hint(&lowered).is_some()
            || contains_any(
                &lowered,
                &[
                    "daily",
                    "weekly",
                    "hourly",
                    "every morning",
                    "every day",
                    "every week",
                    "every hour",
                ],
            ),
        has_source_hint: contains_any(
            &lowered,
            &[
                "reddit",
                "github",
                "slack",
                "discord",
                "telegram",
                "notion",
                "docs",
                "document",
                "files",
                "file",
                "workspace",
                "memory",
                "email",
                "gmail",
                "calendar",
                "jira",
                "linear",
                "airtable",
                "rss",
                "web",
            ],
        ),
        has_output_hint: contains_any(
            &lowered,
            &[
                "report",
                "summary",
                "digest",
                "draft",
                "document",
                "dashboard",
                "ticket",
                "task",
                "save",
                "write",
                "publish",
                "post",
                "send",
            ],
        ),
        has_tools_hint: contains_any(
            &lowered,
            &[
                "mcp",
                "tool",
                "tools",
                "integration",
                "integrations",
                "server",
            ],
        ),
        has_approval_hint: contains_any(
            &lowered,
            &[
                "approval",
                "approve",
                "human review",
                "review first",
                "before sending",
            ],
        ),
        has_memory_or_files_hint: contains_any(
            &lowered,
            &["memory", "files", "file", "workspace", "document", "docs"],
        ),
    }
}

fn workflow_planner_clarifier_topics(input: &str) -> Vec<String> {
    let profile = workflow_planner_clarify_profile(input);
    let mut topics = Vec::new();
    if !profile.has_schedule {
        topics.push("trigger or schedule".to_string());
    }
    if !profile.has_source_hint {
        topics.push("input sources".to_string());
    }
    if !profile.has_output_hint {
        topics.push("output destination".to_string());
    }
    if !profile.has_tools_hint {
        topics.push("MCP/tools".to_string());
    }
    if !profile.has_approval_hint {
        topics.push("approval".to_string());
    }
    if !profile.has_memory_or_files_hint {
        topics.push("memory/files".to_string());
    }
    topics
}

fn workflow_planner_option_label(topic: &str) -> String {
    let mut chars = topic.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => topic.to_string(),
    }
}

fn workflow_planner_clarifier_options(input: &str) -> Vec<SetupClarifierOption> {
    let mut options = Vec::new();
    let mut seen = HashSet::new();
    for topic in workflow_planner_clarifier_topics(input).into_iter().take(3) {
        let id = format!(
            "workflow_planner_{}",
            topic
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .trim_matches('_')
        );
        if !seen.insert(id.clone()) {
            continue;
        }
        options.push(SetupClarifierOption {
            id,
            label: workflow_planner_option_label(&topic),
        });
    }
    if options.is_empty() {
        options.push(SetupClarifierOption {
            id: "workflow_planner_create".to_string(),
            label: "Draft the workflow".to_string(),
        });
    }
    options
}

fn workflow_planner_clarifier_question(input: &str) -> String {
    let topics = workflow_planner_clarifier_topics(input);
    let core = topics
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let mut question = if core.is_empty() {
        "What should the workflow do?".to_string()
    } else {
        format!("I need a few details before I draft this workflow: {core}.")
    };
    if topics
        .iter()
        .any(|topic| matches!(topic.as_str(), "MCP/tools" | "approval" | "memory/files"))
    {
        question
            .push_str(" If it needs MCP tools, approval, or file/memory access, mention that too.");
    }
    question
}

fn workflow_planner_needs_clarification(input: &str) -> bool {
    let profile = workflow_planner_clarify_profile(input);
    let core_signals = usize::from(profile.has_schedule)
        + usize::from(profile.has_source_hint)
        + usize::from(profile.has_output_hint);
    core_signals < 2
}

fn score_channel_setup_help(input: &str) -> IntentScore {
    let mut score = IntentScore::default();
    if !contains_any(
        input,
        &["how do i", "setup", "set up", "configure", "connect"],
    ) {
        return score;
    }
    let mut channels = Vec::new();
    for alias in CHANNEL_ALIASES {
        if input.contains(alias) {
            channels.push((*alias).to_string());
        }
    }
    if channels.is_empty() {
        return score;
    }
    score.score += 5;
    score.slots.channel_targets = channels;
    score
}

fn score_setup_help(input: &str, broad_setup: bool) -> IntentScore {
    let mut score = IntentScore::default();
    if broad_setup {
        score.score = 5;
    } else if contains_any(
        input,
        &[
            "how do i set up",
            "how do i configure",
            "what should i set up",
        ],
    ) {
        score.score = 4;
    }
    score
}

fn extract_schedule_hint(input: &str) -> Option<String> {
    if let Some(start) = input.find("every ") {
        let tail = &input[start..];
        let words = tail.split_whitespace().take(3).collect::<Vec<_>>();
        if words.len() >= 3
            && words[0] == "every"
            && words[1].chars().all(|ch| ch.is_ascii_digit())
            && matches!(
                words[2].trim_matches(|ch: char| !ch.is_ascii_alphabetic()),
                "minute" | "minutes" | "hour" | "hours" | "day" | "days"
            )
        {
            return Some(words.join(" "));
        }
    }
    for phrase in [
        "every morning",
        "every day",
        "daily",
        "weekly",
        "every hour",
        "hourly",
        "every sunday",
        "when ",
    ] {
        if input.contains(phrase) {
            return Some(phrase.trim().to_string());
        }
    }
    None
}

fn extract_delivery_target(input: &str) -> Option<String> {
    for target in ["slack", "email", "gmail", "telegram", "discord"] {
        if input.contains(target) && (input.contains("send") || input.contains("post")) {
            return Some(target.to_string());
        }
    }
    None
}

fn confidence_for(score: i32) -> f32 {
    ((score as f32) / 12.0).clamp(0.0, 1.0)
}

fn pass_through_response() -> SetupUnderstandResponse {
    SetupUnderstandResponse {
        decision: SetupDecision::PassThrough,
        intent_kind: SetupIntentKind::General,
        confidence: 0.0,
        slots: SetupUnderstandSlots::default(),
        evidence: Vec::new(),
        clarifier: None,
        proposed_action: SetupProposedAction {
            action_type: "pass_through".to_string(),
            payload: json!({}),
        },
    }
}

fn intercept_response(
    intent_kind: SetupIntentKind,
    score: IntentScore,
    action_type: &str,
    payload: Value,
) -> SetupUnderstandResponse {
    SetupUnderstandResponse {
        decision: SetupDecision::Intercept,
        intent_kind,
        confidence: confidence_for(score.score),
        slots: score.slots,
        evidence: score.evidence,
        clarifier: None,
        proposed_action: SetupProposedAction {
            action_type: action_type.to_string(),
            payload,
        },
    }
}

fn clarify_response(
    intent_kind: SetupIntentKind,
    score: IntentScore,
    question: &str,
    options: Vec<SetupClarifierOption>,
) -> SetupUnderstandResponse {
    clarify_response_with_action(
        intent_kind,
        score,
        question,
        options,
        "show_setup_help",
        json!({}),
    )
}

fn clarify_response_with_action(
    intent_kind: SetupIntentKind,
    score: IntentScore,
    question: &str,
    options: Vec<SetupClarifierOption>,
    action_type: &str,
    payload: Value,
) -> SetupUnderstandResponse {
    SetupUnderstandResponse {
        decision: SetupDecision::Clarify,
        intent_kind,
        confidence: confidence_for(score.score),
        slots: score.slots,
        evidence: score.evidence,
        clarifier: Some(SetupClarifier {
            question: question.to_string(),
            options,
        }),
        proposed_action: SetupProposedAction {
            action_type: action_type.to_string(),
            payload,
        },
    }
}

fn default_clarifier_options() -> Vec<SetupClarifierOption> {
    vec![
        SetupClarifierOption {
            id: "provider_setup".to_string(),
            label: "Set up a provider".to_string(),
        },
        SetupClarifierOption {
            id: "integration_setup".to_string(),
            label: "Connect tools".to_string(),
        },
        SetupClarifierOption {
            id: "automation_create".to_string(),
            label: "Create an automation".to_string(),
        },
        SetupClarifierOption {
            id: "workflow_planner_create".to_string(),
            label: "Draft a workflow plan".to_string(),
        },
    ]
}
