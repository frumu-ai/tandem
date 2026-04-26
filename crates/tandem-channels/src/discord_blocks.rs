//! Discord interaction renderer for [`InteractiveCard`].
//!
//! Pure functions: take an [`InteractiveCard`] in, produce a `serde_json::Value`
//! suitable for Discord's REST endpoints (`POST /channels/{id}/messages`,
//! `PATCH /channels/{id}/messages/{msg}`) or interaction responses
//! (`InteractionResponseType` = update/modal). No I/O; trivially golden-testable.
//!
//! Discord reference: <https://discord.com/developers/docs/interactions/message-components>.
//!
//! # Layout (delivered as a single embed + action row)
//!
//! ```text
//! ┌────────────────────────────────────────┐
//! │  Embed                                 │
//! │   title:    card.title                 │
//! │   color:    yellow (pending)           │
//! │   description: body markdown           │
//! │   fields:   key/value pairs            │
//! │   footer:   workflow / run_id          │
//! ├────────────────────────────────────────┤
//! │  Action row                            │
//! │   [Approve ✓ green] [Rework ~]         │
//! │   [Cancel ✗ red — confirms]            │
//! └────────────────────────────────────────┘
//! ```
//!
//! Discord caps action rows at 5 buttons; if a card has more buttons, we chunk
//! into multiple action rows. Components arrays are capped at 5 rows.

use serde_json::{json, Value};

use crate::traits::{
    InteractiveCard, InteractiveCardButton, InteractiveCardButtonStyle, InteractiveCardField,
};

/// Discord embed colors as 24-bit RGB integers (the Discord wire format).
const COLOR_PENDING: u32 = 0xF59E0B; // amber-500 (pending decision)
const COLOR_APPROVED: u32 = 0x10B981; // emerald-500 (decided: approve)
const COLOR_CANCELLED: u32 = 0xEF4444; // red-500 (decided: cancel)
const COLOR_REWORKED: u32 = 0x6366F1; // indigo-500 (decided: rework)

/// Discord button styles. See
/// <https://discord.com/developers/docs/interactions/message-components#buttons-button-styles>.
/// Style 1 (PRIMARY/blurple) and 5 (LINK) are intentionally unused — Tandem
/// approval cards map Tandem semantic styles to Discord SUCCESS/DANGER/SECONDARY.
const STYLE_SECONDARY: u32 = 2; // grey
const STYLE_SUCCESS: u32 = 3; // green
const STYLE_DANGER: u32 = 4; // red

/// Render an [`InteractiveCard`] to a complete `chat.create-message` payload
/// for Discord. Includes embeds + components, ready to POST.
///
/// `thread_id` (when present) does not affect the body — Discord routes thread
/// messages by URL path (`/channels/{thread_id}/messages`); callers pass the
/// thread ID at the request-construction level. We expose it here so the
/// builder fully describes "where this card should land."
pub fn build_create_message_payload(card: &InteractiveCard) -> Value {
    let mut payload = json!({
        "embeds": [render_embed(card, EmbedState::Pending)],
        "components": render_components(&card.correlation, &card.buttons),
    });
    // Discord lets you set `flags = 64` for ephemeral, etc. We do not need
    // it here; cards are always public so the channel can audit.
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("allowed_mentions".to_string(), json!({"parse": []}));
    }
    payload
}

/// Build the JSON payload for the in-place edit that fires after a decision.
/// Discord uses `PATCH /channels/{ch}/messages/{ts}` for this. The components
/// array is replaced with an empty list (Discord requires `components: []`,
/// not omission, to clear buttons), and the embed is updated to the
/// post-decision color + a status footer.
pub fn build_edit_message_payload_for_decision(
    card: &InteractiveCard,
    decision: DecisionOutcome,
    decided_by_display: &str,
    decision_summary_markdown: &str,
) -> Value {
    let state = EmbedState::from_decision(decision);
    let mut embed = render_embed(card, state);
    if let Some(obj) = embed.as_object_mut() {
        obj.insert(
            "description".to_string(),
            Value::String(decision_summary_markdown.to_string()),
        );
        obj.insert("footer".to_string(), json!({"text": decided_by_display}));
    }
    json!({
        "embeds": [embed],
        "components": Value::Array(Vec::new()),
        "allowed_mentions": {"parse": []},
    })
}

