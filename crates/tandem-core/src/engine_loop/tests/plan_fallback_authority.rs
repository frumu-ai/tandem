use super::*;
use futures::future::BoxFuture;
use rusqlite::{Connection, OptionalExtension};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, mpsc};
use std::time::Duration;
use tandem_types::{EngineEvent, VerifiedTenantContext};
use tokio::sync::{broadcast, oneshot};

const RUN_ID: &str = "held-plan-fallback-run";
const TODO_CONTENT: &str = "Prepare the synthetic planning deliverable";
const TODO_COMPLETION: &str = "- [ ] Prepare the synthetic planning deliverable";
const QUESTION_COMPLETION: &str =
    "I need more information before I can prepare a concrete task list.";
const REVOKED: &str = "original planning authority revoked";
const ORIGINAL_DENIED: &str = "original planning assertion denied despite renewed stored authority";

#[derive(Clone, Copy, Debug)]
enum Fallback {
    Todo,
    Question,
}

impl Fallback {
    fn completion(self) -> &'static str {
        match self {
            Self::Todo => TODO_COMPLETION,
            Self::Question => QUESTION_COMPLETION,
        }
    }

    fn present(self, canonical: &CanonicalState) -> bool {
        match self {
            Self::Todo => canonical
                .todos
                .iter()
                .any(|todo| todo["content"] == TODO_CONTENT),
            Self::Question => canonical
                .questions
                .iter()
                .any(|request| request["questions"][0]["header"] == "Planning Input"),
        }
    }
}

#[derive(Clone, Copy)]
enum AuthorityMode {
    Revoked,
    Current,
    Standalone,
    SqlFailure,
    RenewedHeader,
    ObservePublication,
}

/// A real provider stream, held before Done so the user transcript and
/// provider dispatch have completed before the independent writer is held.
struct HeldPlanProvider {
    chunks: Mutex<Option<tokio::sync::mpsc::Receiver<StreamChunk>>>,
    terminal_seen: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Option<CancellationToken>>>,
}

