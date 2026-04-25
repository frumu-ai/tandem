use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    AppState, AutomationAgentMcpPolicy, AutomationAgentProfile, AutomationAgentToolPolicy,
    AutomationExecutionPolicy, AutomationFlowNode, AutomationFlowOutputContract,
    AutomationFlowSpec, AutomationOutputValidatorKind, AutomationV2Schedule,
    AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status, RoutineMisfirePolicy,
};

const CHANNEL_DRAFT_TTL_MS: u64 = 10 * 60 * 1000;
const CHANNEL_WORKFLOW_DRAFTING_DISABLED_MESSAGE: &str =
    "Workflow drafting is disabled for this channel. Enable the workflow planner gate in Settings to continue.";
const STRICT_KB_FACTUAL_DRAFT_BLOCKED_MESSAGE: &str =
    "I do not see that in the connected knowledgebase.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelAutomationDraftStatus {
    Collecting,
    PreviewReady,
    Applied,
    Cancelled,
    Expired,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelAutomationDraftChannelContext {
    #[serde(default)]
    pub source_platform: String,
    #[serde(default)]
    pub scope_kind: String,
    #[serde(default)]
    pub scope_id: String,
    #[serde(default)]
    pub reply_target: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAutomationDraftQuestion {
    pub field: String,
    pub text: String,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAutomationDraftPreview {
    pub summary: String,
    pub goal: String,
    pub schedule_hint: String,
    pub delivery_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAutomationDraftRecord {
    pub draft_id: String,
    pub status: ChannelAutomationDraftStatus,
    pub original_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_target: Option<String>,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<ChannelAutomationDraftQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<ChannelAutomationDraftPreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_planner_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_kb_grounding: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factual_question: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_workflow_intent: Option<bool>,
    pub channel_context: ChannelAutomationDraftChannelContext,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ChannelAutomationDraftStartRequest {
    pub text: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub thread_key: Option<String>,
    #[serde(default)]
    pub channel_context: ChannelAutomationDraftChannelContext,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_tools: Vec<String>,
    #[serde(default)]
    pub security_profile: Option<String>,
    #[serde(default)]
    pub workflow_planner_enabled: Option<bool>,
    #[serde(default)]
    pub strict_kb_grounding: Option<bool>,
    #[serde(default)]
    pub factual_question: Option<bool>,
    #[serde(default)]
    pub explicit_workflow_intent: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelAutomationDraftAnswerRequest {
    pub answer: String,
    #[serde(default)]
    pub workflow_planner_enabled: Option<bool>,
    #[serde(default)]
    pub strict_kb_grounding: Option<bool>,
    #[serde(default)]
    pub factual_question: Option<bool>,
    #[serde(default)]
    pub explicit_workflow_intent: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ChannelAutomationDraftPendingQuery {
    pub channel: Option<String>,
    pub scope_id: Option<String>,
    pub sender: Option<String>,
}

impl AppState {
    pub async fn load_channel_automation_drafts(&self) -> anyhow::Result<()> {
        if !self.channel_automation_drafts_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&self.channel_automation_drafts_path).await?;
        let parsed = serde_json::from_str::<HashMap<String, ChannelAutomationDraftRecord>>(&raw)
            .unwrap_or_default();
        *self.channel_automation_drafts.write().await = parsed;
        Ok(())
    }

    pub async fn persist_channel_automation_drafts(&self) -> anyhow::Result<()> {
        let payload = {
            let guard = self.channel_automation_drafts.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        if let Some(parent) = self.channel_automation_drafts_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.channel_automation_drafts_path, payload).await?;
        Ok(())
    }
}

pub(super) async fn channel_automation_drafts_start(
    State(state): State<AppState>,
    Json(input): Json<ChannelAutomationDraftStartRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let text = input.text.trim().to_string();
    if text.is_empty() {
        return Err(bad_request("draft text is required"));
    }

    let mut context = input.channel_context;
    if context.session_id.is_none() {
        context.session_id = input.session_id;
    }
    if context.thread_key.is_none() {
        context.thread_key = input.thread_key;
    }
    let strict_kb_grounding = input
        .strict_kb_grounding
        .unwrap_or(channel_strict_kb_grounding(&state, &context.source_platform).await);
    let factual_question = input
        .factual_question
        .unwrap_or_else(|| channel_draft_message_is_factual_question(&text));
    let explicit_workflow_intent = input
        .explicit_workflow_intent
        .unwrap_or_else(|| channel_draft_message_has_explicit_workflow_intent(&text));
    let mut draft = ChannelAutomationDraftRecord {
        draft_id: format!("channel-draft-{}", Uuid::new_v4()),
        status: ChannelAutomationDraftStatus::Collecting,
        original_text: text.clone(),
        goal: infer_goal(&text),
        schedule_hint: infer_schedule_hint(&text),
        delivery_target: Some(infer_delivery_target(&text)),
        missing_fields: Vec::new(),
        question: None,
        preview: None,
        automation_id: None,
        allowed_tools: normalize_list(input.allowed_tools),
        allowed_mcp_servers: normalize_list(input.allowed_mcp_servers),
        allowed_mcp_tools: normalize_list(input.allowed_mcp_tools),
        security_profile: input.security_profile,
        workflow_planner_enabled: input.workflow_planner_enabled,
        strict_kb_grounding: Some(strict_kb_grounding),
        factual_question: Some(factual_question),
        explicit_workflow_intent: Some(explicit_workflow_intent),
        channel_context: context,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: now.saturating_add(CHANNEL_DRAFT_TTL_MS),
    };
    if let Some(payload) = producer_guarded_response(
        &state,
        &mut draft,
        None,
        ProducerCaller::Start,
        strict_kb_grounding,
        factual_question,
        explicit_workflow_intent,
    )
    .await
    {
        return Ok(Json(payload));
    }
    advance_draft(&mut draft, now);
    state
        .channel_automation_drafts
        .write()
        .await
        .insert(draft.draft_id.clone(), draft.clone());
    persist_channel_drafts_or_log(&state).await;
    Ok(Json(draft_response(&draft)))
}

pub(super) async fn channel_automation_drafts_answer(
    State(state): State<AppState>,
    Path(draft_id): Path<String>,
    Json(input): Json<ChannelAutomationDraftAnswerRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let mut draft = get_mutable_draft(&state, &draft_id).await?;
    ensure_open_draft(&draft, now)?;
    let answer = input.answer.trim().to_string();
    if answer.is_empty() {
        return Err(bad_request("answer is required"));
    }
    let strict_kb_grounding = input
        .strict_kb_grounding
        .or(draft.strict_kb_grounding)
        .unwrap_or(
            channel_strict_kb_grounding(&state, &draft.channel_context.source_platform).await,
        );
    let factual_question = input
        .factual_question
        .unwrap_or_else(|| channel_draft_message_is_factual_question(&answer));
    let explicit_workflow_intent = input
        .explicit_workflow_intent
        .unwrap_or_else(|| channel_draft_message_has_explicit_workflow_intent(&answer));
    draft.workflow_planner_enabled = input
        .workflow_planner_enabled
        .or(draft.workflow_planner_enabled);
    draft.strict_kb_grounding = Some(strict_kb_grounding);
    draft.factual_question = Some(factual_question);
    draft.explicit_workflow_intent = Some(explicit_workflow_intent);
    if let Some(payload) = producer_guarded_response(
        &state,
        &mut draft,
        Some(draft_id.as_str()),
        ProducerCaller::Answer,
        strict_kb_grounding,
        factual_question,
        explicit_workflow_intent,
    )
    .await
    {
        return Ok(Json(payload));
    }
    if is_cancel_text(&answer) {
        draft.status = ChannelAutomationDraftStatus::Cancelled;
        draft.question = None;
        draft.updated_at_ms = now;
        store_draft(&state, draft.clone()).await;
        return Ok(Json(draft_response(&draft)));
    }
    match draft
        .question
        .as_ref()
        .map(|question| question.field.as_str())
    {
        Some("goal") => {
            if draft.schedule_hint.is_none() {
                draft.schedule_hint = infer_schedule_hint(&answer);
            }
            draft.goal = Some(answer);
        }
        Some("schedule_hint") => draft.schedule_hint = Some(answer),
        Some("delivery_target") => draft.delivery_target = Some(answer),
        _ if draft.status == ChannelAutomationDraftStatus::PreviewReady => {}
        _ => draft.goal = Some(answer),
    }
    advance_draft(&mut draft, now);
    store_draft(&state, draft.clone()).await;
    Ok(Json(draft_response(&draft)))
}

pub(super) async fn channel_automation_drafts_confirm(
    State(state): State<AppState>,
    Path(draft_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let mut draft = get_mutable_draft(&state, &draft_id).await?;
    ensure_open_draft(&draft, now)?;
    if draft.status != ChannelAutomationDraftStatus::PreviewReady {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Draft is not ready for confirmation",
                "code": "CHANNEL_AUTOMATION_DRAFT_NOT_READY",
                "draft": draft,
            })),
        ));
    }
    let automation = build_channel_automation(&draft, now);
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "CHANNEL_AUTOMATION_CREATE_FAILED",
            })),
        )
    })?;
    draft.status = ChannelAutomationDraftStatus::Applied;
    draft.automation_id = Some(stored.automation_id.clone());
    draft.question = None;
    draft.updated_at_ms = now;
    store_draft(&state, draft.clone()).await;
    let mut payload = draft_response(&draft);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("automation".to_string(), json!(stored));
    }
    Ok(Json(payload))
}

