pub mod discord;
pub mod slack;
pub mod telegram;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use tandem_channels::traits::{
    Channel, InteractiveCard, InteractiveCardButton, InteractiveCardButtonStyle,
    InteractiveCardField, InteractiveCardReasonPrompt,
};
use tandem_types::{ApprovalDecision, ApprovalRequest};

use crate::app::approval_outbound::{ApprovalNotifier, NotifierError};
use crate::app::state::approval_message_map::ApprovalMessageMap;

pub(crate) fn approval_request_to_card(
    request: &ApprovalRequest,
    recipient: String,
) -> InteractiveCard {
    let workflow = request
        .workflow_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Tandem workflow");
    let action = request
        .action_kind
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("approval gate");
    let title = format!("{workflow}: {action}");

    let mut body_parts = Vec::new();
    if let Some(instructions) = request
        .instructions
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body_parts.push(instructions.to_string());
    }
    if let Some(preview) = request
        .action_preview_markdown
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body_parts.push(preview.to_string());
    }
    let body_markdown = if body_parts.is_empty() {
        "A human approval is required before this run can continue.".to_string()
    } else {
        body_parts.join("\n\n")
    };

    let mut fields = vec![
        InteractiveCardField {
            label: "Run".to_string(),
            value: request.run_id.clone(),
        },
        InteractiveCardField {
            label: "Source".to_string(),
            value: format!("{:?}", request.source),
        },
        InteractiveCardField {
            label: "Workspace".to_string(),
            value: request.tenant.workspace_id.clone(),
        },
    ];
    if let Some(node_id) = request.node_id.as_ref().filter(|value| !value.is_empty()) {
        fields.push(InteractiveCardField {
            label: "Node".to_string(),
            value: node_id.clone(),
        });
    }

    let decisions = if request.decisions.is_empty() {
        vec![
            ApprovalDecision::Approve,
            ApprovalDecision::Rework,
            ApprovalDecision::Cancel,
        ]
    } else {
        request.decisions.clone()
    };
    let buttons = decisions
        .iter()
        .map(button_for_decision)
        .collect::<Vec<_>>();

    let mut correlation = json!({
        "request_id": request.request_id,
        "source": request.source,
        "automation_v2_run_id": request.run_id,
        "run_id": request.run_id,
        "node_id": request.node_id,
    });
    if let Some(payload) = request.surface_payload.as_ref() {
        if let Some(obj) = correlation.as_object_mut() {
            obj.insert("surface_payload".to_string(), payload.clone());
        }
    }

    InteractiveCard {
        recipient,
        title,
        body_markdown,
        fields,
        buttons,
        reason_prompt: Some(InteractiveCardReasonPrompt {
            modal_title: "Request rework".to_string(),
            field_label: "What should change before this can be approved?".to_string(),
            field_placeholder: Some("Add the feedback the workflow should use.".to_string()),
            submit_label: "Send rework".to_string(),
        }),
        thread_key: Some(request.run_id.clone()),
        correlation,
    }
}

fn button_for_decision(decision: &ApprovalDecision) -> InteractiveCardButton {
    match decision {
        ApprovalDecision::Approve => InteractiveCardButton {
            action_id: "approve".to_string(),
            label: "Approve".to_string(),
            style: InteractiveCardButtonStyle::Primary,
            requires_reason: false,
            confirm: None,
        },
        ApprovalDecision::Rework => InteractiveCardButton {
            action_id: "rework".to_string(),
            label: "Rework".to_string(),
            style: InteractiveCardButtonStyle::Default,
            requires_reason: true,
            confirm: None,
        },
        ApprovalDecision::Cancel => InteractiveCardButton {
            action_id: "cancel".to_string(),
            label: "Cancel".to_string(),
            style: InteractiveCardButtonStyle::Destructive,
            requires_reason: false,
            confirm: None,
        },
    }
}

pub struct ChannelApprovalNotifier {
    name: &'static str,
    recipient: String,
    channel: Arc<dyn Channel>,
    message_map: Option<Arc<ApprovalMessageMap>>,
}

impl ChannelApprovalNotifier {
    pub fn new(
        name: &'static str,
        recipient: impl Into<String>,
        channel: Arc<dyn Channel>,
    ) -> Self {
        Self::new_with_message_map(name, recipient, channel, None)
    }

    pub fn new_with_message_map(
        name: &'static str,
        recipient: impl Into<String>,
        channel: Arc<dyn Channel>,
        message_map: Option<Arc<ApprovalMessageMap>>,
    ) -> Self {
        Self {
            name,
            recipient: recipient.into(),
            channel,
            message_map,
        }
    }
}

#[async_trait]
impl ApprovalNotifier for ChannelApprovalNotifier {
    fn name(&self) -> &str {
        self.name
    }