/// Build a Discord modal for the rework reason flow.
///
/// Discord modals (component_type = TextInput inside an action_row inside a
/// modal payload) are returned in an interaction response with type `9`
/// (`MODAL`) — the response body wraps `{ "type": 9, "data": ... }`.
/// We return just the `data` portion so the HTTP handler can decide whether
/// to wrap it as an interaction response or a follow-up.
pub fn build_rework_modal_data(card: &InteractiveCard, custom_id: &str) -> Option<Value> {
    let prompt = card.reason_prompt.as_ref()?;
    Some(json!({
        "title": clamp_for_field(&prompt.modal_title, 45),
        "custom_id": custom_id,
        "components": [
            {
                "type": 1, // ACTION_ROW
                "components": [
                    {
                        "type": 4, // TEXT_INPUT
                        "custom_id": "reason_input",
                        "label": clamp_for_field(&prompt.field_label, 45),
                        "style": 2, // PARAGRAPH (multi-line)
                        "min_length": 1,
                        "max_length": 4000,
                        "required": true,
                        "placeholder": prompt
                            .field_placeholder
                            .as_deref()
                            .map(|p| clamp_for_field(p, 100))
                            .unwrap_or_default(),
                    }
                ]
            }
        ]
    }))
}

/// Build a Discord interaction response that wraps `data` as a modal open.
pub fn wrap_as_modal_response(data: Value) -> Value {
    json!({ "type": 9, "data": data })
}

/// Build a Discord interaction response that updates the original message
/// (used by interaction handlers when they want to ack with content rather
/// than a 200 deferred ack).
pub fn build_update_message_response(payload: Value) -> Value {
    json!({ "type": 7, "data": payload })
}

/// Build a Discord interaction response that defers (3-second-ack-safe).
/// The actual edit lands later via `webhooks/{app_id}/{interaction_token}`.
pub fn build_deferred_update_response() -> Value {
    json!({ "type": 6 })
}

/// Decision outcome for use with `build_edit_message_payload_for_decision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionOutcome {
    Approved,
    Reworked,
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
enum EmbedState {
    Pending,
    Approved,
    Reworked,
    Cancelled,
}

impl EmbedState {
    fn color(self) -> u32 {
        match self {
            EmbedState::Pending => COLOR_PENDING,
            EmbedState::Approved => COLOR_APPROVED,
            EmbedState::Reworked => COLOR_REWORKED,
            EmbedState::Cancelled => COLOR_CANCELLED,
        }
    }

    fn from_decision(decision: DecisionOutcome) -> Self {
        match decision {
            DecisionOutcome::Approved => EmbedState::Approved,
            DecisionOutcome::Reworked => EmbedState::Reworked,
            DecisionOutcome::Cancelled => EmbedState::Cancelled,
        }
    }
}

fn render_embed(card: &InteractiveCard, state: EmbedState) -> Value {
    let mut embed = json!({
        "title": clamp_for_field(&card.title, 256),
        "color": state.color(),
    });
    if !card.body_markdown.trim().is_empty() {
        if let Some(obj) = embed.as_object_mut() {
            obj.insert(
                "description".to_string(),
                Value::String(clamp_for_field(&card.body_markdown, 4096)),
            );
        }
    }
    if !card.fields.is_empty() {
        if let Some(obj) = embed.as_object_mut() {
            obj.insert(
                "fields".to_string(),
                Value::Array(render_fields(&card.fields)),
            );
        }
    }
    if let Some(footer_text) = footer_text_from_correlation(&card.correlation) {
        if let Some(obj) = embed.as_object_mut() {
            obj.insert(
                "footer".to_string(),
                json!({"text": clamp_for_field(&footer_text, 2048)}),
            );
        }
    }
    embed
}

fn render_fields(fields: &[InteractiveCardField]) -> Vec<Value> {
    // Discord caps embeds at 25 fields total; each field name <= 256 chars,
    // each value <= 1024. Inline: short fields render side by side.
    fields
        .iter()
        .take(25)
        .map(|f| {
            json!({
                "name": clamp_for_field(&f.label, 256),
                "value": clamp_for_field(&f.value, 1024),
                "inline": f.value.len() <= 80,
            })
        })
        .collect()
}

