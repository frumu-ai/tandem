// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use futures::future::BoxFuture;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::future::Future;
use std::path::Path;
use std::sync::{atomic::AtomicBool, mpsc};

const PLAN_RUN: &str = "hosted-plan-fallback-run";
const TODO_TEXT: &str = "- [ ] Prepare the synthetic planning deliverable";
const QUESTION_TEXT: &str = "I need more information before I can prepare a concrete task list.";

#[derive(Clone, Copy, Debug)]
enum PlanFallback {
    Todo,
    Question,
}

impl PlanFallback {
    fn text(self) -> &'static str {
        match self {
            Self::Todo => TODO_TEXT,
            Self::Question => QUESTION_TEXT,
        }
    }
    fn present(self, snapshot: &Canonical) -> bool {
        match self {
            Self::Todo => snapshot
                .todos
                .iter()
                .any(|v| v["content"] == "Prepare the synthetic planning deliverable"),
            Self::Question => snapshot
                .questions
                .iter()
                .any(|v| v["questions"][0]["header"] == "Planning Input"),
        }
    }
}

#[derive(Clone, Copy)]
enum PlanMode {
    Revoked,
    SqlFailure,
    RenewedHeader,
    ObservePublication,
}

#[derive(Clone, Debug, PartialEq)]
struct Canonical {
    todos: Vec<Value>,
    questions: Vec<Value>,
}

fn canonical(database: &Path, session_id: &str) -> Canonical {
    let connection = Connection::open(database).unwrap();
    let raw: Option<String> = connection
        .query_row(
            "SELECT metadata_json FROM session_metadata WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    let todos = raw
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value["todos"].as_array().cloned())
        .unwrap_or_default();
    let mut statement = connection.prepare(
        "SELECT request_json FROM session_question_requests WHERE session_id = ?1 ORDER BY request_id"
    ).unwrap();
    let questions = statement
        .query_map([session_id], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    Canonical { todos, questions }
}

fn fallback_payload_event(event: &EngineEvent) -> bool {
    (event.event_type == "message.part.updated" && event.properties["part"]["tool"] == "todo_write")
        || matches!(event.event_type.as_str(), "todo.updated" | "question.asked")
}

fn idle_event(event: &EngineEvent) -> bool {
    matches!(
        event.event_type.as_str(),
        "session.updated" | "session.status"
    ) && event.properties["status"] == "idle"
}

/// The single Tokio blocking worker is occupied by a channel receive.
/// A later sentinel entering proves every native operation queued before it
/// has completed; channel disconnect releases it on assertion unwinding.
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
            .unwrap()
            .unwrap();
        gate
    }

    async fn complete(mut self) {
        self.release.take().unwrap().send(()).unwrap();
        self.worker.take().unwrap().await.unwrap();
    }

    async fn after_read(self) -> Self {
        let (next, entered) = Self::queue();
        self.complete().await;
        tokio::time::timeout(Duration::from_secs(10), entered)
            .await
            .unwrap()
            .unwrap();
        next
    }
}

struct PlanServerHook {
    server: ServerToolPolicyHook,
    state: AppState,
    kind: PlanFallback,
    database: std::path::PathBuf,
    session_id: String,
    terminal_seen: Arc<AtomicBool>,
    terminal_checks: AtomicUsize,
    allowed_terminal: Arc<Mutex<Option<VerifiedTenantContext>>>,
    attempted: Arc<Mutex<Vec<Option<VerifiedTenantContext>>>>,
    observed: Arc<AtomicBool>,
    evaluations: Arc<AtomicUsize>,
    events: Mutex<broadcast::Receiver<EngineEvent>>,
}

impl ToolPolicyHook for PlanServerHook {
    fn revalidate_session(
        &self,
        verified: Option<VerifiedTenantContext>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let real_check = self.server.revalidate_session(verified.clone());
        let terminal = self.terminal_seen.load(Ordering::SeqCst)
            && self.terminal_checks.fetch_add(1, Ordering::SeqCst) == 0;
        let allowed_terminal = self.allowed_terminal.clone();
        Box::pin(async move {
            real_check.await?;
            if terminal {
                *allowed_terminal.lock().unwrap() = verified;
            }
            Ok(())
        })
    }

