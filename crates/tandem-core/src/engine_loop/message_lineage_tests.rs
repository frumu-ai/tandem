use super::super::*;
use super::*;
use std::sync::Arc;
use tandem_types::{
    AuthorityChain, HumanActor, RequestPrincipal, Session, TenantContextAssertionClaims,
};

fn identity() -> VerifiedTenantContext {
    TenantContextAssertionClaims::new_v1(
        "lineage-test",
        "lineage-runtime",
        1,
        u64::MAX,
        "native-assertion",
        TenantContext::explicit_user_workspace("org", "workspace", None, "alice"),
        HumanActor::tandem_user("alice"),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(
            "alice",
            "lineage-test",
        )),
        Vec::new(),
    )
    .into()
}

fn source(id: &str) -> MemorySourceReference {
    MemorySourceReference {
        memory_id: id.into(),
        content_hash: "c".repeat(64),
        restriction_digest: "d".repeat(64),
    }
}

fn text(role: MessageRole, body: &str) -> Message {
    Message::new(role, vec![MessagePart::Text { text: body.into() }])
}

fn previous_assistant() -> Message {
    let mut message = text(MessageRole::Assistant, "prior canonical response");
    message.source_lineage = Some(NativeMessageLineage {
        schema_version: 1,
        run_id: "prior-run".into(),
        tenant_context: identity().tenant_context,
        subject: "alice".into(),
        message_digest: canonical_message_digest(&message),
        input_message_ids: vec!["ancestor-input".into()],
        included_memory: vec![source("prior-source")],
        complete: true,
    });
    message
}

#[test]
fn accepted_input_inherits_valid_canonical_assistant_sources_transitively() {
    let user = text(MessageRole::User, "canonical private input");
    let assistant = previous_assistant();
    let mut accumulator = MessageLineageAccumulator::new(Some(&identity()), Some("current-run"));
    accumulator.accept_provider_input(
        &[user.clone(), assistant.clone()],
        &[source("current-source")],
        true,
    );
    accumulator.accept_provider_input(&[assistant.clone()], &[source("current-source")], true);
    let lineage = accumulator.into_lineage().unwrap();
    assert!(lineage.complete);
    assert_eq!(lineage.subject, "alice");
    assert_eq!(lineage.tenant_context, identity().tenant_context);
    assert!(lineage.input_message_ids.contains(&user.id));
    assert!(lineage.input_message_ids.contains(&assistant.id));
    assert!(lineage.input_message_ids.contains(&"ancestor-input".into()));
    assert_eq!(
        lineage.included_memory,
        vec![source("prior-source"), source("current-source")]
    );
}

#[test]
fn modified_or_foreign_assistant_manifest_is_not_complete_or_inherited() {
    for corruption in 0..3 {
        let mut message = previous_assistant();
        match corruption {
            0 => message.parts.push(MessagePart::Text {
                text: "modified body".into(),
            }),
            1 => message.source_lineage.as_mut().unwrap().schema_version = 2,
            _ => message.source_lineage.as_mut().unwrap().message_digest = "forged-digest".into(),
        }
        let mut accumulator =
            MessageLineageAccumulator::new(Some(&identity()), Some("current-run"));
        accumulator.accept_provider_input(&[message], &[], true);
        let lineage = accumulator.into_lineage().unwrap();
        assert!(!lineage.complete);
        assert!(lineage.included_memory.is_empty());
    }
    for foreign_tenant in [false, true] {
        let mut message = previous_assistant();
        let lineage = message.source_lineage.as_mut().unwrap();
        if foreign_tenant {
            lineage.tenant_context.workspace_id = "foreign-workspace".into();
        } else {
            lineage.subject = "bob".into();
        }
        let mut accumulator =
            MessageLineageAccumulator::new(Some(&identity()), Some("current-run"));
        accumulator.accept_provider_input(&[message], &[], true);
        let lineage = accumulator.into_lineage().unwrap();
        assert!(!lineage.complete);
        assert!(lineage.included_memory.is_empty());
    }
}

#[test]
fn legacy_assistant_unknown_tools_and_incomplete_hooks_keep_private_floor() {
    let tool = Message::new(
        MessageRole::User,
        vec![MessagePart::ToolInvocation {
            tool: "unknown_tool".into(),
            args: json!({}),
            result: Some(json!("unclassified result")),
            error: None,
        }],
    );
    for message in [
        text(MessageRole::Assistant, "legacy response"),
        text(MessageRole::System, "unknown context"),
        tool,
    ] {
        let mut accumulator =
            MessageLineageAccumulator::new(Some(&identity()), Some("current-run"));
        accumulator.accept_provider_input(&[message], &[], true);
        assert!(!accumulator.into_lineage().unwrap().complete);
    }
    let mut accumulator = MessageLineageAccumulator::new(Some(&identity()), Some("current-run"));
    accumulator.accept_provider_input(&[text(MessageRole::User, "private text")], &[], false);
    assert!(!accumulator.into_lineage().unwrap().complete);
    assert!(MessageLineageAccumulator::new(None, Some("run"))
        .into_lineage()
        .is_none());
}