pub(super) async fn channel_automation_drafts_cancel(
    State(state): State<AppState>,
    Path(draft_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let mut draft = get_mutable_draft(&state, &draft_id).await?;
    draft.status = ChannelAutomationDraftStatus::Cancelled;
    draft.question = None;
    draft.updated_at_ms = now;
    store_draft(&state, draft.clone()).await;
    Ok(Json(draft_response(&draft)))
}

pub(super) async fn channel_automation_drafts_pending(
    State(state): State<AppState>,
    Query(query): Query<ChannelAutomationDraftPendingQuery>,
) -> Json<Value> {
    let now = crate::now_ms();
    let drafts = state
        .channel_automation_drafts
        .read()
        .await
        .values()
        .filter(|draft| draft.expires_at_ms > now)
        .filter(|draft| {
            query
                .channel
                .as_deref()
                .map(|value| draft.channel_context.source_platform == value)
                .unwrap_or(true)
        })
        .filter(|draft| {
            query
                .scope_id
                .as_deref()
                .map(|value| draft.channel_context.scope_id == value)
                .unwrap_or(true)
        })
        .filter(|draft| {
            query
                .sender
                .as_deref()
                .map(|value| draft.channel_context.sender == value)
                .unwrap_or(true)
        })
        .filter(|draft| {
            matches!(
                draft.status,
                ChannelAutomationDraftStatus::Collecting
                    | ChannelAutomationDraftStatus::PreviewReady
                    | ChannelAutomationDraftStatus::Blocked
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let count = drafts.len();
    Json(json!({
        "drafts": drafts,
        "count": count,
    }))
}

async fn get_mutable_draft(
    state: &AppState,
    draft_id: &str,
) -> Result<ChannelAutomationDraftRecord, (StatusCode, Json<Value>)> {
    state
        .channel_automation_drafts
        .read()
        .await
        .get(draft_id)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Channel automation draft not found",
                    "code": "CHANNEL_AUTOMATION_DRAFT_NOT_FOUND",
                    "draft_id": draft_id,
                })),
            )
        })
}

