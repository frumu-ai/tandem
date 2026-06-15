use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

use tandem_types::{EngineEvent, RuntimeEvent, RuntimeEventEnvelope};

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<EngineEvent>,
    session_part_tx: mpsc::UnboundedSender<EngineEvent>,
    session_part_rx: std::sync::Arc<Mutex<Option<mpsc::UnboundedReceiver<EngineEvent>>>>,
    seq: Arc<AtomicU64>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(2048);
        let (session_part_tx, session_part_rx) = mpsc::unbounded_channel();
        Self {
            tx,
            session_part_tx,
            session_part_rx: std::sync::Arc::new(Mutex::new(Some(session_part_rx))),
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.tx.subscribe()
    }

    /// Number of live broadcast subscribers. Useful for diagnostics and for tests
    /// that must wait until a spawned consumer has subscribed before publishing
    /// (the broadcast channel drops events sent before a receiver exists).
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    pub fn take_session_part_receiver(&self) -> Option<mpsc::UnboundedReceiver<EngineEvent>> {
        self.session_part_rx.lock().ok()?.take()
    }

    /// Publish an event, stamping the canonical [`RuntimeEventEnvelope`]
    /// (event id, monotonic seq, schema version, occurred-at, correlation
    /// ids) when the emitter did not provide one. Centralizing the stamp
    /// here means every emitter publishes the canonical envelope without
    /// each call site changing.
    pub fn publish(&self, event: EngineEvent) {
        let event = self.stamp_envelope(event);
        if should_enqueue_session_part_event(&event) {
            let _ = self.session_part_tx.send(event.clone());
        }
        let _ = self.tx.send(event);
    }

    /// Publish a canonical [`RuntimeEvent`] directly. The envelope's `seq`
    /// is reassigned by the bus to preserve publish-order monotonicity, and
    /// a missing (zero) `occurred_at_ms` is stamped with publish time.
    pub fn publish_runtime(&self, event: RuntimeEvent) {
        let mut engine_event = event.to_engine_event();
        if let Some(envelope) = engine_event.envelope.as_mut() {
            envelope.seq = self.next_seq();
            if envelope.occurred_at_ms == 0 {
                envelope.occurred_at_ms = now_ms();
            }
        }
        self.publish(engine_event);
    }

    fn stamp_envelope(&self, mut event: EngineEvent) -> EngineEvent {
        if event.envelope.is_none() {
            event.envelope = Some(RuntimeEventEnvelope::derive(
                self.next_seq(),
                now_ms(),
                &event.properties,
            ));
        }
        event
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn should_enqueue_session_part_event(event: &EngineEvent) -> bool {
    if event.event_type != "message.part.updated" {
        return false;
    }
    let Some(part) = event.properties.get("part").and_then(Value::as_object) else {
        return false;
    };
    let part_type = part
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_tool_part = matches!(
        part_type.as_str(),
        "tool" | "tool-invocation" | "tool-result" | "tool_invocation" | "tool_result"
    );
    if !is_tool_part {
        return false;
    }

    let part_state = part
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_result = part.get("result").is_some_and(|value| !value.is_null());
    let has_error = part
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());

    // Streaming write and tool-call deltas can be extremely noisy.
    // Keep only actionable updates for persistence.
    !(part_state == "running" && !has_result && !has_error)
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters_running_tool_delta_events_for_session_part_queue() {
        let event = EngineEvent::new(
            "message.part.updated",
            json!({
                "part": {
                    "type": "tool",
                    "state": "running",
                    "tool": "write",
                    "args": {"path":"out.md","content":"hello"}
                }
            }),
        );
        assert!(!should_enqueue_session_part_event(&event));
    }

    #[test]
    fn keeps_completed_tool_events_for_session_part_queue() {
        let event = EngineEvent::new(
            "message.part.updated",
            json!({
                "part": {
                    "type": "tool",
                    "state": "completed",
                    "tool": "write",
                    "args": {"path":"out.md","content":"hello"},
                    "result": {"ok": true}
                }
            }),
        );
        assert!(should_enqueue_session_part_event(&event));
    }

    #[test]
    fn supports_snake_case_tool_part_types_for_session_queue() {
        let event = EngineEvent::new(
            "message.part.updated",
            json!({
                "part": {
                    "type": "tool_invocation",
                    "state": "completed",
                    "tool": "websearch",
                    "result": {"results":[{"url":"https://example.com"}]}
                }
            }),
        );
        assert!(should_enqueue_session_part_event(&event));
    }

    #[tokio::test]
    async fn publish_stamps_canonical_envelope_with_monotonic_seq() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(EngineEvent::new(
            "session.run.started",
            json!({"sessionID": "ses_1", "runID": "run_1"}),
        ));
        bus.publish(EngineEvent::new(
            "session.run.finished",
            json!({"session_id": "ses_1", "run_id": "run_1"}),
        ));

        let first = rx.recv().await.expect("first event");
        let second = rx.recv().await.expect("second event");
        let first_envelope = first.envelope.expect("first envelope");
        let second_envelope = second.envelope.expect("second envelope");

        assert_eq!(
            first_envelope.schema_version,
            tandem_types::RUNTIME_EVENT_SCHEMA_VERSION
        );
        assert!(!first_envelope.event_id.is_empty());
        assert_ne!(first_envelope.event_id, second_envelope.event_id);
        assert!(second_envelope.seq > first_envelope.seq);
        assert!(first_envelope.occurred_at_ms > 0);
        // Correlation ids are extracted regardless of key spelling.
        assert_eq!(first_envelope.session_id.as_deref(), Some("ses_1"));
        assert_eq!(second_envelope.session_id.as_deref(), Some("ses_1"));
        assert_eq!(second_envelope.run_id.as_deref(), Some("run_1"));
    }

    #[tokio::test]
    async fn publish_preserves_emitter_provided_envelope() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        let envelope = RuntimeEventEnvelope::derive(99, 123, &json!({"sessionID": "ses_1"}));
        bus.publish(
            EngineEvent::new("session.updated", json!({"sessionID": "ses_1"}))
                .with_envelope(envelope.clone()),
        );

        let received = rx.recv().await.expect("event");
        assert_eq!(received.envelope, Some(envelope));
    }

    #[tokio::test]
    async fn publish_runtime_assigns_bus_sequence() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        // Consume one seq so the runtime event cannot accidentally keep it.
        bus.publish(EngineEvent::new("server.connected", json!({})));
        let _ = rx.recv().await.expect("warmup event");

        let runtime_event = RuntimeEvent::from_engine_event(&EngineEvent::new(
            "automation_v2.run.failed",
            json!({"run_id": "run_1", "node_id": "node_1"}),
        ))
        .expect("canonical event");
        bus.publish_runtime(runtime_event);

        let received = rx.recv().await.expect("runtime event");
        assert_eq!(received.event_type, "automation_v2.run.failed");
        let envelope = received.envelope.expect("envelope");
        assert!(envelope.seq >= 2, "bus reassigns seq, got {}", envelope.seq);
        assert!(
            envelope.occurred_at_ms > 0,
            "bus stamps publish time when the envelope carries none"
        );
        assert_eq!(envelope.run_id.as_deref(), Some("run_1"));
        assert_eq!(envelope.node_id.as_deref(), Some("node_1"));
    }
}
