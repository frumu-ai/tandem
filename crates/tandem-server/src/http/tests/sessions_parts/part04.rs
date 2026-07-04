// TAN-392: audit-mode data-boundary integration tests. The engine loop reads
// TANDEM_DATA_BOUNDARY_* at dispatch time and EngineConfigReport::from_env
// validates the same vars, so these tests guard the env with an RAII restore
// and share the DEFAULT serial group with the config::engine tests — a named
// group would let the two families race on the same process environment.

struct DataBoundaryEnvGuard {
    name: &'static str,
    previous: Option<String>,
}

impl DataBoundaryEnvGuard {
    fn set(name: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(name).ok();
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
        Self { name, previous }
    }
}

impl Drop for DataBoundaryEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var(self.name, previous),
            None => std::env::remove_var(self.name),
        }
    }
}

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
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("audit"));
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
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
        .find(|event| event.event_type.starts_with("data_boundary."))
        .expect("data_boundary event emitted in audit mode");

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
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", None);
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
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
    let _mode = DataBoundaryEnvGuard::set("TANDEM_DATA_BOUNDARY_MODE", Some("audit"));
    let state = test_state().await;
    let session_id = boundary_test_session(&state).await;
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
            .await;
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
    );
}
