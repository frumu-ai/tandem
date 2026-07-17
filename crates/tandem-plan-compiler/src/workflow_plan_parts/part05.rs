// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn output_target_is_webhook_event_identifier(prompt: &str, token: &str) -> bool {
    let lowered_prompt = prompt
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| {
            // Keep punctuation inside event names (for example `.`), but remove
            // quoting and grouping delimiters around the identifier.
            !matches!(
                *ch as u32,
                0x22 | 0x27 | 0x28 | 0x29 | 0x2c | 0x3a | 0x3b | 0x5b | 0x5d
                    | 0x60 | 0x7b | 0x7d | 0x2018 | 0x2019 | 0x201c | 0x201d
            )
        })
        .collect::<String>();
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
