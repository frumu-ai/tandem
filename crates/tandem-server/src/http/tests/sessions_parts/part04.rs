// TAN-392: audit-mode data-boundary integration tests. The engine loop reads
// TANDEM_DATA_BOUNDARY_* at dispatch time, so these tests scope configuration
// through the in-crate override (TAN-684) instead of mutating the process
// environment — env mutation leaked boundary modes into every unannotated
// test running concurrently in the same process. They stay in the DEFAULT
// serial group because the override itself is process-global per key.
use tandem_core::ScopedDataBoundaryConfigOverride as DataBoundaryEnvGuard;

struct BoundaryTextTestProvider;

#[async_trait]
impl Provider for BoundaryTextTestProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "boundary-test".to_string(),
            name: "Boundary Test".to_string(),
            models: vec![ModelInfo {
                id: "boundary-test-1".to_string(),
                provider_id: "boundary-test".to_string(),
                display_name: "Boundary Test 1".to_string(),
                context_window: 32_000,
            }],
        }
    }

    async fn complete(&self, _prompt: &str, _model_override: Option<&str>) -> anyhow::Result<String> {
        Ok("ok".to_string())
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let chunks = vec![
            Ok(StreamChunk::TextDelta("all done".to_string())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ];
        Ok(Box::pin(stream::iter(chunks)))
    }
}

const BOUNDARY_TEST_SECRET: &str = "sk-live-abcdef1234567890";

async fn boundary_test_session(state: &AppState) -> String {
    state
        .providers
        .replace_for_test(
            vec![Arc::new(BoundaryTextTestProvider)],
            Some("boundary-test".to_string()),
        )
        .await;
    let mut session = Session::new(Some("data-boundary".to_string()), Some(".".to_string()));
    session.model = Some(ModelSpec {
        provider_id: "boundary-test".to_string(),
        model_id: "boundary-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save session");
    session_id
}

fn boundary_prompt_request(session_id: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_async"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "parts": [{
                    "type": "text",
                    "text": format!("please use api_key={BOUNDARY_TEST_SECRET} to call the api"),
                }],
                "model": {"provider_id": "boundary-test", "model_id": "boundary-test-1"},
            })
            .to_string(),
        ))
        .expect("prompt request")
}

/// Collects bus events until `session.run.finished`, returning everything
/// seen along the way (including the finish event).
async fn collect_events_until_run_finished(
    rx: &mut tokio::sync::broadcast::Receiver<EngineEvent>,
) -> Vec<EngineEvent> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut events = Vec::new();
        loop {
            let event = rx.recv().await.expect("event");
            let done = event.event_type == "session.run.finished";
            events.push(event);
            if done {
                return events;
            }
        }
    })
    .await
    .expect("run did not finish in time")
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_audit_mode_records_findings_and_allows_provider_call() {
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("audit"));
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    let boundary_event = events
        .iter()
        .find(|event| {
            event.event_type.starts_with("data_boundary.")
                && event.properties["operation"]["kind"] == "provider_request"
        })
        .expect("data_boundary dispatch event emitted in audit mode");

    assert_eq!(boundary_event.event_type, "data_boundary.evaluated");
    assert_eq!(boundary_event.properties["action"], "allow_with_audit");
    assert_eq!(boundary_event.properties["mode"], "audit");
    assert_eq!(boundary_event.properties["auditOnly"], true);
    assert!(
        boundary_event.properties["finding_summary"]["total_findings"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "audit mode must record findings for sensitive content"
    );

    let serialized = serde_json::to_string(&boundary_event.properties).expect("json");
    assert!(
        !serialized.contains(BOUNDARY_TEST_SECRET),
        "boundary event must not leak raw secret: {serialized}"
    );
    assert!(serialized.contains("sha256:"));

    // Audit mode must not have blocked the provider call: the streamed
    // assistant text still went out and the run finished.
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "message.part.updated"),
        "provider call should proceed in audit mode"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_off_mode_emits_no_boundary_events() {
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
    let _mode = DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert!(
        events
            .iter()
            .all(|event| !event.event_type.starts_with("data_boundary.")),
        "config-off mode must not emit data_boundary events"
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "message.part.updated"),
        "provider call should proceed with boundary off"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_bridge_writes_protected_audit_without_raw_content() {
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("audit"));
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    let boundary_event = events
        .iter()
        .find(|event| event.event_type.starts_with("data_boundary."))
        .expect("boundary event");

    let recorded =
        crate::data_boundary_bridge::record_data_boundary_protected_audit(&state, boundary_event)
            .await
            .expect("record protected audit");
    assert!(recorded, "allow_with_audit decisions belong in protected audit");

    let ledger = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit ledger");
    assert!(ledger.contains("data_boundary.evaluated"));
    assert!(ledger.contains("finding_summary"));
    assert!(
        !ledger.contains(BOUNDARY_TEST_SECRET),
        "protected audit must not contain raw secret values"
    );

    // Plain allow decisions (no findings) stay out of the ledger.
    let allow_event = EngineEvent::new(
        "data_boundary.evaluated",
        json!({"action": "allow", "sessionID": session_id}),
    );
    assert!(
        !crate::data_boundary_bridge::record_data_boundary_protected_audit(&state, &allow_event)
            .await
            .expect("plain allow audit decision")
    );
}