    fn with_session_commit_authority(
        &self,
        verified: Option<VerifiedTenantContext>,
        commit: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.attempted.lock().unwrap().push(verified.clone());
        let mut observed_commit = || {
            let before = canonical(&self.database, &self.session_id);
            commit()?;
            let after = canonical(&self.database, &self.session_id);
            if !self.kind.present(&before) && self.kind.present(&after) {
                assert!(
                    self.state
                        .enterprise
                        .hosted_policy
                        .publication_write_blocked_for_test(),
                    "real policy snapshot must remain held through the fallback success events"
                );
                let mut receiver = self.events.lock().unwrap();
                let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
                let relevant: Vec<_> = events
                    .iter()
                    .filter(|event| match self.kind {
                        PlanFallback::Todo => {
                            event.event_type == "todo.updated"
                                || event.event_type == "message.part.updated"
                                    && event.properties["part"]["tool"] == "todo_write"
                        }
                        PlanFallback::Question => event.event_type == "question.asked",
                    })
                    .collect();
                assert_eq!(
                    relevant.len(),
                    match self.kind {
                        PlanFallback::Todo => 3,
                        PlanFallback::Question => 1,
                    }
                );
                for event in relevant {
                    assert_eq!(event.properties["sessionID"], self.session_id);
                    assert_eq!(event.properties["runID"], PLAN_RUN);
                    let envelope = event.envelope.as_ref().unwrap();
                    assert_eq!(
                        envelope.session_id.as_deref(),
                        Some(self.session_id.as_str())
                    );
                    assert_eq!(envelope.run_id.as_deref(), Some(PLAN_RUN));
                }
                self.observed.store(true, Ordering::SeqCst);
            }
            Ok(())
        };
        self.server
            .with_session_commit_authority(verified, &mut observed_commit)
    }

    fn revalidate_dispatch(
        &self,
        context: tandem_tools::ToolDispatchPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<Option<tandem_tools::ToolDispatchDecision>>> {
        self.server.revalidate_dispatch(context)
    }

    fn evaluate_tool(
        &self,
        context: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        self.server.evaluate_tool(context)
    }
}

fn plan_request() -> SendMessageRequest {
    SendMessageRequest {
        parts: vec![MessagePartInput::Text {
            text: "Prepare a synthetic task plan.".into(),
        }],
        model: Some(model()),
        agent: Some("plan".into()),
        tool_mode: Some(ToolMode::None),
        tool_allowlist: None,
        strict_kb_grounding: None,
        context_mode: None,
        write_required: None,
        prewrite_requirements: None,
        sampling: Default::default(),
    }
}

async fn pending<F: Future<Output = anyhow::Result<()>>>(
    prompt: std::pin::Pin<&mut F>,
    stage: &str,
) {
    let state = futures::poll!(prompt);
    assert!(
        state.is_pending(),
        "prompt must queue {stage}, got {state:?}"
    );
}

/// Run the exact EngineLoop path with a real published host policy. The extra
/// thread is used only for real reload: reload_hosted_policy itself needs a
/// blocking worker, while the prompt's one-worker FIFO gate is parked.
fn reload_from_another_runtime(state: AppState) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(state.reload_hosted_policy())
    })
    .join()
    .unwrap()
    .unwrap();
}

struct PlanOutcome {
    result: anyhow::Result<()>,
    previous: Canonical,
    current: Canonical,
    events: Vec<EngineEvent>,
    session: Session,
    attempted: Vec<Option<VerifiedTenantContext>>,
    observed: bool,
    cancelled: bool,
    evaluations: usize,
}