#[test]
fn unaccepted_or_malformed_source_input_cannot_claim_completeness() {
    assert!(
        !MessageLineageAccumulator::new(Some(&identity()), Some("run"))
            .into_lineage()
            .unwrap()
            .complete
    );
    let mut malformed = source("source");
    malformed.restriction_digest = "not-a-native-digest".into();
    let mut accumulator = MessageLineageAccumulator::new(Some(&identity()), Some("run"));
    accumulator.accept_provider_input(
        &[text(MessageRole::User, "canonical input")],
        &[malformed],
        true,
    );
    let lineage = accumulator.into_lineage().unwrap();
    assert!(!lineage.complete);
    assert!(lineage.included_memory.is_empty());
}

#[test]
fn lineage_keeps_original_verified_subject_and_tenant_after_context_changes() {
    let mut verified = identity();
    let original_tenant = verified.tenant_context.clone();
    let mut accumulator = MessageLineageAccumulator::new(Some(&verified), Some("original-run"));
    verified.tenant_context.actor_id = Some("bob".into());
    verified.tenant_context.workspace_id = "changed-workspace".into();
    accumulator.accept_provider_input(
        &[text(MessageRole::User, "original canonical input")],
        &[],
        true,
    );
    let lineage = accumulator.into_lineage().unwrap();
    assert!(lineage.complete);
    assert_eq!(lineage.subject, "alice");
    assert_eq!(lineage.tenant_context, original_tenant);
    assert_ne!(lineage.tenant_context, verified.tenant_context);
    assert_eq!(lineage.run_id, "original-run");
}

#[tokio::test]
async fn compact_history_tracks_only_actual_selected_native_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(Storage::new(dir.path()).await.unwrap());
    let mut session = Session::new(Some("native history".into()), None);
    session.messages = (0..15)
        .map(|index| {
            text(
                MessageRole::User,
                &format!("ordinary canonical turn {index}"),
            )
        })
        .collect();
    let expected = session.messages[3..]
        .iter()
        .map(|message| message.id.clone())
        .collect::<Vec<_>>();
    storage.save_session(session.clone()).await.unwrap();
    let loaded = prompt_runtime::load_chat_history(
        storage,
        &session.id,
        prompt_runtime::ChatHistoryProfile::Compact,
    )
    .await;
    assert_eq!(loaded.dropped_messages, 3);
    assert_eq!(loaded.source_message_ids, expected);
    assert_eq!(
        loaded
            .canonical_messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(loaded.messages[0]
        .content
        .starts_with("[history compacted:"));
}

struct LineageProvider {
    deny: bool,
}

#[async_trait::async_trait]
impl tandem_providers::Provider for LineageProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "lineage-provider".into(),
            name: "Native lineage test".into(),
            models: vec![tandem_types::ModelInfo {
                id: "lineage-model".into(),
                provider_id: "lineage-provider".into(),
                display_name: "Native lineage model".into(),
                context_window: 8192,
            }],
        }
    }
    async fn complete(&self, _prompt: &str, _model: Option<&str>) -> anyhow::Result<String> {
        anyhow::bail!("unexpected completion path")
    }
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model: Option<&str>,
        _mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<
        std::pin::Pin<Box<dyn futures::Stream<Item = anyhow::Result<StreamChunk>> + Send>>,
    > {
        assert!(messages
            .iter()
            .any(|message| message.content == "native memory source evidence"));
        if self.deny {
            anyhow::bail!("authentication failed for native lineage fixture");
        }
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamChunk::TextDelta("native lineage answer".into())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".into(),
                usage: None,
            }),
        ])))
    }
}