async fn store_draft(state: &AppState, draft: ChannelAutomationDraftRecord) {
    state
        .channel_automation_drafts
        .write()
        .await
        .insert(draft.draft_id.clone(), draft);
    persist_channel_drafts_or_log(state).await;
}

async fn persist_channel_drafts_or_log(state: &AppState) {
    if let Err(error) = state.persist_channel_automation_drafts().await {
        tracing::warn!("failed to persist channel automation drafts: {error}");
    }
}

fn ensure_open_draft(
    draft: &ChannelAutomationDraftRecord,
    now: u64,
) -> Result<(), (StatusCode, Json<Value>)> {
    if draft.expires_at_ms <= now {
        return Err((
            StatusCode::GONE,
            Json(json!({
                "error": "Channel automation draft expired",
                "code": "CHANNEL_AUTOMATION_DRAFT_EXPIRED",
                "draft_id": draft.draft_id,
            })),
        ));
    }
    match draft.status {
        ChannelAutomationDraftStatus::Cancelled
        | ChannelAutomationDraftStatus::Expired
        | ChannelAutomationDraftStatus::Applied => Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Channel automation draft is closed",
                "code": "CHANNEL_AUTOMATION_DRAFT_CLOSED",
                "draft_id": draft.draft_id,
                "status": draft.status.clone(),
            })),
        )),
        ChannelAutomationDraftStatus::Collecting
        | ChannelAutomationDraftStatus::PreviewReady
        | ChannelAutomationDraftStatus::Blocked => Ok(()),
    }
}

