use std::borrow::Cow;
use std::pin::Pin;

use futures::Stream;
use tandem_data_boundary::{
    provider_egress_payload_hash, DataBoundaryDetectorConfig, ProviderEgressField,
    ProviderEgressPermit,
};
use tandem_types::{SamplingParams, ToolMode, ToolSchema};
use tokio_util::sync::CancellationToken;

use crate::{ChatAttachment, ChatMessage, ProviderRegistry, StreamChunk};

impl ProviderRegistry {
    /// Dispatch a completion only after the canonical data-boundary evaluator
    /// authorized this exact provider/model route.
    pub async fn complete_with_egress_permit(
        &self,
        permit: &ProviderEgressPermit,
        provider_id: Option<&str>,
        prompt: &str,
        model_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let fields = [ProviderEgressField::transformable("prompt", prompt)];
        permit
            .ensure_request(
                provider_id,
                model_id,
                &provider_egress_payload_hash(&fields),
            )
            .map_err(anyhow::Error::msg)?;
        self.complete_for_provider(provider_id, prompt, model_id)
            .await
    }

    /// Dispatch a streaming chat only after the canonical data-boundary
    /// evaluator authorized this exact provider/model route. A borrowed permit
    /// may be reused for transport retries of the same prepared payload; the
    /// payload must not be rebuilt between attempts.
    #[allow(clippy::too_many_arguments)]
    pub async fn stream_with_egress_permit(
        &self,
        permit: &ProviderEgressPermit,
        provider_id: Option<&str>,
        model_id: Option<&str>,
        messages: Vec<ChatMessage>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        sampling: SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let fields = stream_payload_fields(&messages, tools.as_deref());
        permit
            .ensure_request(
                provider_id,
                model_id,
                &provider_egress_payload_hash(&fields),
            )
            .map_err(anyhow::Error::msg)?;
        self.stream_for_provider(
            provider_id,
            model_id,
            messages,
            tool_mode,
            tools,
            sampling,
            cancel,
        )
        .await
    }
}

fn stream_payload_fields(
    messages: &[ChatMessage],
    tools: Option<&[ToolSchema]>,
) -> Vec<ProviderEgressField<'static>> {
    let mut fields = Vec::new();
    for (message_index, message) in messages.iter().enumerate() {
        if !message.role.is_empty() {
            fields.push(ProviderEgressField::untransformable(
                Cow::Owned(format!("message.{message_index}.role")),
                Cow::Owned(message.role.clone()),
            ));
        }
        fields.push(ProviderEgressField::transformable(
            Cow::Owned(format!("message.{message_index}.content")),
            Cow::Owned(message.content.clone()),
        ));
        for (attachment_index, attachment) in message.attachments.iter().enumerate() {
            let ChatAttachment::ImageUrl { url } = attachment;
            let label = Cow::Owned(format!(
                "message.{message_index}.attachment.{attachment_index}.url"
            ));
            if let Some(prefix_len) = data_url_scan_prefix_len(url) {
                fields.push(ProviderEgressField::untransformable_with_binding(
                    label,
                    Cow::Owned(url[..prefix_len].to_string()),
                    Cow::Owned(url.clone()),
                ));
            } else {
                fields.push(ProviderEgressField::untransformable(
                    label,
                    Cow::Owned(url.clone()),
                ));
            }
        }
    }
    if let Some(tools) = tools {
        if let Ok(value) = serde_json::to_value(tools) {
            append_tool_schema_strings(&value, &mut fields);
        }
    }
    fields
}

fn append_tool_schema_strings(
    value: &serde_json::Value,
    fields: &mut Vec<ProviderEgressField<'static>>,
) {
    match value {
        serde_json::Value::String(value) => {
            let index = fields.len();
            fields.push(ProviderEgressField::untransformable_with_detector_config(
                Cow::Owned(format!("tool_schema.value.{index}")),
                Cow::Owned(value.clone()),
                DataBoundaryDetectorConfig {
                    detect_high_entropy: false,
                    ..DataBoundaryDetectorConfig::default()
                },
            ));
        }
        serde_json::Value::Array(values) => {
            for value in values {
                append_tool_schema_strings(value, fields);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                append_tool_schema_strings(value, fields);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn data_url_scan_prefix_len(value: &str) -> Option<usize> {
    value
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
        .then(|| value.find(',').map(|index| index + 1))
        .flatten()
}