#[async_trait]
impl Provider for HeldPlanProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "scripted-provider-stream".into(),
            name: "Held planning provider".into(),
            models: vec![tandem_types::ModelInfo {
                id: "scripted-model".into(),
                provider_id: "scripted-provider-stream".into(),
                display_name: "Scripted model".into(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(&self, _: &str, _: Option<&str>) -> anyhow::Result<String> {
        anyhow::bail!("planning regression requires the held provider stream")
    }

    async fn stream(
        &self,
        _: Vec<ChatMessage>,
        _: Option<&str>,
        _: ToolMode,
        _: Option<Vec<ToolSchema>>,
        _: SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        *self.cancellation.lock().unwrap() = Some(cancel);
        let chunks = self.chunks.lock().unwrap().take().unwrap();
        let terminal_seen = self.terminal_seen.clone();
        Ok(Box::pin(futures::stream::unfold(
            (chunks, terminal_seen),
            |(mut chunks, terminal_seen)| async move {
                chunks.recv().await.map(|chunk| {
                    if matches!(&chunk, StreamChunk::Done { .. }) {
                        terminal_seen.store(true, Ordering::SeqCst);
                    }
                    (Ok(chunk), (chunks, terminal_seen))
                })
            },
        )))
    }
}

/// The test runtime has exactly one blocking worker. A parked worker and a
/// later FIFO sentinel let us finish each preliminary native read without
/// advancing the owned prompt future until the real mutation is queued.
/// Channel disconnect also releases the worker on assertion unwinding.
struct BlockingGate {
    release: Option<mpsc::Sender<()>>,
    worker: Option<tokio::task::JoinHandle<()>>,
}

impl BlockingGate {
    fn queue() -> (Self, oneshot::Receiver<()>) {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        });
        (
            Self {
                release: Some(release_tx),
                worker: Some(worker),
            },
            entered_rx,
        )
    }

    async fn park() -> Self {
        let (gate, entered) = Self::queue();
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .expect("sole blocking worker reaches the test gate")
            .unwrap();
        gate
    }

    async fn release_and_join(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.worker.take().unwrap().await.unwrap();
    }

    async fn after_read(self) -> Self {
        // The prompt's read was queued before this sentinel. Since there is
        // one worker, sentinel entry proves that actual read has finished.
        let (next, entered) = Self::queue();
        self.release_and_join().await;
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .expect("native read finishes before its FIFO sentinel")
            .unwrap();
        next
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CanonicalState {
    todos: Vec<Value>,
    questions: Vec<Value>,
}

fn read_canonical(database: &Path, session_id: &str) -> CanonicalState {
    let connection = Connection::open(database).expect("independent canonical state reader");
    let metadata: Option<String> = connection
        .query_row(
            "SELECT metadata_json FROM session_metadata WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    let todos = metadata
        .map(|raw| serde_json::from_str::<Value>(&raw).unwrap())
        .and_then(|metadata| metadata["todos"].as_array().cloned())
        .unwrap_or_default();
    let mut statement = connection
        .prepare("SELECT request_json FROM session_question_requests WHERE session_id = ?1 ORDER BY request_id")
        .unwrap();
    let questions = statement
        .query_map([session_id], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    CanonicalState { todos, questions }
}

fn assistant_count(database: &Path, session_id: &str) -> usize {
    Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1 AND role = 'assistant'",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn fallback_events(events: &[EngineEvent]) -> Vec<&EngineEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(event.event_type.as_str(), "todo.updated" | "question.asked")
                || (event.event_type == "message.part.updated"
                    && event.properties["part"]["tool"] == "todo_write")
        })
        .collect()
}

fn assert_correlation(event: &EngineEvent, session_id: &str) {
    assert_eq!(event.properties["sessionID"], session_id);
    assert_eq!(event.properties["runID"], RUN_ID);
    let envelope = event.envelope.as_ref().expect("canonical event envelope");
    assert_eq!(envelope.session_id.as_deref(), Some(session_id));
    assert_eq!(envelope.run_id.as_deref(), Some(RUN_ID));
}

struct CommitObservation {
    fallback: Fallback,
    database: PathBuf,
    session_id: String,
    events: Mutex<broadcast::Receiver<EngineEvent>>,
    identities: Arc<Mutex<Vec<Option<VerifiedTenantContext>>>>,
}

struct PlanningAuthority {
    revoked: Arc<AtomicBool>,
    terminal_seen: Arc<AtomicBool>,
    post_terminal_checks: AtomicUsize,
    allowed_terminal_check: Arc<Mutex<Option<VerifiedTenantContext>>>,
    evaluations: Arc<AtomicUsize>,
    deny_original_assertion: bool,
    attempted_commits: Arc<Mutex<Vec<Option<VerifiedTenantContext>>>>,
    observation: CommitObservation,
}

impl ToolPolicyHook for PlanningAuthority {
    fn evaluate_tool(
        &self,
        _: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
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
        verified: Option<VerifiedTenantContext>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let allowed = !self.revoked.load(Ordering::SeqCst);
        if self.terminal_seen.load(Ordering::SeqCst)
            && self.post_terminal_checks.fetch_add(1, Ordering::SeqCst) == 0
            && allowed
        {
            *self.allowed_terminal_check.lock().unwrap() = verified;
        }
        Box::pin(async move {
            anyhow::ensure!(allowed, REVOKED);
            Ok(())
        })
    }

    fn with_session_commit_authority(
        &self,
        verified: Option<VerifiedTenantContext>,
        commit: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.attempted_commits
            .lock()
            .unwrap()
            .push(verified.clone());
        anyhow::ensure!(!self.revoked.load(Ordering::SeqCst), REVOKED);
        anyhow::ensure!(
            !self.deny_original_assertion
                || verified.as_ref().is_none_or(|identity| {
                    identity.assertion_id != "original-planning-authority"
                }),
            ORIGINAL_DENIED
        );
        let observation = &self.observation;
        let before = read_canonical(&observation.database, &observation.session_id);
        let result = commit();
        if result.is_ok() {
            let after = read_canonical(&observation.database, &observation.session_id);
            if !observation.fallback.present(&before) && observation.fallback.present(&after) {
                // A later final-assistant hook cannot pass this observer:
                // the canonical fallback must transition inside this call.
                assert_eq!(
                    assistant_count(&observation.database, &observation.session_id),
                    0
                );
                let mut receiver = observation.events.lock().unwrap();
                let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
                let events = fallback_events(&events);
                assert_eq!(
                    events.len(),
                    match observation.fallback {
                        Fallback::Todo => 3,
                        Fallback::Question => 1,
                    }
                );
                for event in events {
                    assert_correlation(event, &observation.session_id);
                }
                observation.identities.lock().unwrap().push(verified);
            }
        }
        result
    }
}

fn planning_identity(
    tenant: TenantContext,
    assertion_id: &str,
    version: u64,
) -> VerifiedTenantContext {
    let mut claims = tandem_types::TenantContextAssertionClaims::new_v1(
        "planning-test",
        "planning-runtime",
        1,
        u64::MAX,
        assertion_id,
        tenant,
        tandem_types::HumanActor::tandem_user("planning-user"),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user("planning-user", "planning-test"),
        ),
        Vec::new(),
    );
    claims.policy_version = Some(version);
    claims.into()
}

struct PlanningResult {
    result: anyhow::Result<()>,
    canonical: CanonicalState,
    previous: CanonicalState,
    session: Session,
    events: Vec<EngineEvent>,
    queued_parts: Vec<EngineEvent>,
    original_commits: Vec<Option<VerifiedTenantContext>>,
    attempted_commits: Vec<Option<VerifiedTenantContext>>,
    cancelled: bool,
    evaluations: usize,
}

async fn pending_prompt<F: Future<Output = anyhow::Result<()>>>(prompt: Pin<&mut F>, stage: &str) {
    let state = futures::poll!(prompt);
    assert!(
        state.is_pending(),
        "prompt must queue {stage}, got {state:?}"
    );
}

async fn run_planning(fallback: Fallback, mode: AuthorityMode) -> PlanningResult {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite3");
    let (chunks_tx, chunks_rx) = tokio::sync::mpsc::channel(2);
    let terminal_seen = Arc::new(AtomicBool::new(false));
    let cancellation = Arc::new(Mutex::new(None));
    let provider = Arc::new(HeldPlanProvider {
        chunks: Mutex::new(Some(chunks_rx)),
        terminal_seen: terminal_seen.clone(),
        cancellation: cancellation.clone(),
    });
    let (engine, bus, storage) =
        engine_loop_with_scripted_provider(directory.path(), provider).await;
    let mut session = Session::new(
        Some("held planning fallback".into()),
        Some(directory.path().display().to_string()),
    );
    session.model = Some(scripted_model());
    session.tenant_context = TenantContext::explicit_user_workspace(
        "planning-org",
        "planning-workspace",
        Some("planning-deployment".into()),
        "planning-user",
    );
    if !matches!(mode, AuthorityMode::Standalone) {
        session.verified_tenant_context = Some(planning_identity(
            session.tenant_context.clone(),
            "original-planning-authority",
            1,
        ));
    }
    let tenant = session.tenant_context.clone();
    let session_id = session.id.clone();
    let _boundary =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    if matches!(mode, AuthorityMode::SqlFailure) {
        match fallback {
            Fallback::Todo => storage.set_todos(&session_id, vec![json!({"id":"prior-todo", "content":"Previously authorized task", "status":"pending"})]).await.unwrap(),
            Fallback::Question => { storage.add_question_request(&session_id, "prior-message", vec![json!({"header":"Earlier input", "question":"Previously authorized input"})]).await.unwrap(); }
        }
    }
    let previous = read_canonical(&database, &session_id);
    let revoked = Arc::new(AtomicBool::new(false));
    let allowed_terminal_check = Arc::new(Mutex::new(None));
    let original_commits = Arc::new(Mutex::new(Vec::new()));
    let attempted_commits = Arc::new(Mutex::new(Vec::new()));
    let evaluations = Arc::new(AtomicUsize::new(0));
    let has_hook = !matches!(mode, AuthorityMode::Standalone);
    if has_hook {
        engine
            .set_tool_policy_hook(Arc::new(PlanningAuthority {
                revoked: revoked.clone(),
                terminal_seen: terminal_seen.clone(),
                post_terminal_checks: AtomicUsize::new(0),
                allowed_terminal_check: allowed_terminal_check.clone(),
                evaluations: evaluations.clone(),
                deny_original_assertion: matches!(mode, AuthorityMode::RenewedHeader),
                attempted_commits: attempted_commits.clone(),
                observation: CommitObservation {
                    fallback,
                    database: database.clone(),
                    session_id: session_id.clone(),
                    events: Mutex::new(bus.subscribe()),
                    identities: original_commits.clone(),
                },
            }))
            .await;
    }
    let mut live = bus.subscribe();
    let mut queued = bus.take_session_part_receiver().unwrap();
    let mut events = Vec::new();
    let prompt = engine.run_prompt_async_with_execution_context(
        session_id.clone(),
        SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: "Prepare a synthetic task plan.".into(),
            }],
            model: Some(scripted_model()),
            agent: Some("plan".into()),
            tool_mode: Some(ToolMode::None),
            tool_allowlist: None,
            strict_kb_grounding: None,
            context_mode: None,
            write_required: None,
            prewrite_requirements: None,
            sampling: Default::default(),
        },
        None,
        Some(RUN_ID.into()),
        Vec::new(),
    );
    tokio::pin!(prompt);
    chunks_tx
        .send(StreamChunk::TextDelta(fallback.completion().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                result = &mut prompt => panic!("prompt ended before held provider output: {result:?}"),
                event = live.recv() => {
                    let event = event.unwrap();
                    let reached = event.event_type == "message.part.updated" && event.properties["delta"] == fallback.completion();
                    events.push(event);
                    if reached { break; }
                }
            }
        }
    }).await.expect("real plan agent consumes provider output");
    if matches!(mode, AuthorityMode::RenewedHeader) {
        assert!(storage
            .update_session_authority(
                &session_id,
                tenant.clone(),
                Some(planning_identity(
                    tenant.clone(),
                    "renewed-planning-authority",
                    2
                ))
            )
            .await
            .unwrap());
    }
    let writer = Connection::open(&database).unwrap();
    if matches!(mode, AuthorityMode::SqlFailure) {
        writer.execute_batch(match fallback {
            Fallback::Todo => "CREATE TRIGGER reject_plan_todos BEFORE UPDATE ON session_metadata BEGIN SELECT RAISE(ABORT, 'synthetic plan todo write failure'); END;",
            Fallback::Question => "CREATE TRIGGER reject_plan_question BEFORE INSERT ON session_question_requests BEGIN SELECT RAISE(ABORT, 'synthetic plan question insert failure'); END;",
        }).unwrap();
    }
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut gate = BlockingGate::park().await;
    chunks_tx
        .send(StreamChunk::Done {
            finish_reason: "stop".into(),
            usage: None,
        })
        .await
        .unwrap();
    pending_prompt(prompt.as_mut(), "terminal authority read or fallback").await;
    assert!(terminal_seen.load(Ordering::SeqCst));
    if has_hook {
        gate = gate.after_read().await;
        pending_prompt(prompt.as_mut(), "fallback after the allowed terminal check").await;
        let checked = allowed_terminal_check.lock().unwrap();
        let checked = checked
            .as_ref()
            .expect("real terminal authority check allowed before writer wait");
        assert_eq!(
            checked.assertion_id,
            if matches!(mode, AuthorityMode::RenewedHeader) {
                "renewed-planning-authority"
            } else {
                "original-planning-authority"
            }
        );
        assert_eq!(
            checked.policy_version,
            Some(if matches!(mode, AuthorityMode::RenewedHeader) {
                2
            } else {
                1
            })
        );
    }
    if matches!(fallback, Fallback::Question) {
        gate = gate.after_read().await;
        pending_prompt(prompt.as_mut(), "question preparation session read").await;
        gate = gate.after_read().await;
        pending_prompt(prompt.as_mut(), "native question mutation").await;
    }
    // With the final sentinel still parked, the real mutation is now queued
    // behind it. Both preliminary question reads have actually completed;
    // Done alone is never used as a question-writer witness.
    assert_eq!(read_canonical(&database, &session_id), previous);
    assert!(original_commits.lock().unwrap().is_empty());
    assert!(
        attempted_commits.lock().unwrap().is_empty(),
        "native authority guard cannot run before SQLite grants its writer"
    );
    if matches!(mode, AuthorityMode::ObservePublication) {
        while let Ok(event) = live.try_recv() {
            events.push(event);
        }
        assert!(
            fallback_events(&events).is_empty(),
            "fallback payload must not publish before the native guarded commit"
        );
    }
    gate.release_and_join().await;
    if matches!(mode, AuthorityMode::Revoked) {
        revoked.store(true, Ordering::SeqCst);
    }
    writer.execute_batch("COMMIT").unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), prompt.as_mut())
        .await
        .expect("actual pending fallback finishes after writer release");
    let canonical = read_canonical(&database, &session_id);
    let session = storage.get_session(&session_id).await.unwrap();
    if matches!(fallback, Fallback::Question) && result.is_ok() {
        for request in storage.list_question_requests().await {
            assert!(
                storage
                    .get_question_request_for_tenant(&request.id, &tenant, Some(&session_id))
                    .await
                    .unwrap()
                    .is_some(),
                "real stored question retains tenant/session, digest and expiry binding"
            );
        }
    }
    while let Ok(event) = live.try_recv() {
        events.push(event);
    }
    let queued_parts = std::iter::from_fn(|| queued.try_recv().ok()).collect();
    let cancelled = cancellation
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_cancelled();
    let original_commits = original_commits.lock().unwrap().clone();
    let attempted_commits = attempted_commits.lock().unwrap().clone();
    PlanningResult {
        result,
        canonical,
        previous,
        session,
        events,
        queued_parts,
        original_commits,
        attempted_commits,
        cancelled,
        evaluations: evaluations.load(Ordering::SeqCst),
    }
}