fn advance_draft(draft: &mut ChannelAutomationDraftRecord, now: u64) {
    draft.updated_at_ms = now;
    draft.expires_at_ms = now.saturating_add(CHANNEL_DRAFT_TTL_MS);
    draft.missing_fields.clear();
    draft.question = None;
    draft.preview = None;

    if draft
        .goal
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        draft.status = ChannelAutomationDraftStatus::Collecting;
        draft.missing_fields.push("goal".to_string());
        draft.question = Some(ChannelAutomationDraftQuestion {
            field: "goal".to_string(),
            text: "What should this automation do?".to_string(),
            options: Vec::new(),
        });
        return;
    }
    if draft
        .schedule_hint
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        draft.status = ChannelAutomationDraftStatus::Collecting;
        draft.missing_fields.push("schedule_hint".to_string());
        draft.question = Some(ChannelAutomationDraftQuestion {
            field: "schedule_hint".to_string(),
            text: "When should it run, or what event should trigger it?".to_string(),
            options: vec![
                "daily".to_string(),
                "weekly".to_string(),
                "when something changes".to_string(),
            ],
        });
        return;
    }

    let goal = draft.goal.clone().unwrap_or_default();
    let schedule_hint = draft.schedule_hint.clone().unwrap_or_default();
    let delivery_target = draft
        .delivery_target
        .clone()
        .unwrap_or_else(|| "same_chat".to_string());
    draft.status = ChannelAutomationDraftStatus::PreviewReady;
    draft.preview = Some(ChannelAutomationDraftPreview {
        summary: format!(
            "Run `{}` on `{}` and report to `{}`.",
            truncate_for_label(&goal, 80),
            schedule_hint,
            delivery_target
        ),
        goal,
        schedule_hint,
        delivery_target,
    });
}

fn draft_response(draft: &ChannelAutomationDraftRecord) -> Value {
    let message = format_channel_draft_message(draft);
    log_channel_automation_draft_response_producer(
        draft,
        None,
        draft.workflow_planner_enabled,
        draft.strict_kb_grounding.unwrap_or(false),
        draft.factual_question.unwrap_or(false),
        draft.explicit_workflow_intent.unwrap_or(false),
        false,
        true,
        "draft_response",
    );
    json!({
        "draft": draft,
        "message": message,
    })
}

#[derive(Debug, Clone, Copy)]
enum ProducerCaller {
    Start,
    Answer,
}

impl ProducerCaller {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "channel_automation_drafts_start",
            Self::Answer => "channel_automation_drafts_answer",
        }
    }
}

async fn producer_guarded_response(
    state: &AppState,
    draft: &mut ChannelAutomationDraftRecord,
    pending_draft_id: Option<&str>,
    caller: ProducerCaller,
    strict_kb_grounding: bool,
    factual_question: bool,
    explicit_workflow_intent: bool,
) -> Option<Value> {
    let workflow_planner_enabled = draft.workflow_planner_enabled.unwrap_or(true);
    let block_reason = if !workflow_planner_enabled {
        Some((
            "workflow_drafting_disabled",
            CHANNEL_WORKFLOW_DRAFTING_DISABLED_MESSAGE,
        ))
    } else if strict_kb_grounding && factual_question && !explicit_workflow_intent {
        Some((
            "strict_kb_factual_question",
            STRICT_KB_FACTUAL_DRAFT_BLOCKED_MESSAGE,
        ))
    } else {
        None
    };

    let Some((reason, message)) = block_reason else {
        log_channel_automation_draft_response_producer(
            draft,
            pending_draft_id,
            Some(workflow_planner_enabled),
            strict_kb_grounding,
            factual_question,
            explicit_workflow_intent,
            false,
            false,
            caller.label(),
        );
        return None;
    };

    draft.status = ChannelAutomationDraftStatus::Cancelled;
    draft.goal = None;
    draft.schedule_hint = None;
    draft.delivery_target = None;
    draft.missing_fields.clear();
    draft.question = None;
    draft.preview = None;
    draft.updated_at_ms = crate::now_ms();
    draft.workflow_planner_enabled = Some(workflow_planner_enabled);
    draft.strict_kb_grounding = Some(strict_kb_grounding);
    draft.factual_question = Some(factual_question);
    draft.explicit_workflow_intent = Some(explicit_workflow_intent);
    store_draft(state, draft.clone()).await;
    log_channel_automation_draft_response_producer(
        draft,
        pending_draft_id,
        Some(workflow_planner_enabled),
        strict_kb_grounding,
        factual_question,
        explicit_workflow_intent,
        true,
        false,
        reason,
    );
    Some(json!({
        "draft": draft,
        "message": message,
        "blocked": true,
        "block_reason": reason,
    }))
}

