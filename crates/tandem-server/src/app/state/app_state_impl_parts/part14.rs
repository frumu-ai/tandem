// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Channel status reporting for AppState.
//
// Split out of part01 to keep that file within the repository's per-file line
// budget. Included into `app/state/mod.rs`, so it shares that module's imports.

impl AppState {
    pub async fn channel_statuses(&self) -> std::collections::HashMap<String, ChannelStatus> {
        let runtime = self.channels_runtime.lock().await;
        let mut status = runtime.statuses.clone();
        let diagnostics = runtime.diagnostics.read().await;
        for spec in registered_channels() {
            let entry = status
                .entry(spec.name.to_string())
                .or_insert(ChannelStatus {
                    enabled: false,
                    connected: false,
                    last_error: None,
                    active_sessions: 0,
                    meta: json!({}),
                });
            let mut meta = entry.meta.as_object().cloned().unwrap_or_default();
            if let Some(diag) = diagnostics.get(spec.name) {
                entry.last_error = diag.last_error.clone().or_else(|| entry.last_error.clone());
                // Reflect live listener liveness rather than the boot-time
                // snapshot: a channel is connected when it's enabled and its
                // supervised listener is actually running. Downstream provider
                // failures (e.g. an expired model token) do not flip this —
                // the channel connection itself is still healthy (TAN-597).
                entry.connected = entry.enabled && diag.state == "running";
                meta.insert("state".to_string(), Value::String(diag.state.to_string()));
                meta.insert(
                    "last_error_code".to_string(),
                    diag.last_error_code
                        .map(|code| Value::String(code.to_string()))
                        .unwrap_or(Value::Null),
                );
                meta.insert(
                    "last_reconnect_at".to_string(),
                    diag.last_reconnect_at
                        .map(|value| Value::Number(value.into()))
                        .unwrap_or(Value::Null),
                );
                meta.insert(
                    "listener_start_count".to_string(),
                    Value::Number(serde_json::Number::from(diag.listener_start_count)),
                );
            } else {
                // No live diagnostic yet: we cannot confirm the listener is
                // running, so do not report it as connected (TAN-597).
                entry.connected = false;
                meta.insert("state".to_string(), Value::String("stopped".to_string()));
                meta.insert("last_error_code".to_string(), Value::Null);
                meta.insert("last_reconnect_at".to_string(), Value::Null);
                meta.insert(
                    "listener_start_count".to_string(),
                    Value::Number(0u64.into()),
                );
            }
            entry.meta = Value::Object(meta);
        }
        status
    }
}