fn scenario(fallback: Fallback, mode: AuthorityMode) -> PlanningResult {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(run_planning(fallback, mode))
}

fn assert_no_final_success(result: &PlanningResult) {
    assert!(
        !result
            .session
            .messages
            .iter()
            .any(|message| matches!(message.role, MessageRole::Assistant)),
        "failed fallback must not commit a final assistant"
    );
    assert!(
        !result.events.iter().any(|event| matches!(
            event.event_type.as_str(),
            "session.updated" | "session.status"
        ) && event.properties["status"] == "idle"),
        "failed fallback must not publish final success"
    );
    assert!(
        !result.events.iter().any(|event| {
            event.event_type == "message.part.updated"
                && event.properties["part"]["type"] == "text"
                && event.properties.get("delta").is_none()
        }),
        "failed fallback must not publish a final assistant text event"
    );
}

fn assert_success(result: &PlanningResult, fallback: Fallback) {
    assert!(
        result.result.is_ok(),
        "current-authority fallback failed: {:?}",
        result.result
    );
    assert!(fallback.present(&result.canonical));
    assert_eq!(
        result.evaluations, 0,
        "fallback must not repeat approval-producing tool policy"
    );
    assert!(!result.cancelled);
    assert_eq!(
        result
            .session
            .messages
            .iter()
            .filter(|message| matches!(message.role, MessageRole::Assistant))
            .count(),
        1
    );
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| {
                (event.event_type == "message.part.updated"
                    && event.properties["part"]["type"] == "text"
                    && event.properties.get("delta").is_none())
                    || (matches!(
                        event.event_type.as_str(),
                        "session.updated" | "session.status"
                    ) && event.properties["status"] == "idle")
            })
            .count(),
        3,
        "successful fallback retains the final text and both idle events"
    );
    let events = fallback_events(&result.events);
    for event in &events {
        assert_correlation(event, &result.session.id);
    }
    match fallback {
        Fallback::Todo => {
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].properties["part"]["state"], "running");
            assert_eq!(events[1].properties["part"]["state"], "completed");
            assert_eq!(
                events[0].properties["part"]["id"],
                events[1].properties["part"]["id"]
            );
            assert_eq!(
                events[1].properties["part"]["result"]["todos"],
                json!(result.canonical.todos)
            );
            assert_eq!(events[2].event_type, "todo.updated");
            assert_eq!(events[2].properties["todos"], json!(result.canonical.todos));
            let queued: Vec<_> = result
                .queued_parts
                .iter()
                .filter(|event| event.properties["part"]["tool"] == "todo_write")
                .collect();
            assert_eq!(
                queued.len(),
                1,
                "completed tool result is enqueued once; running invocation is filtered"
            );
            assert_correlation(queued[0], &result.session.id);
        }
        Fallback::Question => {
            assert_eq!(events.len(), 1);
            let request = result
                .canonical
                .questions
                .iter()
                .find(|request| request["questions"][0]["header"] == "Planning Input")
                .unwrap();
            assert_eq!(events[0].properties["id"], request["id"]);
            assert_eq!(events[0].properties["questions"], request["questions"]);
            assert_eq!(events[0].properties["tool"], request["tool"]);
            assert_eq!(
                events[0].properties["messageID"],
                request["tool"]["messageID"]
            );
            assert!(request["actionDigest"]
                .as_str()
                .is_some_and(|digest| !digest.is_empty()));
            assert!(request["expiresAtMs"].as_u64().unwrap() > 0);
        }
    }
}