fn render_components(correlation: &Value, buttons: &[InteractiveCardButton]) -> Value {
    if buttons.is_empty() {
        return Value::Array(Vec::new());
    }
    // Discord: 1 row holds up to 5 buttons; up to 5 rows per message. We cap
    // at 25 buttons (5 × 5) silently — beyond that is a UX problem callers
    // should fix at the card level.
    let rows: Vec<Value> = buttons
        .chunks(5)
        .take(5)
        .map(|chunk| {
            json!({
                "type": 1, // ACTION_ROW
                "components": chunk
                    .iter()
                    .map(|btn| render_button(correlation, btn))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    Value::Array(rows)
}

fn render_button(correlation: &Value, btn: &InteractiveCardButton) -> Value {
    let custom_id = build_custom_id(correlation, &btn.action_id);
    let style = button_style_to_discord(btn.style);
    let mut element = json!({
        "type": 2, // BUTTON
        "style": style,
        "label": clamp_for_field(&btn.label, 80),
        "custom_id": custom_id,
    });
    if btn.requires_reason {
        // Hint to the interaction handler that this button should open a modal
        // instead of dispatching the decision directly. The handler reads
        // `requires_reason` from the parsed custom_id correlation.
        if let Some(obj) = element.as_object_mut() {
            obj.insert("style".to_string(), Value::Number(STYLE_SECONDARY.into()));
        }
    }
    element
}

fn button_style_to_discord(style: InteractiveCardButtonStyle) -> u32 {
    match style {
        InteractiveCardButtonStyle::Default => STYLE_SECONDARY,
        InteractiveCardButtonStyle::Primary => STYLE_SUCCESS,
        InteractiveCardButtonStyle::Destructive => STYLE_DANGER,
    }
}

/// Discord `custom_id` is the round-trip identifier for a button click. We
/// stuff the action_id and a short correlation hash so the interaction
/// handler can dispatch without holding state. Custom IDs are capped at 100
/// chars by Discord, so we truncate the correlation if needed.
fn build_custom_id(correlation: &Value, action_id: &str) -> String {
    let run_id = correlation
        .pointer("/automation_v2_run_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let node_id = correlation
        .pointer("/node_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    // Format: `tdm:{action}:{run_id}:{node_id}`. Discord's limit is 100 chars.
    let raw = format!("tdm:{action_id}:{run_id}:{node_id}");
    if raw.chars().count() <= 100 {
        return raw;
    }
    let truncated: String = raw.chars().take(99).collect();
    format!("{truncated}…")
}

/// Parse a `custom_id` produced by `build_custom_id` back into its parts.
/// Returns `None` for malformed IDs (callers reject those as 400).
pub fn parse_custom_id(custom_id: &str) -> Option<ParsedCustomId> {
    let mut parts = custom_id.splitn(4, ':');
    let prefix = parts.next()?;
    if prefix != "tdm" {
        return None;
    }
    let action = parts.next()?;
    let run_id = parts.next()?;
    let node_id = parts.next().unwrap_or("");
    Some(ParsedCustomId {
        action: action.to_string(),
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCustomId {
    pub action: String,
    pub run_id: String,
    pub node_id: String,
}

fn footer_text_from_correlation(correlation: &Value) -> Option<String> {
    let run_id = correlation
        .pointer("/automation_v2_run_id")
        .and_then(Value::as_str)?;
    Some(format!("Tandem · {}", run_id))
}

fn clamp_for_field(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{
        InteractiveCardButton, InteractiveCardButtonStyle, InteractiveCardConfirm,
        InteractiveCardField, InteractiveCardReasonPrompt,
    };

    fn approval_card() -> InteractiveCard {
        InteractiveCard {
            recipient: "channel-12345".to_string(),
            title: "sales-research-outreach · approve outbound email".to_string(),
            body_markdown:
                "Will email **alice@example.com** with subject _Quick question about your stack_."
                    .to_string(),
            fields: vec![
                InteractiveCardField {
                    label: "Run".to_string(),
                    value: "auto-v2-run-abc123".to_string(),
                },
                InteractiveCardField {
                    label: "Workflow".to_string(),
                    value: "sales-research-outreach".to_string(),
                },
                InteractiveCardField {
                    label: "Recipient".to_string(),
                    value: "alice@example.com".to_string(),
                },
            ],
            buttons: vec![
                InteractiveCardButton {
                    action_id: "approve".to_string(),
                    label: "Approve".to_string(),
                    style: InteractiveCardButtonStyle::Primary,
                    requires_reason: false,
                    confirm: None,
                },
                InteractiveCardButton {
                    action_id: "rework".to_string(),
                    label: "Rework".to_string(),
                    style: InteractiveCardButtonStyle::Default,
                    requires_reason: true,
                    confirm: None,
                },
                InteractiveCardButton {
                    action_id: "cancel".to_string(),
                    label: "Cancel".to_string(),
                    style: InteractiveCardButtonStyle::Destructive,
                    requires_reason: false,
                    confirm: Some(InteractiveCardConfirm {
                        title: "Cancel run?".to_string(),
                        body: "This stops the workflow run.".to_string(),
                        confirm_label: "Cancel run".to_string(),
                        deny_label: "Keep waiting".to_string(),
                    }),
                },
            ],
            reason_prompt: Some(InteractiveCardReasonPrompt {
                modal_title: "Rework feedback".to_string(),
                field_label: "What should change?".to_string(),
                field_placeholder: Some("Tighten the ICP filter…".to_string()),
                submit_label: "Send back".to_string(),
            }),
            thread_key: Some("run-thread-abc123".to_string()),
            correlation: json!({
                "automation_v2_run_id": "auto-v2-run-abc123",
                "node_id": "send_email",
                "request_id": "automation_v2:auto-v2-run-abc123:send_email"
            }),
        }
    }

    #[test]
    fn create_message_payload_emits_one_embed_and_action_rows() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let embeds = payload.get("embeds").and_then(Value::as_array).unwrap();
        assert_eq!(embeds.len(), 1);
        let components = payload.get("components").and_then(Value::as_array).unwrap();
        assert_eq!(components.len(), 1, "3 buttons fit in one action row");
        let row = &components[0];
        assert_eq!(row.get("type").and_then(Value::as_u64), Some(1));
        let buttons = row.get("components").and_then(Value::as_array).unwrap();
        assert_eq!(buttons.len(), 3);
    }

    #[test]
    fn embed_uses_pending_color_initially() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let color = payload
            .pointer("/embeds/0/color")
            .and_then(Value::as_u64)
            .unwrap();
        assert_eq!(color as u32, COLOR_PENDING);
    }

    #[test]
    fn embed_includes_title_and_description() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let title = payload
            .pointer("/embeds/0/title")
            .and_then(Value::as_str)
            .unwrap();
        assert!(title.contains("sales-research-outreach"));
        let description = payload
            .pointer("/embeds/0/description")
            .and_then(Value::as_str)
            .unwrap();
        assert!(description.contains("alice@example.com"));
    }

    #[test]
    fn embed_includes_fields_with_inline_short_values() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let fields = payload
            .pointer("/embeds/0/fields")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(fields.len(), 3);
        // All sample values are short, so all should be inline.
        for field in fields {
            assert_eq!(field.get("inline").and_then(Value::as_bool), Some(true));
        }
    }

    #[test]
    fn embed_footer_uses_run_id_from_correlation() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let footer = payload
            .pointer("/embeds/0/footer/text")
            .and_then(Value::as_str)
            .unwrap();
        assert!(footer.contains("auto-v2-run-abc123"));
    }

    #[test]
    fn approve_button_uses_success_style() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let approve = payload.pointer("/components/0/components/0").unwrap();
        assert_eq!(
            approve.get("style").and_then(Value::as_u64),
            Some(STYLE_SUCCESS as u64)
        );
        assert_eq!(
            approve.get("label").and_then(Value::as_str),
            Some("Approve")
        );
    }

    #[test]
    fn cancel_button_uses_danger_style() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let cancel = payload.pointer("/components/0/components/2").unwrap();
        assert_eq!(
            cancel.get("style").and_then(Value::as_u64),
            Some(STYLE_DANGER as u64)
        );
    }

    #[test]
    fn button_custom_id_round_trips() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let approve = payload.pointer("/components/0/components/0").unwrap();
        let custom_id = approve.get("custom_id").and_then(Value::as_str).unwrap();
        let parsed = parse_custom_id(custom_id).expect("parses");
        assert_eq!(parsed.action, "approve");
        assert_eq!(parsed.run_id, "auto-v2-run-abc123");
        assert_eq!(parsed.node_id, "send_email");
    }

    #[test]
    fn parse_custom_id_rejects_unknown_prefix() {
        assert!(parse_custom_id("other:approve:run:node").is_none());
        assert!(parse_custom_id("notdm").is_none());
    }

    #[test]
    fn parse_custom_id_handles_missing_node_id() {
        let parsed = parse_custom_id("tdm:approve:run-1").expect("parses");
        assert_eq!(parsed.action, "approve");
        assert_eq!(parsed.run_id, "run-1");
        assert_eq!(parsed.node_id, "");
    }

    #[test]
    fn chunks_more_than_five_buttons_into_multiple_rows() {
        let mut card = approval_card();
        card.buttons = (0..7)
            .map(|i| InteractiveCardButton {
                action_id: format!("act{i}"),
                label: format!("Button {i}"),
                style: InteractiveCardButtonStyle::Default,
                requires_reason: false,
                confirm: None,
            })
            .collect();
        let payload = build_create_message_payload(&card);
        let components = payload.get("components").and_then(Value::as_array).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0]
                .get("components")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            5
        );
        assert_eq!(
            components[1]
                .get("components")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn edit_message_payload_clears_components_on_decision() {
        let card = approval_card();
        let payload = build_edit_message_payload_for_decision(
            &card,
            DecisionOutcome::Approved,
            "Approved by @alice at 14:32",
            "Outbound email sent.",
        );
        let components = payload.get("components").and_then(Value::as_array).unwrap();
        assert!(
            components.is_empty(),
            "buttons must be cleared after decide"
        );
        let color = payload
            .pointer("/embeds/0/color")
            .and_then(Value::as_u64)
            .unwrap();
        assert_eq!(color as u32, COLOR_APPROVED);
        let footer = payload
            .pointer("/embeds/0/footer/text")
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(footer, "Approved by @alice at 14:32");
        let description = payload
            .pointer("/embeds/0/description")
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(description, "Outbound email sent.");
    }

    #[test]
    fn edit_message_payload_uses_red_for_cancelled() {
        let card = approval_card();
        let payload = build_edit_message_payload_for_decision(
            &card,
            DecisionOutcome::Cancelled,
            "Cancelled by @alice",
            "Run stopped.",
        );
        let color = payload
            .pointer("/embeds/0/color")
            .and_then(Value::as_u64)
            .unwrap();
        assert_eq!(color as u32, COLOR_CANCELLED);
    }

    #[test]
    fn rework_modal_data_includes_text_input() {
        let card = approval_card();
        let modal = build_rework_modal_data(&card, "modal_rework_v1").expect("modal data");
        assert_eq!(
            modal.get("custom_id").and_then(Value::as_str),
            Some("modal_rework_v1")
        );
        assert_eq!(
            modal.get("title").and_then(Value::as_str),
            Some("Rework feedback")
        );
        let input = modal
            .pointer("/components/0/components/0")
            .expect("text input present");
        assert_eq!(input.get("type").and_then(Value::as_u64), Some(4));
        assert_eq!(
            input.get("custom_id").and_then(Value::as_str),
            Some("reason_input")
        );
        assert_eq!(input.get("style").and_then(Value::as_u64), Some(2));
        assert_eq!(input.get("required").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn rework_modal_data_returns_none_when_no_reason_prompt() {
        let mut card = approval_card();
        card.reason_prompt = None;
        assert!(build_rework_modal_data(&card, "anything").is_none());
    }

    #[test]
    fn wrap_as_modal_response_uses_type_9() {
        let response = wrap_as_modal_response(json!({"ping": true}));
        assert_eq!(response.get("type").and_then(Value::as_u64), Some(9));
    }

    #[test]
    fn build_deferred_update_response_uses_type_6() {
        let response = build_deferred_update_response();
        assert_eq!(response.get("type").and_then(Value::as_u64), Some(6));
    }

    #[test]
    fn clamp_for_field_truncates_with_ellipsis() {
        let long: String = "x".repeat(500);
        let clamped = clamp_for_field(&long, 50);
        assert_eq!(clamped.chars().count(), 50);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn allowed_mentions_blocks_pings() {
        let card = approval_card();
        let payload = build_create_message_payload(&card);
        let parse = payload
            .pointer("/allowed_mentions/parse")
            .and_then(Value::as_array)
            .unwrap();
        assert!(parse.is_empty(), "approval cards must not @-mention");
    }
}
