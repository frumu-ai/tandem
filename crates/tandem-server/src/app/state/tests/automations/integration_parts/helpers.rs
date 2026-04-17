#[derive(Clone, Debug)]
struct PromptRecord {
    prompt: String,
    tool_names: Vec<String>,
    tool_mode: String,
    model_override: Option<String>,
}

#[derive(Clone)]
struct ScriptedProvider {
    records: Arc<Mutex<Vec<PromptRecord>>>,
    scripts: Arc<Mutex<VecDeque<Vec<StreamChunk>>>>,
}

impl ScriptedProvider {
    fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(Vec::new())),
            scripts: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    async fn push_script(&self, script: Vec<StreamChunk>) {
        self.scripts.lock().await.push_back(script);
    }

    async fn records(&self) -> Vec<PromptRecord> {
        self.records.lock().await.clone()
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "scripted".to_string(),
            name: "Scripted".to_string(),
            models: vec![ModelInfo {
                id: "scripted-model".to_string(),
                provider_id: "scripted".to_string(),
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
        anyhow::bail!("scripted provider only supports streaming");
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt = messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        let mut tool_names = tools
            .unwrap_or_default()
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();
        tool_names.sort();
        tool_names.dedup();
        self.records.lock().await.push(PromptRecord {
            prompt,
            tool_names,
            tool_mode: format!("{tool_mode:?}"),
            model_override: model_override.map(str::to_string),
        });

        let script = self
            .scripts
            .lock()
            .await
            .pop_front()
            .expect("scripted provider exhausted");

        Ok(Box::pin(stream::iter(script.into_iter().map(Ok))))
    }
}

#[derive(Clone)]
struct RecordingTool {
    schema: ToolSchema,
    output: String,
    metadata: serde_json::Value,
    calls: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl RecordingTool {
    fn new(
        name: &str,
        description: &str,
        input_schema: serde_json::Value,
        output: impl Into<String>,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            schema: ToolSchema::new(name, description, input_schema),
            output: output.into(),
            metadata,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn calls(&self) -> Vec<serde_json::Value> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl Tool for RecordingTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.calls.lock().await.push(args);
        Ok(ToolResult {
            output: self.output.clone(),
            metadata: self.metadata.clone(),
        })
    }
}

fn tool_turn(calls: Vec<(&str, &str, serde_json::Value)>) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    for (index, (id, name, args)) in calls.into_iter().enumerate() {
        let call_id = if id.is_empty() {
            format!("call_{}", index + 1)
        } else {
            id.to_string()
        };
        chunks.push(StreamChunk::ToolCallStart {
            id: call_id.clone(),
            name: name.to_string(),
        });
        chunks.push(StreamChunk::ToolCallDelta {
            id: call_id.clone(),
            args_delta: args.to_string(),
        });
        chunks.push(StreamChunk::ToolCallEnd { id: call_id });
    }
    chunks.push(StreamChunk::Done {
        finish_reason: "tool_calls".to_string(),
        usage: None,
    });
    chunks
}

fn json_tool_turn(tool: &str, args: serde_json::Value) -> Vec<StreamChunk> {
    vec![
        StreamChunk::TextDelta(
            serde_json::to_string(&json!({
                "tool": tool,
                "args": args
            }))
            .expect("tool call json"),
        ),
        StreamChunk::Done {
            finish_reason: "tool_calls".to_string(),
            usage: None,
        },
    ]
}

fn final_turn(text: &str) -> Vec<StreamChunk> {
    vec![
        StreamChunk::TextDelta(text.to_string()),
        StreamChunk::Done {
            finish_reason: "stop".to_string(),
            usage: Some(TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            }),
        },
    ]
}

fn brief_research_node(
    node_id: &str,
    output_path: &str,
    web_research_expected: bool,
) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "researcher".to_string(),
        objective: "Write a research brief grounded in the workspace".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "web_research_expected": web_research_expected,
                "source_coverage_required": true
            }
        })),
    }
}

fn citations_research_node(node_id: &str, output_path: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "researcher".to_string(),
        objective: "Write a grounded citation handoff".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a citation handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "source_coverage_required": true,
                "preferred_mcp_servers": ["tandem-mcp"]
            }
        })),
    }
}

