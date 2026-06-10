//! Long-session context regression evals (TAN-192) with provenance
//! assertions (TAN-193).
//!
//! Each eval drives a real engine loop against a scripted provider (no
//! network), captures the exact provider-facing message vector at the
//! provider boundary, and asserts on what context was injected — not just on
//! the final answer. See `docs/CONTEXT_EVALS.md` for how to add scenarios.

use super::*;

/// Captures the provider-facing message vector at the provider boundary so
/// evals can assert on injected context rather than only on completions.
struct MessagesCaptureProvider {
    captured: Arc<std::sync::Mutex<Option<Vec<ChatMessage>>>>,
}

#[async_trait]
impl Provider for MessagesCaptureProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "scripted-provider-stream".to_string(),
            name: "Messages Capture".to_string(),
            models: vec![tandem_types::ModelInfo {
                id: "scripted-model".to_string(),
                provider_id: "scripted-provider-stream".to_string(),
                display_name: "Scripted Model".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("complete fallback".to_string())
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
        *self.captured.lock().unwrap() = Some(messages);
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta("ok".to_string())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

/// Everything an eval can assert on: the provider-facing context, the final
/// budget telemetry, and all engine events from the run. No raw prompt
/// bodies are persisted anywhere — captures live in test memory only.
struct ContextEvalRun {
    final_messages: Vec<ChatMessage>,
    budget_event: Value,
    events: Vec<(String, Value)>,
}

impl ContextEvalRun {
    fn final_context_contains(&self, needle: &str) -> bool {
        self.final_messages
            .iter()
            .any(|message| message.content.contains(needle))
    }

    /// TAN-193: a lossy compaction projection must carry retrievable source
    /// handles; this fails with the offending note text when handles are
    /// missing.
    fn assert_compaction_has_provenance_handles(&self) {
        let note = self
            .final_messages
            .iter()
            .find(|message| message.content.contains("[history compacted:"))
            .unwrap_or_else(|| {
                panic!(
                    "expected a compaction note in final context; got {} messages without one",
                    self.final_messages.len()
                )
            });
        assert!(
            note.content.contains("source messages"),
            "lossy compaction projection lacks source range handles: {}",
            note.content
        );
        assert!(
            note.content.contains("(ids "),
            "lossy compaction projection lacks source message id handles: {}",
            note.content
        );
    }

    fn budget_u64(&self, key: &str) -> u64 {
        self.budget_event
            .get(key)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| {
                panic!(
                    "context.budget.final missing `{key}`: {}",
                    self.budget_event
                )
            })
    }

    fn budget_bool(&self, key: &str) -> bool {
        self.budget_event
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                panic!(
                    "context.budget.final missing `{key}`: {}",
                    self.budget_event
                )
            })
    }
}

