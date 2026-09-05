use super::*;
use std::sync::atomic::AtomicBool;

struct MutableAuthority {
    revoked: Arc<AtomicBool>,
    initial_checks: Arc<AtomicUsize>,
}

impl ToolPolicyHook for MutableAuthority {
    fn evaluate_tool(
        &self,
        _ctx: ToolPolicyContext,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        self.initial_checks.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: None,
                dispatch_decision: None,
            })
        })
    }

    fn revalidate_session(
        &self,
        _verified: Option<tandem_types::VerifiedTenantContext>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
        let revoked = self.revoked.clone();
        Box::pin(async move {
            anyhow::ensure!(!revoked.load(Ordering::SeqCst), "hosted membership revoked");
            Ok(())
        })
    }
}

struct CountedTool(Arc<AtomicUsize>);

#[async_trait]
impl Tool for CountedTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("counted_tool", "Synthetic effect counter", json!({}))
    }
    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult {
            output: "effect executed".into(),
            metadata: json!({}),
        })
    }
}

#[tokio::test]
async fn hosted_policy_revocation_during_permission_wait_prevents_effect() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(SamplingCaptureProvider {
        captured: Arc::new(Mutex::new(None)),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let session = Session::new(
        Some("revocation during approval".into()),
        Some(temp.path().display().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    engine
        .tools
        .register_tool(
            "counted_tool".into(),
            Arc::new(CountedTool(effects.clone())),
        )
        .await;
    let revoked = Arc::new(AtomicBool::new(false));
    let initial_checks = Arc::new(AtomicUsize::new(0));
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: revoked.clone(),
            initial_checks: initial_checks.clone(),
        }))
        .await;
    let mut events = bus.subscribe();
    let task_engine = engine.clone();
    let task = tokio::spawn(async move {
        task_engine
            .execute_tool_with_permission(
                &session_id,
                "message-1",
                None,
                "counted_tool".into(),
                json!({}),
                Some("call-1".into()),
                None,
                "test approved effect",
                false,
                None,
                CancellationToken::new(),
            )
            .await
    });
    let permission_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.event_type == "permission.asked" {
                break event.properties["requestID"].as_str().unwrap().to_string();
            }
        }
    })
    .await
    .expect("permission barrier reached");
    assert_eq!(initial_checks.load(Ordering::SeqCst), 1);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    revoked.store(true, Ordering::SeqCst);
    assert!(engine.permissions.reply(&permission_id, "once").await);
    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        initial_checks.load(Ordering::SeqCst),
        1,
        "final check must not repeat approval-producing policy"
    );
}

#[tokio::test]
async fn hosted_policy_revocation_prevents_tool_free_provider_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let captured = Arc::new(Mutex::new(None));
    let provider = Arc::new(PostToolCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, _, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let session = Session::new(Some("revoked synthesis".into()), None);
    let session_id = session.id.clone();
    storage.save_session(session).await.unwrap();
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: Arc::new(AtomicBool::new(true)),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    let agent = engine.agents.get(None).await;
    let error = engine
        .generate_final_narrative_without_tools(
            &session_id,
            None,
            &agent,
            Some("scripted-provider-stream"),
            Some("scripted-model"),
            Default::default(),
            CancellationToken::new(),
            &["synthetic tool output".into()],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert!(captured.lock().unwrap().is_none());
}