async fn run_plan(kind: PlanFallback, mode: PlanMode) -> PlanOutcome {
    let fixture = Fixture::new().await;
    let database = fixture.directory.path().join("sessions/sessions.sqlite3");
    if matches!(mode, PlanMode::SqlFailure) {
        match kind {
            PlanFallback::Todo => fixture.storage.set_todos(&fixture.session_id,
                vec![json!({"id":"prior-todo", "content":"Previously authorized task", "status":"pending"})]).await.unwrap(),
            PlanFallback::Question => { fixture.storage.add_question_request(&fixture.session_id,
                "prior-message", vec![json!({"header":"Earlier input", "question":"Previously authorized input"})]).await.unwrap(); }
        }
    }
    let previous = canonical(&database, &fixture.session_id);
    let allowed_terminal = Arc::new(Mutex::new(None));
    let attempted = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::new(AtomicBool::new(false));
    let evaluations = Arc::new(AtomicUsize::new(0));
    fixture
        .engine
        .set_tool_policy_hook(Arc::new(PlanServerHook {
            server: ServerToolPolicyHook::new(fixture.state.clone()),
            state: fixture.state.clone(),
            kind,
            database: database.clone(),
            session_id: fixture.session_id.clone(),
            terminal_seen: fixture.terminal_seen.clone(),
            terminal_checks: AtomicUsize::new(0),
            allowed_terminal: allowed_terminal.clone(),
            attempted: attempted.clone(),
            observed: observed.clone(),
            evaluations: evaluations.clone(),
            events: Mutex::new(fixture.state.event_bus.subscribe()),
        }))
        .await;
    let mut receiver = fixture.state.event_bus.subscribe();
    let mut events = Vec::new();
    let prompt = fixture.engine.run_prompt_async_with_execution_context(
        fixture.session_id.clone(),
        plan_request(),
        None,
        Some(PLAN_RUN.into()),
        Vec::new(),
    );
    tokio::pin!(prompt);
    fixture
        .chunks
        .send(StreamChunk::TextDelta(kind.text().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                result = &mut prompt => panic!("prompt ended before provider text: {result:?}"),
                event = receiver.recv() => {
                    let event = event.unwrap();
                    let reached = event.event_type == "message.part.updated" && event.properties["delta"] == kind.text();
                    events.push(event);
                    if reached { break; }
                }
            }
        }
    }).await.expect("actual plan agent consumed provider text");

    if matches!(mode, PlanMode::RenewedHeader) {
        let mut renewed = identity(5);
        renewed.assertion_id = "assertion-renewed".into();
        assert!(fixture
            .storage
            .update_session_authority(
                &fixture.session_id,
                fixture.verified.tenant_context.clone(),
                Some(renewed)
            )
            .await
            .unwrap());
        write_input(
            &fixture.directory.path().join("policy.json"),
            &policy_json(5, crate::now_ms(), true),
        );
        fixture.state.reload_hosted_policy().await.unwrap();
    }
    let writer = Connection::open(&database).unwrap();
    if matches!(mode, PlanMode::SqlFailure) {
        writer.execute_batch(match kind {
            PlanFallback::Todo => "CREATE TRIGGER reject_plan_todos BEFORE UPDATE ON session_metadata BEGIN SELECT RAISE(ABORT, 'synthetic plan todo write failure'); END;",
            PlanFallback::Question => "CREATE TRIGGER reject_plan_question BEFORE INSERT ON session_question_requests BEGIN SELECT RAISE(ABORT, 'synthetic plan question insert failure'); END;",
        }).unwrap();
    }
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut gate = BlockingGate::park().await;
    fixture.finish_stream().await;
    pending(prompt.as_mut(), "terminal session authority read").await;
    assert!(fixture.terminal_seen.load(Ordering::SeqCst));
    gate = gate.after_read().await;
    pending(
        prompt.as_mut(),
        "fallback after real allowed terminal check",
    )
    .await;
    let terminal_guard = allowed_terminal.lock().unwrap();
    let terminal = terminal_guard
        .as_ref()
        .expect("real server hook allowed terminal read");
    assert_eq!(
        terminal.policy_version,
        Some(if matches!(mode, PlanMode::RenewedHeader) {
            5
        } else {
            4
        })
    );
    assert_eq!(
        terminal.assertion_id,
        if matches!(mode, PlanMode::RenewedHeader) {
            "assertion-renewed"
        } else {
            "assertion-a"
        }
    );
    drop(terminal_guard);
    if matches!(kind, PlanFallback::Question) {
        gate = gate.after_read().await;
        pending(prompt.as_mut(), "preliminary get_session read").await;
        gate = gate.after_read().await;
        pending(prompt.as_mut(), "native question INSERT").await;
    }
    assert_eq!(
        canonical(&database, &fixture.session_id),
        previous,
        "the canonical fallback cannot appear before the held native writer"
    );
    assert!(!observed.load(Ordering::SeqCst));
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    assert!(
        !events.iter().any(fallback_payload_event),
        "no planning payload may publish before the guarded native commit"
    );
    if matches!(mode, PlanMode::Revoked) {
        write_input(
            &fixture.directory.path().join("policy.json"),
            &policy_json(5, crate::now_ms(), false),
        );
        reload_from_another_runtime(fixture.state.clone());
    }
    gate.complete().await;
    writer.execute_batch("COMMIT").unwrap();
    let result = tokio::time::timeout(Duration::from_secs(10), prompt.as_mut())
        .await
        .expect("actual pending fallback completes after native writer release");
    let current = canonical(&database, &fixture.session_id);
    if matches!(kind, PlanFallback::Question) && result.is_ok() {
        for request in fixture.storage.list_question_requests().await {
            assert!(
                fixture
                    .storage
                    .get_question_request_for_tenant(
                        &request.id,
                        &fixture.verified.tenant_context,
                        Some(&fixture.session_id)
                    )
                    .await
                    .unwrap()
                    .is_some(),
                "question request must retain tenant, digest, TTL and session binding"
            );
        }
    }
    let session = fixture
        .storage
        .get_session(&fixture.session_id)
        .await
        .unwrap();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    let attempted = attempted.lock().unwrap().clone();
    let cancelled = fixture
        .cancellation
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_cancelled();
    PlanOutcome {
        result,
        previous,
        current,
        events,
        session,
        attempted,
        observed: observed.load(Ordering::SeqCst),
        cancelled,
        evaluations: evaluations.load(Ordering::SeqCst),
    }
}