async fn channel_strict_kb_grounding(state: &AppState, channel: &str) -> bool {
    let channel = channel.trim().to_ascii_lowercase();
    if channel.is_empty() {
        return false;
    }
    state
        .config
        .get_effective_value()
        .await
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|channels| channels.get(&channel))
        .and_then(Value::as_object)
        .and_then(|cfg| cfg.get("strict_kb_grounding"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn channel_draft_message_is_factual_question(message: &str) -> bool {
    let text = message.trim().to_ascii_lowercase();
    if text.is_empty() || text.starts_with('/') || !text.contains('?') {
        return false;
    }
    [
        "what ", "who ", "where ", "when ", "which ", "can ", "could ", "does ", "do ", "is ",
        "are ", "how ", "tell me ", "explain ",
    ]
    .iter()
    .any(|starter| text.starts_with(starter))
}

fn channel_draft_message_has_explicit_workflow_intent(message: &str) -> bool {
    let text = message.trim().to_ascii_lowercase();
    if text.is_empty() || text.starts_with('/') {
        return false;
    }
    let contains_explicit_phrase = [
        "create a workflow",
        "create workflow",
        "build a workflow",
        "build workflow",
        "set up a workflow",
        "setup a workflow",
        "make a workflow",
        "create an automation",
        "create automation",
        "build an automation",
        "build automation",
        "schedule a workflow",
        "schedule a daily report",
        "schedule a report",
        "schedule a reminder",
        "set up a bot",
        "setup a bot",
        "make a bot that runs",
        "create a bot that runs",
    ]
    .iter()
    .any(|phrase| text.contains(phrase));
    let contains_workflow_target = ["workflow", "automation", "automations", "bot", "reminder"]
        .iter()
        .any(|word| text.contains(word));
    let contains_authoring_verb = [
        "create", "build", "make", "draft", "schedule", "automate", "set up", "setup",
    ]
    .iter()
    .any(|word| text.contains(word));
    let monitoring_request = text.starts_with("monitor ")
        && [
            " every ",
            " each ",
            "daily",
            "weekly",
            "hourly",
            "every morning",
        ]
        .iter()
        .any(|word| text.contains(word));
    contains_explicit_phrase
        || (contains_workflow_target && contains_authoring_verb)
        || monitoring_request
}

#[allow(clippy::too_many_arguments)]
fn log_channel_automation_draft_response_producer(
    draft: &ChannelAutomationDraftRecord,
    pending_draft_id: Option<&str>,
    workflow_planner_enabled: Option<bool>,
    strict_kb_grounding: bool,
    factual_question: bool,
    explicit_workflow_intent: bool,
    blocked: bool,
    emitted: bool,
    reason: &str,
) {
    tracing::warn!(
        prefix = "CHANNEL_AUTOMATION_DRAFT_RESPONSE_PRODUCER",
        channel = %draft.channel_context.source_platform,
        platform = %draft.channel_context.source_platform,
        session_id = ?draft.channel_context.session_id,
        scope_id = %draft.channel_context.scope_id,
        draft_id = %draft.draft_id,
        pending_draft_id = ?pending_draft_id,
        workflow_planner_enabled = ?workflow_planner_enabled,
        strict_kb_grounding,
        factual_question,
        explicit_workflow_intent,
        blocked,
        emitted,
        reason,
        "CHANNEL_AUTOMATION_DRAFT_RESPONSE_PRODUCER"
    );
}

fn format_channel_draft_message(draft: &ChannelAutomationDraftRecord) -> String {
    match draft.status {
        ChannelAutomationDraftStatus::Collecting => {
            let question = draft
                .question
                .as_ref()
                .map(|question| {
                    let mut lines = vec![question.text.clone()];
                    for (index, option) in question.options.iter().enumerate() {
                        lines.push(format!("{}. {}", index + 1, option));
                    }
                    lines.join("\n")
                })
                .unwrap_or_else(|| {
                    "I need one more detail before I can draft this automation.".to_string()
                });
            format!("{question}\nReply here with the answer, or reply `cancel` to stop.")
        }
        ChannelAutomationDraftStatus::PreviewReady => {
            let preview = draft.preview.as_ref();
            let summary = preview
                .map(|value| value.summary.clone())
                .unwrap_or_else(|| "Automation draft is ready.".to_string());
            format!("{summary}\nReply `confirm` to create it, or `cancel` to stop.")
        }
        ChannelAutomationDraftStatus::Applied => {
            let id = draft.automation_id.as_deref().unwrap_or("unknown");
            format!("Automation created: `{id}`")
        }
        ChannelAutomationDraftStatus::Cancelled => "Automation draft cancelled.".to_string(),
        ChannelAutomationDraftStatus::Expired => "Automation draft expired.".to_string(),
        ChannelAutomationDraftStatus::Blocked => {
            "Automation draft is blocked until required channel setup is completed.".to_string()
        }
    }
}

fn build_channel_automation(draft: &ChannelAutomationDraftRecord, now: u64) -> AutomationV2Spec {
    let goal = draft
        .goal
        .clone()
        .unwrap_or_else(|| draft.original_text.clone());
    let schedule_hint = draft.schedule_hint.clone().unwrap_or_default();
    let automation_id = format!("automation-v2-{}", Uuid::new_v4());
    let agent_id = format!("agent-{}", Uuid::new_v4());
    let output_target = format!(
        "channel:{}:{}",
        empty_as_unknown(&draft.channel_context.source_platform),
        empty_as_unknown(&draft.channel_context.scope_id)
    );
    let metadata = json!({
        "created_from": "channel_automation_draft",
        "draft_id": draft.draft_id,
        "channel_context": draft.channel_context,
        "schedule_hint": schedule_hint,
        "delivery_target": draft.delivery_target,
        "security_profile": draft.security_profile,
        "allowed_tools": draft.allowed_tools,
        "allowed_mcp_servers": draft.allowed_mcp_servers,
        "allowed_mcp_tools": draft.allowed_mcp_tools,
        "confirmation_required": true,
        "confirmed_at_ms": now,
    });
    AutomationV2Spec {
        automation_id,
        name: automation_name_from_goal(&goal),
        description: Some(format!("Created from channel chat: {goal}")),
        status: AutomationV2Status::Active,
        schedule: schedule_from_hint(&schedule_hint),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: agent_id.clone(),
            template_id: None,
            display_name: "Channel Automation Agent".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: draft.allowed_tools.clone(),
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: draft.allowed_mcp_servers.clone(),
                allowed_tools: if draft.allowed_mcp_tools.is_empty() {
                    None
                } else {
                    Some(draft.allowed_mcp_tools.clone())
                },
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                node_id: "channel_automation_task".to_string(),
                agent_id,
                objective: format!(
                    "{goal}\n\nReport results back to the originating chat context."
                ),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "channel_response".to_string(),
                    validator: Some(AutomationOutputValidatorKind::GenericArtifact),
                    enforcement: None,
                    schema: None,
                    summary_guidance: Some(
                        "Summarize the outcome in channel-safe language.".to_string(),
                    ),
                }),
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: Some(16),
                stage_kind: None,
                gate: None,
                metadata: Some(json!({
                    "created_from": "channel_automation_draft",
                    "source_platform": draft.channel_context.source_platform,
                    "source_scope_id": draft.channel_context.scope_id,
                })),
            }],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: Some(15 * 60 * 1000),
            max_total_tool_calls: Some(24),
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec![output_target],
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: if draft.channel_context.sender.trim().is_empty() {
            "channel".to_string()
        } else {
            draft.channel_context.sender.clone()
        },
        workspace_root: None,
        metadata: Some(metadata),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn schedule_from_hint(hint: &str) -> AutomationV2Schedule {
    let lower = hint.to_ascii_lowercase();
    if lower.contains("daily") || lower.contains("every day") {
        if let Some((hour, minute)) = parse_daily_time_hint(&lower) {
            return AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Cron,
                cron_expression: Some(format!("0 {minute} {hour} * * * *")),
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
            };
        }
    }
    let interval_seconds = if lower.contains("hourly") || lower.contains("every hour") {
        Some(60 * 60)
    } else if lower.contains("weekly") || lower.contains("every week") {
        Some(7 * 24 * 60 * 60)
    } else if lower.contains("daily") || lower.contains("every day") {
        Some(24 * 60 * 60)
    } else {
        None
    };
    AutomationV2Schedule {
        schedule_type: interval_seconds
            .map(|_| AutomationV2ScheduleType::Interval)
            .unwrap_or(AutomationV2ScheduleType::Manual),
        cron_expression: None,
        interval_seconds,
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
    }
}