fn assert_revoked(fallback: Fallback) {
    let result = scenario(fallback, AuthorityMode::Revoked);
    // Check the actual incorrect durability first, not a timeout or unrelated
    // early denial. Unchanged production reaches this assertion with the row.
    assert_eq!(
        result.canonical, result.previous,
        "revoked {fallback:?} fallback persisted behind the held real writer"
    );
    assert!(
        fallback_events(&result.events).is_empty(),
        "revoked fallback must not publish an invocation or success"
    );
    assert!(result
        .result
        .as_ref()
        .unwrap_err()
        .to_string()
        .contains(REVOKED));
    assert_no_final_success(&result);
    assert!(result.cancelled);
    assert_eq!(
        result.attempted_commits.len(),
        1,
        "authority must be denied at the actual native fallback boundary"
    );
}

fn assert_sql_failure(fallback: Fallback) {
    let result = scenario(fallback, AuthorityMode::SqlFailure);
    assert_eq!(
        result.canonical, result.previous,
        "failed native write must preserve earlier canonical state"
    );
    assert!(fallback_events(&result.events).is_empty(), "SQL failure must not publish fallback payload, invocation, completion, or synthetic question");
    assert_no_final_success(&result);
    let error = result
        .result
        .as_ref()
        .expect_err("native fallback failure propagates");
    let error = format!("{error:#}");
    assert!(
        error.contains(match fallback {
            Fallback::Todo => "synthetic plan todo write failure",
            Fallback::Question => "synthetic plan question insert failure",
        }),
        "unexpected native failure: {error}"
    );
    assert!(result.cancelled);
}

