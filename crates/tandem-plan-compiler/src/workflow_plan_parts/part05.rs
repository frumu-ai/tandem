// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn output_target_is_webhook_event_identifier(prompt: &str, token: &str) -> bool {
    let lowered_prompt = prompt.to_ascii_lowercase();
    let lowered_token = token.to_ascii_lowercase();
    [
        format!("webhook event named {lowered_token}"),
        format!("webhook event name {lowered_token}"),
        format!("webhook event type {lowered_token}"),
        format!("webhook event kind {lowered_token}"),
        format!("event named {lowered_token}"),
        format!("event name {lowered_token}"),
        format!("event type {lowered_token}"),
        format!("event kind {lowered_token}"),
    ]
    .iter()
    .any(|pattern| lowered_prompt.contains(pattern))
}