fn parse_daily_time_hint(lower: &str) -> Option<(u32, u32)> {
    let marker = lower
        .find(" at ")
        .map(|index| index + 4)
        .or_else(|| lower.find(" around ").map(|index| index + 8))?;
    let tail = lower.get(marker..)?.trim();
    let token = tail
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch: char| ch == '.' || ch == ',');
    if token.is_empty() {
        return None;
    }
    let is_pm = token.ends_with("pm");
    let is_am = token.ends_with("am");
    let numeric = token.trim_end_matches("am").trim_end_matches("pm");
    let mut pieces = numeric.split(':');
    let mut hour = pieces.next()?.parse::<u32>().ok()?;
    let minute = pieces
        .next()
        .map(|value| value.parse::<u32>().ok())
        .unwrap_or(Some(0))?;
    if hour > 23 || minute > 59 {
        return None;
    }
    if is_pm && hour < 12 {
        hour += 12;
    } else if is_am && hour == 12 {
        hour = 0;
    }
    Some((hour, minute))
}

fn infer_goal(text: &str) -> Option<String> {
    let mut candidate = text.trim();
    let lower = candidate.to_ascii_lowercase();
    let generic = [
        "create an automation",
        "create automation",
        "make an automation",
        "set up an automation",
        "setup an automation",
        "automate this",
    ];
    if generic.iter().any(|value| lower == *value) {
        return None;
    }
    let prefixes = [
        "create an automation that",
        "create automation that",
        "make an automation that",
        "set up an automation that",
        "setup an automation that",
        "automate",
        "please automate",
    ];
    for prefix in prefixes {
        if lower.starts_with(prefix) {
            candidate = candidate.get(prefix.len()..).unwrap_or(candidate).trim();
            break;
        }
    }
    if candidate.chars().count() < 8 {
        None
    } else {
        Some(candidate.to_string())
    }
}