fn assert_original_authority(fallback: Fallback) {
    let result = scenario(fallback, AuthorityMode::RenewedHeader);
    assert_eq!(
        result.canonical, result.previous,
        "renewed stored identity must not grant the original prompt a fallback commit"
    );
    assert!(fallback_events(&result.events).is_empty());
    assert_no_final_success(&result);
    let error = result
        .result
        .as_ref()
        .expect_err("original assertion is denied");
    assert!(format!("{error:#}").contains(ORIGINAL_DENIED));
    assert!(result.cancelled);
    assert!(result.original_commits.is_empty());
    assert_eq!(
        result.attempted_commits.len(),
        1,
        "the denied native fallback must receive the original prompt identity"
    );
    let captured = result.attempted_commits[0].as_ref().unwrap();
    assert_eq!(captured.assertion_id, "original-planning-authority");
    assert_eq!(captured.policy_version, Some(1));
    let renewed = result.session.verified_tenant_context.as_ref().unwrap();
    assert_eq!(renewed.assertion_id, "renewed-planning-authority");
    assert_eq!(renewed.policy_version, Some(2));
}

fn assert_publication_guard(fallback: Fallback) {
    let result = scenario(fallback, AuthorityMode::ObservePublication);
    assert_success(&result, fallback);
    assert_eq!(result.original_commits.len(), 1, "row transition and all fallback events must be visible before the same authority callback returns");
}