/// Seeds a session with `seed` messages, runs one prompt through the engine
/// loop with a capture provider, and returns the captured provider-facing
/// context plus telemetry.
async fn run_context_eval(
    base: &std::path::Path,
    seed: Vec<Message>,
    prompt: &str,
) -> (ContextEvalRun, Arc<Storage>, String) {
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(MessagesCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(base, provider).await;
    let mut session = Session::new(Some("context eval".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    for message in seed {
        storage
            .append_message(&session_id, message)
            .await
            .expect("seed message");
    }
    let mut rx = bus.subscribe();

    engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: prompt.to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await
        .expect("eval prompt runs");

    let mut events = Vec::new();
    let mut budget_event = Value::Null;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "context.budget.final" {
            budget_event = event.properties.clone();
        }
        events.push((event.event_type.clone(), event.properties.clone()));
    }
    let final_messages = captured
        .lock()
        .unwrap()
        .clone()
        .expect("provider received final context");
    assert!(
        !budget_event.is_null(),
        "context.budget.final must fire for every eval run"
    );
    (
        ContextEvalRun {
            final_messages,
            budget_event,
            events,
        },
        storage,
        session_id,
    )
}

fn seed_text_turn(index: usize, text: &str) -> Message {
    Message::new(
        if index % 2 == 0 {
            MessageRole::User
        } else {
            MessageRole::Assistant
        },
        vec![MessagePart::Text {
            text: text.to_string(),
        }],
    )
}

/// TAN-192 scenario 1: a 10-turn chat followed by a turn-11 follow-up. The
/// task goal stated in turn 1 must still be in the provider-facing context,
/// and telemetry must report that no compaction was needed.
#[tokio::test]
async fn context_eval_ten_turn_chat_followup_retains_task_goal() {
    let base = std::env::temp_dir().join(format!("context-eval-ten-turn-{}", Uuid::new_v4()));
    let mut seed = vec![seed_text_turn(
        0,
        "Task goal: ship the billing migration runbook by Friday.",
    )];
    for i in 1..10 {
        seed.push(seed_text_turn(
            i,
            &format!("turn-{i}: progress discussion about step {i} of the runbook."),
        ));
    }

    let (run, _storage, _session_id) =
        run_context_eval(&base, seed, "What was the task goal again?").await;

    assert!(
        run.final_context_contains("ship the billing migration runbook"),
        "turn-11 follow-up lost the task goal from turn 1"
    );
    assert!(!run.budget_bool("compactionOccurred"));
    assert_eq!(run.budget_u64("droppedHistoryMessages"), 0);
    let _ = std::fs::remove_dir_all(base);
}

/// TAN-192 scenario 2 + TAN-193: a human approval boundary set early in a
/// long session must survive compaction and dozens of unrelated turns, and
/// remain traceable in the final provider-facing context. This eval fails if
/// the approval boundary is missing from final context.
#[tokio::test]
async fn context_eval_approval_boundary_survives_compaction_and_unrelated_turns() {
    let base = std::env::temp_dir().join(format!("context-eval-approval-{}", Uuid::new_v4()));
    let mut seed = Vec::new();
    for i in 0..60 {
        if i == 2 {
            seed.push(seed_text_turn(
                i,
                "Approval granted: production deploy of the billing service.",
            ));
        } else {
            seed.push(seed_text_turn(
                i,
                &format!(
                    "turn-{i}: unrelated side discussion about week {i} planning and chores. {}",
                    "filler ".repeat(20)
                ),
            ));
        }
    }
    let raw_seed_count = 60;

    let (run, storage, session_id) =
        run_context_eval(&base, seed, "Proceed with the next deploy step.").await;

    assert!(
        run.budget_bool("compactionOccurred"),
        "scenario requires the old turns to be compacted"
    );
    assert!(
        run.final_context_contains("Approval granted: production deploy of the billing service"),
        "human approval boundary was lost from the provider-facing context after compaction"
    );
    assert!(
        run.final_messages.iter().any(|message| {
            message.content.contains("pinned from compacted history")
                && message.content.contains("Approval granted")
        }),
        "approval boundary should be carried as a pinned projection with provenance"
    );
    assert!(run.budget_u64("pinnedHistoryMessages") >= 1);

    // Raw stored history is the source of truth and must not shrink.
    let raw = storage
        .get_session(&session_id)
        .await
        .expect("session stored");
    assert!(
        raw.messages.len() > raw_seed_count,
        "raw history intact plus the new turn"
    );
    let _ = std::fs::remove_dir_all(base);
}

/// TAN-192 scenario 3: a tool-heavy session with large intermediate outputs.
/// Old logs must not drown the current task: the provider-facing projection
/// must be measurably smaller than raw history, with telemetry reporting the
/// tool-result compaction that made it so.
#[tokio::test]
async fn context_eval_tool_heavy_run_keeps_final_prompt_bounded() {
    let base = std::env::temp_dir().join(format!("context-eval-tool-heavy-{}", Uuid::new_v4()));
    let mut seed = Vec::new();
    let mut raw_chars = 0usize;
    for i in 0..24 {
        if i % 2 == 1 {
            let output = format!(
                "run-{i} log start\n{}\nrun-{i} exit 0",
                "log line\n".repeat(900)
            );
            raw_chars += output.len();
            seed.push(Message::new(
                MessageRole::Assistant,
                vec![MessagePart::ToolInvocation {
                    tool: "bash".to_string(),
                    args: json!({"command": format!("./build.sh --step {i}")}),
                    result: Some(json!({"output": output})),
                    error: None,
                }],
            ));
        } else {
            seed.push(seed_text_turn(
                i,
                &format!("turn-{i}: keep building step {i}."),
            ));
        }
    }

    let (run, _storage, _session_id) =
        run_context_eval(&base, seed, "Summarize the current build status.").await;

    assert!(run.budget_u64("toolResultsCompacted") > 0);
    assert!(run.budget_u64("toolResultCharsSaved") > 10_000);
    let final_chars = run.budget_u64("finalMessageChars") as usize;
    assert!(
        final_chars < raw_chars / 2,
        "final prompt ({final_chars} chars) must be far below raw tool output volume ({raw_chars} chars)"
    );
    // Exit-status tails survive head/tail compaction for shell results.
    assert!(run.final_context_contains("exit 0"));
    let _ = std::fs::remove_dir_all(base);
}

/// TAN-192 scenario 6 + TAN-193: when history is compacted, the lossy
/// projection must keep retrievable provenance handles (source ranges and
/// stored message ids). This eval fails if the projection lacks handles.
#[tokio::test]
async fn context_eval_compacted_history_retains_provenance_handles() {
    let base = std::env::temp_dir().join(format!("context-eval-provenance-{}", Uuid::new_v4()));
    let mut seed = Vec::new();
    let mut first_id = None;
    for i in 0..50 {
        let message = seed_text_turn(i, &format!("turn-{i}: ongoing analysis notes for {i}."));
        if i == 0 {
            first_id = Some(message.id.clone());
        }
        seed.push(message);
    }

    let (run, _storage, _session_id) =
        run_context_eval(&base, seed, "Continue the analysis.").await;

    assert!(run.budget_u64("droppedHistoryMessages") > 0);
    run.assert_compaction_has_provenance_handles();
    let first_id = first_id.expect("first seed id");
    assert!(
        run.final_context_contains(&first_id),
        "compaction note must cite the stored id of the first dropped message"
    );
    // Telemetry mirrors the projection so dashboards can track it.
    assert!(run.budget_bool("compactionOccurred"));
    assert!(run
        .events
        .iter()
        .any(|(event_type, _)| event_type == "context.profile.selected"));
    let _ = std::fs::remove_dir_all(base);
}