struct LineageHook;
impl PromptContextHook for LineageHook {
    fn augment_provider_messages(
        &self,
        _ctx: PromptContextHookContext,
        mut messages: Vec<ChatMessage>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<PromptContextHookResult>> {
        Box::pin(async move {
            messages.push(ChatMessage {
                role: "system".into(),
                content: "native memory source evidence".into(),
                attachments: Vec::new(),
            });
            Ok(
                PromptContextHookResult::new(messages, PromptContextHookStats::default())
                    .with_memory_lineage(vec![source("native-source")], true),
            )
        })
    }
}

async fn engine(base: &std::path::Path, deny: bool) -> (EngineLoop, Arc<Storage>) {
    let storage = Arc::new(Storage::new(base).await.unwrap());
    let bus = EventBus::new();
    let providers = tandem_providers::ProviderRegistry::new(tandem_providers::AppConfig::default());
    providers
        .replace_for_test(
            vec![Arc::new(LineageProvider { deny })],
            Some("lineage-provider".into()),
        )
        .await;
    let engine = EngineLoop::new(
        storage.clone(),
        bus.clone(),
        providers,
        PluginRegistry::new(base).await.unwrap(),
        AgentRegistry::new(base).await.unwrap(),
        PermissionManager::new(bus),
        ToolRegistry::new(),
        CancellationRegistry::new(),
        tandem_types::HostRuntimeContext {
            os: tandem_types::HostOs::Linux,
            arch: "fixture".into(),
            shell_family: tandem_types::ShellFamily::Posix,
            path_style: tandem_types::PathStyle::Posix,
        },
    );
    engine.set_prompt_context_hook(Arc::new(LineageHook)).await;
    (engine, storage)
}

fn request() -> SendMessageRequest {
    serde_json::from_value(json!({"parts": [{"type": "text", "text": "Please summarize this evidence in one sentence."}],
        "model": {"providerID": "lineage-provider", "modelID": "lineage-model"}, "toolMode": "none", "contextMode": "full"})).unwrap()
}

#[tokio::test]
async fn real_engine_final_lineage_survives_native_header_storage_and_cold_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, storage) = engine(dir.path(), false).await;
    let mut session = Session::new(Some("lineage fixture".into()), None);
    session.tenant_context = identity().tenant_context;
    session.verified_tenant_context = Some(identity());
    let id = session.id.clone();
    storage.save_session(session).await.unwrap();
    let _boundary = ScopedDataBoundaryConfigOverride::set(&id, "TANDEM_DATA_BOUNDARY_MODE", None);
    engine
        .run_prompt_async_with_execution_context(
            id.clone(),
            request(),
            None,
            Some("actual-native-run".into()),
            Vec::new(),
        )
        .await
        .unwrap();
    let stored = storage.get_session(&id).await.unwrap();
    let user = stored
        .messages
        .iter()
        .find(|message| matches!(message.role, MessageRole::User))
        .unwrap();
    let assistant = stored
        .messages
        .iter()
        .find(|message| matches!(message.role, MessageRole::Assistant))
        .unwrap();
    let lineage = assistant.source_lineage.as_ref().unwrap();
    assert!(lineage.complete);
    assert_eq!(lineage.run_id, "actual-native-run");
    assert_eq!(lineage.subject, "alice");
    assert_eq!(lineage.tenant_context, identity().tenant_context);
    assert_eq!(lineage.message_digest, canonical_message_digest(assistant));
    assert_eq!(lineage.input_message_ids, vec![user.id.clone()]);
    assert_eq!(lineage.included_memory, vec![source("native-source")]);
    assert!(user.source_lineage.is_none());
    let assistant_id = assistant.id.clone();
    let digest = lineage.message_digest.clone();
    drop(engine);
    drop(storage);
    let reopened = Storage::new(dir.path()).await.unwrap();
    let restored = reopened.get_session(&id).await.unwrap();
    let assistant = restored
        .messages
        .iter()
        .find(|message| message.id == assistant_id)
        .unwrap();
    assert_eq!(
        assistant.source_lineage.as_ref().unwrap().message_digest,
        digest
    );
    assert_eq!(canonical_message_digest(assistant), digest);
    assert_eq!(
        assistant.source_lineage.as_ref().unwrap().included_memory,
        vec![source("native-source")]
    );
}

#[tokio::test]
async fn rejected_provider_input_does_not_persist_a_prepared_source_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, storage) = engine(dir.path(), true).await;
    let mut session = Session::new(Some("rejected lineage fixture".into()), None);
    session.tenant_context = identity().tenant_context;
    session.verified_tenant_context = Some(identity());
    let id = session.id.clone();
    storage.save_session(session).await.unwrap();
    let _boundary = ScopedDataBoundaryConfigOverride::set(&id, "TANDEM_DATA_BOUNDARY_MODE", None);
    assert!(engine
        .run_prompt_async_with_execution_context(
            id.clone(),
            request(),
            None,
            Some("rejected-native-run".into()),
            Vec::new()
        )
        .await
        .is_err());
    let stored = storage.get_session(&id).await.unwrap();
    assert!(stored
        .messages
        .iter()
        .all(|message| message.source_lineage.is_none()));
    assert!(!stored
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::Assistant)
            && message.parts.iter().any(
                |part| matches!(part, MessagePart::Text { text } if text == "native lineage answer")
            )));
}