#[test]
fn hosted_plan_fallback_todo_writer_wait_rechecks_revocation() {
    assert_revoked(Fallback::Todo);
}

#[test]
fn hosted_plan_fallback_question_writer_wait_rechecks_revocation() {
    assert_revoked(Fallback::Question);
}

#[test]
fn hosted_plan_fallback_todo_preserves_current_authority() {
    assert_success(
        &scenario(Fallback::Todo, AuthorityMode::Current),
        Fallback::Todo,
    );
}

#[test]
fn hosted_plan_fallback_question_preserves_current_authority() {
    assert_success(
        &scenario(Fallback::Question, AuthorityMode::Current),
        Fallback::Question,
    );
}

#[test]
fn hosted_plan_fallback_todo_preserves_standalone_without_hook() {
    assert_success(
        &scenario(Fallback::Todo, AuthorityMode::Standalone),
        Fallback::Todo,
    );
}

#[test]
fn hosted_plan_fallback_question_preserves_standalone_without_hook() {
    assert_success(
        &scenario(Fallback::Question, AuthorityMode::Standalone),
        Fallback::Question,
    );
}

#[test]
fn hosted_plan_fallback_todo_sql_failure_has_no_final_success() {
    assert_sql_failure(Fallback::Todo);
}

#[test]
fn hosted_plan_fallback_question_sql_failure_has_no_synthetic_question() {
    assert_sql_failure(Fallback::Question);
}

#[test]
fn hosted_plan_fallback_todo_retains_original_prompt_authority() {
    assert_original_authority(Fallback::Todo);
}

#[test]
fn hosted_plan_fallback_question_retains_original_prompt_authority() {
    assert_original_authority(Fallback::Question);
}

#[test]
fn hosted_plan_fallback_todo_keeps_write_and_events_inside_authority() {
    assert_publication_guard(Fallback::Todo);
}

#[test]
fn hosted_plan_fallback_question_keeps_write_and_events_inside_authority() {
    assert_publication_guard(Fallback::Question);
}