/// Records the messages the provider actually received, so enforcement tests
/// can prove what crossed (or never crossed) the boundary.
struct CapturingBoundaryProvider {
    captured: Arc<std::sync::Mutex<Option<Vec<ChatMessage>>>>,
}

#[async_trait]
impl Provider for CapturingBoundaryProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "boundary-test".to_string(),
            name: "Boundary Capture".to_string(),
            models: vec![ModelInfo {
                id: "boundary-test-1".to_string(),
                provider_id: "boundary-test".to_string(),
                display_name: "Boundary Test 1".to_string(),
                context_window: 32_000,
            }],
        }
    }

    async fn complete(&self, _prompt: &str, _model_override: Option<&str>) -> anyhow::Result<String> {
        Ok("ok".to_string())
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        *self.captured.lock().expect("captured lock") = Some(messages);
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta("all done".to_string())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

async fn capturing_boundary_session(
    state: &AppState,
) -> (String, Arc<std::sync::Mutex<Option<Vec<ChatMessage>>>>) {
    let captured = Arc::new(std::sync::Mutex::new(None));
    state
        .providers
        .replace_for_test(
            vec![Arc::new(CapturingBoundaryProvider {
                captured: captured.clone(),
            })],
            Some("boundary-test".to_string()),
        )
        .await;
    let mut session = Session::new(Some("data-boundary".to_string()), Some(".".to_string()));
    session.model = Some(ModelSpec {
        provider_id: "boundary-test".to_string(),
        model_id: "boundary-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state.storage.save_session(session).await.expect("save session");
    (session_id, captured)
}

fn run_finished_status(events: &[EngineEvent]) -> String {
    events
        .iter()
        .find(|event| event.event_type == "session.run.finished")
        .and_then(|event| event.properties.get("status"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_enforce_blocks_sensitive_dispatch_to_unclassified_provider() {
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_eq!(run_finished_status(&events), "error");
    let blocked = events
        .iter()
        .find(|event| event.event_type == "data_boundary.blocked")
        .expect("blocked event");
    assert_eq!(blocked.properties["enforced"], true);
    let serialized = serde_json::to_string(&blocked.properties).expect("json");
    assert!(!serialized.contains(BOUNDARY_TEST_SECRET));
    assert!(
        captured.lock().expect("captured lock").is_none(),
        "provider must never receive a blocked dispatch"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_enforce_redacts_dispatched_payload_for_approved_provider() {
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _classes = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=approved_external"),
    );
    let _redact = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_REDACT_CLASSES",
        Some("credential,pii,secret"),
    );
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state);

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_ne!(
        run_finished_status(&events),
        "error",
        "boundary redaction run failed: {events:#?}"
    );
    assert!(events
        .iter()
        .any(|event| event.event_type == "data_boundary.redacted"));

    let dispatched = captured
        .lock()
        .expect("captured lock")
        .clone()
        .expect("provider called with transformed payload");
    let joined = dispatched
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !joined.contains(BOUNDARY_TEST_SECRET),
        "raw secret must not reach the provider: {joined}"
    );
    assert!(joined.contains("[REDACTED:"));
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_approval_denied_blocks_dispatch() {
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _classes = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=approved_external"),
    );
    let _approval = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES",
        Some("credential"),
    );
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Answer the approval ask as soon as it surfaces.
    let request_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "permission.asked"
                && event.properties["tool"] == "data_boundary_egress"
            {
                let serialized = serde_json::to_string(&event.properties).expect("json");
                assert!(
                    !serialized.contains(BOUNDARY_TEST_SECRET),
                    "approval ask must carry safe evidence only: {serialized}"
                );
                return event.properties["requestID"]
                    .as_str()
                    .expect("request id")
                    .to_string();
            }
        }
    })
    .await
    .expect("permission ask timeout");
    assert!(state.permissions.reply(&request_id, "deny").await);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_eq!(run_finished_status(&events), "error");
    assert!(
        captured.lock().expect("captured lock").is_none(),
        "denied approval must never dispatch the raw payload"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn data_boundary_approval_granted_dispatches_original_payload() {
    let state = test_state().await;
    let (session_id, captured) = capturing_boundary_session(&state).await;
    let _mode =
        DataBoundaryEnvGuard::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", Some("enforce"));
    let _classes = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
        Some("boundary-test=approved_external"),
    );
    let _approval = DataBoundaryEnvGuard::set(
        &session_id,
        "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES",
        Some("credential"),
    );
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let resp = app
        .oneshot(boundary_prompt_request(&session_id))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let request_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "permission.asked"
                && event.properties["tool"] == "data_boundary_egress"
            {
                return event.properties["requestID"]
                    .as_str()
                    .expect("request id")
                    .to_string();
            }
        }
    })
    .await
    .expect("permission ask timeout");
    assert!(state.permissions.reply(&request_id, "once").await);

    let events = collect_events_until_run_finished(&mut rx).await;
    assert_ne!(run_finished_status(&events), "error");
    assert!(
        captured.lock().expect("captured lock").is_some(),
        "explicit approval dispatches the payload"
    );
}