    async fn notify(&self, request: &ApprovalRequest) -> Result<(), NotifierError> {
        if !self.channel.supports_interactive_cards() {
            return Err(NotifierError::Permanent(format!(
                "{} channel does not support interactive cards",
                self.channel.name()
            )));
        }

        let mut card = approval_request_to_card(request, self.recipient.clone());
        if self.name == "telegram" {
            let callback_id = telegram_callback_id(&request.request_id);
            if let Some(obj) = card.correlation.as_object_mut() {
                obj.insert("telegram_callback_id".to_string(), json!(callback_id));
            }
        }
        let sent = self
            .channel
            .send_card(&card)
            .await
            .map_err(|err| NotifierError::Transient(err.to_string()))?;
        if let Some(message_map) = self.message_map.as_ref() {
            if self.name == "telegram" {
                let callback_id = telegram_callback_id(&request.request_id);
                message_map
                    .record_telegram_callback(callback_id, request, self.recipient.clone())
                    .await
                    .map_err(|err| {
                        NotifierError::Transient(format!(
                            "failed to persist Telegram callback mapping: {err}"
                        ))
                    })?;
            }
            message_map
                .record_approval_sent(request, sent)
                .await
                .map_err(|err| {
                    NotifierError::Transient(format!("failed to persist approval message: {err}"))
                })?;
        }
        Ok(())
    }
}

fn telegram_callback_id(request_id: &str) -> String {
    let digest = Sha256::digest(request_id.as_bytes());
    format!(
        "tgcb_{:016x}",
        u64::from_be_bytes(digest[0..8].try_into().unwrap())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_channels::traits::{ChannelMessage, InteractiveCardError, InteractiveCardSent};
    use tandem_types::{ApprovalSourceKind, ApprovalTenantRef};

    fn fake_request() -> ApprovalRequest {
        ApprovalRequest {
            request_id: "automation_v2:run-1:send_email".to_string(),
            source: ApprovalSourceKind::AutomationV2,
            tenant: ApprovalTenantRef {
                org_id: "org".to_string(),
                workspace_id: "workspace".to_string(),
                user_id: None,
            },
            run_id: "run-1".to_string(),
            node_id: Some("send_email".to_string()),
            workflow_name: Some("Sales outreach".to_string()),
            action_kind: Some("send email".to_string()),
            action_preview_markdown: Some("Email alice@example.com".to_string()),
            surface_payload: None,
            requested_at_ms: 1,
            expires_at_ms: None,
            decisions: vec![
                ApprovalDecision::Approve,
                ApprovalDecision::Rework,
                ApprovalDecision::Cancel,
            ],
            rework_targets: vec![],
            instructions: Some("Check the recipient and tone.".to_string()),
            decided_by: None,
            decided_at_ms: None,
            decision: None,
            rework_feedback: None,
        }
    }

    struct FakeChannel {
        supports_cards: bool,
        seen: std::sync::Mutex<Vec<InteractiveCard>>,
    }

    #[async_trait]
    impl Channel for FakeChannel {
        fn name(&self) -> &str {
            "fake"
        }

        async fn send(
            &self,
            _message: &tandem_channels::traits::SendMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_card(
            &self,
            card: &InteractiveCard,
        ) -> Result<InteractiveCardSent, InteractiveCardError> {
            self.seen.lock().unwrap().push(card.clone());
            Ok(InteractiveCardSent {
                channel: "fake".to_string(),
                message_id: "msg-1".to_string(),
                recipient: card.recipient.clone(),
                thread_id: card.thread_key.clone(),
            })
        }

        fn supports_interactive_cards(&self) -> bool {
            self.supports_cards
        }
    }

    #[test]
    fn approval_request_to_card_preserves_core_identity() {
        let card = approval_request_to_card(&fake_request(), "C123".to_string());

        assert_eq!(card.recipient, "C123");
        assert_eq!(card.title, "Sales outreach: send email");
        assert!(card.body_markdown.contains("alice@example.com"));
        assert_eq!(card.thread_key.as_deref(), Some("run-1"));
        assert_eq!(card.buttons.len(), 3);
        assert_eq!(
            card.correlation["request_id"],
            "automation_v2:run-1:send_email"
        );
        assert_eq!(card.correlation["automation_v2_run_id"], "run-1");
        assert_eq!(card.correlation["run_id"], "run-1");
        assert_eq!(card.correlation["node_id"], "send_email");
    }

    #[tokio::test]
    async fn channel_approval_notifier_sends_interactive_card() {
        let channel = Arc::new(FakeChannel {
            supports_cards: true,
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let notifier = ChannelApprovalNotifier::new("fake", "C123", channel.clone());

        notifier.notify(&fake_request()).await.unwrap();

        let seen = channel.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].recipient, "C123");
    }

    #[tokio::test]
    async fn channel_approval_notifier_records_sent_message() {
        let channel = Arc::new(FakeChannel {
            supports_cards: true,
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let message_map = Arc::new(ApprovalMessageMap::ephemeral());
        let notifier = ChannelApprovalNotifier::new_with_message_map(
            "fake",
            "C123",
            channel,
            Some(message_map.clone()),
        );

        let request = fake_request();
        notifier.notify(&request).await.unwrap();

        let record = message_map.get(&request.request_id).await.unwrap();
        assert_eq!(record.channel, "fake");
        assert_eq!(record.recipient, "C123");
        assert_eq!(record.message_id, "msg-1");
        let thread = message_map
            .get_thread_for_run(&request.run_id)
            .await
            .unwrap();
        assert_eq!(thread.message_id, "msg-1");
    }

    #[tokio::test]
    async fn channel_approval_notifier_rejects_non_interactive_channel() {
        let channel = Arc::new(FakeChannel {
            supports_cards: false,
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let notifier = ChannelApprovalNotifier::new("fake", "C123", channel);

        let err = notifier.notify(&fake_request()).await.unwrap_err();
        assert!(matches!(err, NotifierError::Permanent(_)));
    }
}