fn infer_schedule_hint(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("hourly") || lower.contains("every hour") {
        return Some("hourly".to_string());
    }
    if lower.contains("weekly") || lower.contains("every week") {
        return Some("weekly".to_string());
    }
    if lower.contains("daily") || lower.contains("every day") || lower.contains("each day") {
        return Some(extract_daily_hint(text));
    }
    if lower.starts_with("when ")
        || lower.contains(" whenever ")
        || lower.contains(" when ")
        || lower.contains(" on new ")
        || lower.contains(" new issue")
    {
        return Some("event-driven".to_string());
    }
    None
}

fn extract_daily_hint(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    for marker in [" at ", " around "] {
        if let Some(index) = lower.find(marker) {
            let time = text[index + marker.len()..]
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            if !time.trim().is_empty() {
                return format!(
                    "daily at {}",
                    time.trim_matches(|ch: char| ch == '.' || ch == ',')
                );
            }
        }
    }
    "daily".to_string()
}

fn infer_delivery_target(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains(" here") || lower.contains("this channel") || lower.contains("this chat") {
        "same_chat".to_string()
    } else {
        "same_chat".to_string()
    }
}

fn automation_name_from_goal(goal: &str) -> String {
    let label = truncate_for_label(goal.trim(), 56);
    if label.is_empty() {
        "Channel automation".to_string()
    } else {
        label
    }
}

fn truncate_for_label(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut clipped = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    clipped.push_str("...");
    clipped
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn empty_as_unknown(value: &str) -> &str {
    if value.trim().is_empty() {
        "unknown"
    } else {
        value
    }
}

fn is_cancel_text(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "cancel" | "stop" | "abort" | "never mind" | "nevermind"
    )
}

fn bad_request(detail: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "Invalid channel automation draft request",
            "code": "CHANNEL_AUTOMATION_DRAFT_INVALID",
            "detail": detail,
        })),
    )
}