fn analyze_findings_node(
    node_id: &str,
    output_path: &str,
    workspace_file: &str,
) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "analyst".to_string(),
        objective:
            "Synthesize the clustered findings into structured JSON and update the durable analysis file."
                .to_string(),
        depends_on: vec!["cluster_topics".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "cluster_topics".to_string(),
            alias: "clusters".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "output_files": [workspace_file]
            }
        })),
    }
}

fn compare_results_node(node_id: &str, output_path: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "editor".to_string(),
        objective: "Review existing persistent blog memory and recent Tandem blog history to produce a recent blog review.".to_string(),
        depends_on: vec!["collect_inputs".to_string(), "research_sources".to_string()],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: "collect_inputs".to_string(),
                alias: "run_context".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research_sources".to_string(),
                alias: "tandem_grounding".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "preferred_mcp_servers": ["blog-mcp"]
            }
        })),
    }
}

fn delivery_node(node_id: &str, recipient: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "operator".to_string(),
        objective: format!(
            "Send the finalized report to {} using the validated artifact body as the delivery source of truth.",
            recipient
        ),
        depends_on: vec!["generate_report".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "generate_report".to_string(),
            alias: "final_report".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "delivery": {
                "method": "email",
                "to": recipient,
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    }
}

fn code_loop_node(node_id: &str, output_path: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "engineer".to_string(),
        objective:
            "Inspect the code, patch the smallest root cause, rerun verification, and write a concise implementation handoff."
                .to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: Some(json!({
            "max_attempts": 2
        })),
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "verification_command": "cargo test",
                "output_path": output_path
            }
        })),
    }
}

fn automation_with_single_node(
    automation_id: &str,
    node: AutomationFlowNode,
    workspace_root: &std::path::Path,
    allowlist: Vec<String>,
) -> AutomationV2Spec {
    let mut automation = AutomationSpecBuilder::new(automation_id)
        .name(format!("{automation_id} test"))
        .nodes(vec![node])
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .build();
    let agent = automation.agents.first_mut().expect("test agent");
    agent.agent_id = "researcher".to_string();
    agent.template_id = None;
    agent.display_name = "Researcher".to_string();
    agent.tool_policy.allowlist = allowlist;
    agent.tool_policy.denylist.clear();
    agent.mcp_policy.allowed_servers = Vec::new();
    agent.mcp_policy.allowed_tools = None;
    automation
}

async fn install_provider_and_tools(
    state: &AppState,
    provider: &ScriptedProvider,
    tools: Vec<(&str, Arc<RecordingTool>)>,
) {
    state
        .providers
        .replace_for_test(
            vec![Arc::new(provider.clone())],
            Some("scripted".to_string()),
        )
        .await;
    for (name, tool) in tools {
        state.tools.register_tool(name.to_string(), tool).await;
    }
}

fn prompt_contains_only_run_scoped_path(record: &PromptRecord, output_path: &str) {
    assert!(
        record.prompt.contains(output_path),
        "prompt did not include the run-scoped output path {output_path:?}"
    );
    assert!(
        !record.prompt.contains(".tandem/artifacts/"),
        "prompt still mentioned the legacy workspace-scoped artifact path"
    );
}

fn assistant_session_with_tool_invocations(
    title: &str,
    workspace_root: &std::path::Path,
    invocations: Vec<(&str, serde_json::Value, serde_json::Value, Option<&str>)>,
) -> Session {
    let mut session = Session::new(
        Some(title.to_string()),
        Some(workspace_root.to_string_lossy().to_string()),
    );
    session.messages.push(Message::new(
        MessageRole::Assistant,
        invocations
            .into_iter()
            .map(|(tool, args, result, error)| MessagePart::ToolInvocation {
                tool: tool.to_string(),
                args,
                result: Some(result),
                error: error.map(str::to_string),
            })
            .collect(),
    ));
    session
}

async fn persist_validated_output(
    state: &AppState,
    run_id: &str,
    node_id: &str,
    output: serde_json::Value,
    status: AutomationRunStatus,
    attempt: u32,
) {
    state
        .update_automation_v2_run(run_id, |row| {
            row.status = status;
            row.checkpoint
                .node_outputs
                .insert(node_id.to_string(), output.clone());
            row.checkpoint
                .node_attempts
                .insert(node_id.to_string(), attempt);
        })
        .await
        .expect("persist validated output");
}