fn scenario(kind: PlanFallback, mode: PlanMode) -> PlanOutcome {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(run_plan(kind, mode))
}

fn assert_no_final(outcome: &PlanOutcome) {
    assert!(!outcome
        .session
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::Assistant)));
    assert!(!outcome.events.iter().any(idle_event));
    assert!(
        !outcome.events.iter().any(|event| {
            event.event_type == "message.part.updated"
                && event.properties.get("delta").is_none()
                && event.properties["part"]["text"]
                    .as_str()
                    .is_some_and(|text| matches!(text, TODO_TEXT | QUESTION_TEXT))
        }),
        "failed fallback must not publish final completion text"
    );
}

fn assert_failure(kind: PlanFallback, mode: PlanMode) {
    let outcome = scenario(kind, mode);
    assert_eq!(
        outcome.current, outcome.previous,
        "denied/failed {kind:?} mutated the canonical row"
    );
    assert!(
        !outcome.events.iter().any(fallback_payload_event),
        "denied/failed fallback published a planning payload"
    );
    assert_no_final(&outcome);
    assert!(outcome.cancelled);
    assert_eq!(outcome.evaluations, 0);
    assert_eq!(
        outcome.attempted.len(),
        1,
        "the native fallback must reach its own commit authority hook"
    );
    assert_eq!(
        outcome.attempted[0].as_ref().unwrap().policy_version,
        Some(4),
        "commit must use original prompt authority, not a renewed stored header"
    );
    assert_eq!(
        outcome.attempted[0].as_ref().unwrap().assertion_id,
        "assertion-a",
        "commit must retain the original assertion id"
    );
    let error = outcome.result.unwrap_err().to_string();
    assert!(
        error.contains(match mode {
            PlanMode::Revoked | PlanMode::RenewedHeader =>
                "hosted_identity_policy_revision_changed",
            PlanMode::SqlFailure => match kind {
                PlanFallback::Todo => "synthetic plan todo write failure",
                PlanFallback::Question => "synthetic plan question insert failure",
            },
            _ => unreachable!(),
        }),
        "unexpected failure: {error}"
    );
}

fn assert_current(kind: PlanFallback, mode: PlanMode) {
    let outcome = scenario(kind, mode);
    outcome
        .result
        .expect("current authority must complete the real plan prompt");
    assert!(kind.present(&outcome.current));
    assert!(
        outcome.observed,
        "same server authority callback must see canonical row and success events"
    );
    assert_eq!(outcome.evaluations, 0);
    assert!(!outcome.cancelled);
    assert_eq!(
        outcome
            .session
            .messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::Assistant))
            .count(),
        1
    );
    assert!(outcome.events.iter().any(idle_event));
    let original = outcome.attempted[0].as_ref().unwrap();
    assert_eq!(original.policy_version, Some(4));
    assert_eq!(original.assertion_id, "assertion-a");
    match kind {
        PlanFallback::Todo => {
            let events: Vec<_> = outcome
                .events
                .iter()
                .filter(|e| {
                    e.event_type == "todo.updated"
                        || e.event_type == "message.part.updated"
                            && e.properties["part"]["tool"] == "todo_write"
                })
                .collect();
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].properties["part"]["state"], "running");
            assert_eq!(events[1].properties["part"]["state"], "completed");
            assert_eq!(
                events[0].properties["part"]["id"],
                events[1].properties["part"]["id"]
            );
            assert_eq!(
                events[1].properties["part"]["result"]["todos"],
                json!(outcome.current.todos)
            );
            assert_eq!(events[2].properties["todos"], json!(outcome.current.todos));
        }
        PlanFallback::Question => {
            let events: Vec<_> = outcome
                .events
                .iter()
                .filter(|e| e.event_type == "question.asked")
                .collect();
            assert_eq!(events.len(), 1);
            let request = outcome
                .current
                .questions
                .iter()
                .find(|v| v["questions"][0]["header"] == "Planning Input")
                .unwrap();
            assert_eq!(events[0].properties["id"], request["id"]);
            assert_eq!(events[0].properties["questions"], request["questions"]);
            assert_eq!(events[0].properties["tool"], request["tool"]);
            assert!(request["actionDigest"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
            assert!(request["expiresAtMs"].as_u64().unwrap() > 0);
        }
    }
}

#[test]
fn hosted_plan_todo_real_policy_writer_wait_rejects_v5_revocation() {
    assert_failure(PlanFallback::Todo, PlanMode::Revoked);
}

#[test]
fn hosted_plan_question_real_policy_fifo_writer_wait_rejects_v5_revocation() {
    assert_failure(PlanFallback::Question, PlanMode::Revoked);
}

#[test]
fn hosted_plan_todo_real_policy_preserves_current_authority_and_publication() {
    assert_current(PlanFallback::Todo, PlanMode::ObservePublication);
}

#[test]
fn hosted_plan_question_real_policy_preserves_current_authority_and_publication() {
    assert_current(PlanFallback::Question, PlanMode::ObservePublication);
}

#[test]
fn hosted_plan_todo_real_policy_uses_original_assertion_after_header_renewal() {
    assert_failure(PlanFallback::Todo, PlanMode::RenewedHeader);
}

#[test]
fn hosted_plan_question_real_policy_uses_original_assertion_after_header_renewal() {
    assert_failure(PlanFallback::Question, PlanMode::RenewedHeader);
}

#[test]
fn hosted_plan_todo_real_sql_failure_preserves_canonical_state() {
    assert_failure(PlanFallback::Todo, PlanMode::SqlFailure);
}

#[test]
fn hosted_plan_question_real_sql_failure_emits_no_synthetic_success() {
    assert_failure(PlanFallback::Question, PlanMode::SqlFailure);
}
