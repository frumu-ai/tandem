const BUG_MONITOR_SECURITY_POSTURE_SCHEMA_VERSION: u64 = 1;

const BUG_MONITOR_POSTURE_RULES: [(&str, &str, &str); 8] = [
    (
        "high_risk_tool_without_approval",
        "missing_approval",
        "high",
    ),
    (
        "mcp_server_without_tool_allowlist",
        "mcp_allowlist_gap",
        "high",
    ),
    (
        "broad_source_destination_scope",
        "scoped_intake_gap",
        "medium",
    ),
    ("missing_tenant_context", "tenant_context_gap", "high"),
    (
        "external_destination_without_approval",
        "unsafe_destination",
        "medium",
    ),
    (
        "unready_destination_fail_open",
        "unsafe_destination",
        "medium",
    ),
    ("external_action_missing_receipt", "audit_gap", "medium"),
    ("recurring_denied_action", "recurrence_gap", "medium"),
];

#[derive(Debug, Default, serde::Deserialize)]
pub(super) struct BugMonitorPostureChecksQuery {
    pub mode: Option<String>,
    pub rules: Option<String>,
    pub disabled_rules: Option<String>,
    pub min_severity: Option<String>,
}

#[derive(Debug)]
struct BugMonitorPostureRulePolicy {
    mode: String,
    enabled_rules: HashSet<String>,
    disabled_rules: HashSet<String>,
    min_severity_rank: u8,
}

impl BugMonitorPostureRulePolicy {
    fn from_query(query: &BugMonitorPostureChecksQuery) -> Self {
        let mode = bug_monitor_posture_mode(query.mode.as_deref());
        let requested_rules = bug_monitor_posture_query_set(query.rules.as_deref());
        let mut enabled_rules = if requested_rules.is_empty() {
            BUG_MONITOR_POSTURE_RULES
                .iter()
                .map(|(rule_id, _, _)| (*rule_id).to_string())
                .collect::<HashSet<_>>()
        } else {
            requested_rules
        };
        let mut disabled_rules = bug_monitor_posture_query_set(query.disabled_rules.as_deref());
        if mode == "disabled" {
            disabled_rules.extend(
                BUG_MONITOR_POSTURE_RULES
                    .iter()
                    .map(|(rule_id, _, _)| (*rule_id).to_string()),
            );
            enabled_rules.clear();
        } else {
            for rule_id in &disabled_rules {
                enabled_rules.remove(rule_id);
            }
        }
        Self {
            mode,
            enabled_rules,
            disabled_rules,
            min_severity_rank: bug_monitor_posture_severity_rank(
                query.min_severity.as_deref().unwrap_or("low"),
            ),
        }
    }

    fn rule_enabled(&self, rule_id: &str) -> bool {
        self.mode != "disabled"
            && self.enabled_rules.contains(rule_id)
            && !self.disabled_rules.contains(rule_id)
    }

    fn allows_severity(&self, severity: &str) -> bool {
        bug_monitor_posture_severity_rank(severity) >= self.min_severity_rank
    }

    fn dry_run(&self) -> bool {
        self.mode != "enabled"
    }
}

pub(super) async fn get_bug_monitor_security_posture_checks(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<BugMonitorPostureChecksQuery>,
) -> Response {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let authority_inventory =
        bug_monitor_authority_inventory_payload(&state, tenant_context, verified).await;
    let policy = BugMonitorPostureRulePolicy::from_query(&query);
    let findings = bug_monitor_security_posture_findings(&authority_inventory, &policy);
    let counts = json!({
        "findings": findings.len(),
        "by_severity": bug_monitor_posture_counts_by(&findings, "severity"),
        "by_category": bug_monitor_posture_counts_by(&findings, "category"),
        "active_rules": policy.enabled_rules.len(),
    });
    Json(json!({
        "schema_version": BUG_MONITOR_SECURITY_POSTURE_SCHEMA_VERSION,
        "generated_at_ms": crate::now_ms(),
        "scope": {
            "source": "bug_monitor_security_posture_checks",
            "read_only": true,
            "dry_run": policy.dry_run(),
            "authority_inventory_source": authority_inventory.get("scope").and_then(|scope| scope.get("source")).cloned().unwrap_or(Value::Null),
        },
        "baseline_policy": {
            "mode": policy.mode.clone(),
            "dry_run": policy.dry_run(),
            "read_only": true,
            "supported_modes": ["dry_run", "enabled", "disabled"],
            "enabled_rule_ids": bug_monitor_posture_sorted_set(&policy.enabled_rules),
            "disabled_rule_ids": bug_monitor_posture_sorted_set(&policy.disabled_rules),
            "min_severity": query.min_severity.as_deref().unwrap_or("low"),
        },
        "rules": bug_monitor_posture_rule_states(&policy),
        "counts": counts,
        "findings": findings,
        "authority_inventory": {
            "schema_version": authority_inventory.get("schema_version").cloned().unwrap_or(Value::Null),
            "generated_at_ms": authority_inventory.get("generated_at_ms").cloned().unwrap_or(Value::Null),
            "counts": authority_inventory.get("counts").cloned().unwrap_or(Value::Null),
            "incident_history_inputs": [
                "inventory.policy_decisions",
                "inventory.external_publish_surfaces.recent_actions"
            ],
        },
        "draft_conversion": {
            "available": true,
            "method": "submit finding.incident_draft_suggestion through the normal Bug Monitor report or draft flow",
            "mutates_state": false,
        },
        "sensitive_values": {
            "policy": "redacted_or_summarized",
            "omitted_fields": authority_inventory.get("sensitive_values").and_then(|value| value.get("omitted_fields")).cloned().unwrap_or(Value::Array(Vec::new())),
        },
    }))
    .into_response()
}

fn bug_monitor_security_posture_findings(
    authority_inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
) -> Vec<Value> {
    let inventory = authority_inventory
        .get("inventory")
        .unwrap_or(&Value::Null);
    let mut findings = Vec::new();
    let mut seen = HashSet::new();

    bug_monitor_posture_check_automation_authority(inventory, policy, &mut findings, &mut seen);
    bug_monitor_posture_check_sources(inventory, policy, &mut findings, &mut seen);
    bug_monitor_posture_check_destinations(inventory, policy, &mut findings, &mut seen);
    bug_monitor_posture_check_external_actions(inventory, policy, &mut findings, &mut seen);
    bug_monitor_posture_check_policy_decisions(inventory, policy, &mut findings, &mut seen);

    findings
}

fn bug_monitor_posture_check_automation_authority(
    inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    for automation in bug_monitor_posture_array(inventory, &["automation_specs"]) {
        let automation_id = bug_monitor_posture_str(automation, "automation_id")
            .unwrap_or("unknown_automation");
        let automation_name = bug_monitor_posture_str(automation, "name").unwrap_or(automation_id);
        if bug_monitor_posture_tenant_context_gap(automation.get("tenant_context")) {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "missing_tenant_context",
                "tenant_context_gap",
                "high",
                format!("Automation `{automation_name}` is using local or incomplete tenant context"),
                "Automation authority should be bound to an explicit organization and workspace before it can be evaluated as production-ready.".to_string(),
                vec![json!({
                    "kind": "automation",
                    "id": automation_id,
                    "name": automation_name,
                })],
                vec![bug_monitor_posture_evidence_ref(format!(
                    "inventory.automation_specs[{automation_id}].tenant_context"
                ))],
                "Set an explicit tenant context on the automation and require verified tenant context on external triggers.",
            );
        }

        let agents = bug_monitor_posture_array(automation, &["agents"]);
        let nodes = bug_monitor_posture_array(automation, &["nodes"]);
        let mut agent_allowlists = std::collections::BTreeMap::<String, Vec<String>>::new();
        let mut agent_has_approval_policy = std::collections::BTreeMap::<String, bool>::new();
        let mut agent_names = std::collections::BTreeMap::<String, String>::new();

        for agent in agents {
            let agent_id = bug_monitor_posture_str(agent, "agent_id").unwrap_or("unknown_agent");
            let agent_name = bug_monitor_posture_str(agent, "display_name").unwrap_or(agent_id);
            let allowlist = bug_monitor_posture_string_array(agent, &["tool_policy", "allowlist"]);
            let approval_required_tools =
                bug_monitor_posture_approval_required_tools(allowlist.as_slice());
            let has_approval_policy =
                bug_monitor_posture_has_non_empty(agent.get("approval_policy"));
            agent_allowlists.insert(agent_id.to_string(), allowlist);
            agent_has_approval_policy.insert(agent_id.to_string(), has_approval_policy);
            agent_names.insert(agent_id.to_string(), agent_name.to_string());
            if !approval_required_tools.is_empty()
                && !has_approval_policy
                && !bug_monitor_posture_agent_has_nodes(nodes, agent_id)
            {
                bug_monitor_posture_push_finding(
                    findings,
                    seen,
                    policy,
                    "high_risk_tool_without_approval",
                    "missing_approval",
                    "high",
                    format!("Agent `{agent_name}` can use approval-required tools without approval"),
                    format!(
                        "Agent `{agent_id}` allows approval-required tools but has no approval policy or required approval gate."
                    ),
                    vec![json!({
                        "kind": "automation_agent",
                        "id": agent_id,
                        "name": agent_name,
                        "automation_id": automation_id,
                    })],
                    vec![
                        bug_monitor_posture_evidence_ref(format!(
                            "inventory.automation_specs[{automation_id}].agents[{agent_id}].tool_policy.allowlist"
                        )),
                        bug_monitor_posture_evidence_ref(format!(
                            "inventory.automation_specs[{automation_id}].agents[{agent_id}].approval_policy"
                        )),
                    ],
                    "Add an agent approval policy, add a required approval gate before the mutating step, or remove the mutating tools from the allowlist.",
                );
            }

            let allowed_servers =
                bug_monitor_posture_string_array(agent, &["mcp_policy", "effective_allowed_servers"]);
            let allowed_tools = bug_monitor_posture_string_array(agent, &["mcp_policy", "allowed_tools"]);
            if !allowed_servers.is_empty() && allowed_tools.is_empty() {
                bug_monitor_posture_push_finding(
                    findings,
                    seen,
                    policy,
                    "mcp_server_without_tool_allowlist",
                    "mcp_allowlist_gap",
                    "high",
                    format!("Agent `{agent_name}` can reach MCP servers without a tool allowlist"),
                    format!(
                        "Agent `{agent_id}` grants MCP server access but does not pin allowed tool names."
                    ),
                    vec![json!({
                        "kind": "automation_agent",
                        "id": agent_id,
                        "name": agent_name,
                        "automation_id": automation_id,
                    })],
                    vec![bug_monitor_posture_evidence_ref(format!(
                        "inventory.automation_specs[{automation_id}].agents[{agent_id}].mcp_policy"
                    ))],
                    "Restrict the MCP policy with explicit allowed_tools entries and require approval for mutating MCP tools.",
                );
            }
        }

        for node in nodes {
            let node_id = bug_monitor_posture_str(node, "node_id").unwrap_or("unknown_node");
            let node_agent_id = bug_monitor_posture_str(node, "agent_id").unwrap_or_default();
            let node_gate_required = node
                .get("gate")
                .and_then(|gate| gate.get("required"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let node_has_explicit_tool_policy = node
                .get("tool_policy")
                .is_some_and(|value| !value.is_null());
            let effective_allowlist = if node_has_explicit_tool_policy {
                bug_monitor_posture_string_array(node, &["tool_policy", "allowlist"])
            } else {
                agent_allowlists
                    .get(node_agent_id)
                    .cloned()
                    .unwrap_or_default()
            };
            let node_approval_required_tools =
                bug_monitor_posture_approval_required_tools(effective_allowlist.as_slice());
            let agent_approval_policy = agent_has_approval_policy
                .get(node_agent_id)
                .copied()
                .unwrap_or(false);
            if !node_approval_required_tools.is_empty()
                && !node_gate_required
                && !agent_approval_policy
            {
                let tool_policy_evidence_path = if node_has_explicit_tool_policy {
                    format!(
                        "inventory.automation_specs[{automation_id}].nodes[{node_id}].tool_policy.allowlist"
                    )
                } else {
                    format!(
                        "inventory.automation_specs[{automation_id}].agents[{node_agent_id}].tool_policy.allowlist"
                    )
                };
                let node_agent_name = agent_names
                    .get(node_agent_id)
                    .map(String::as_str)
                    .unwrap_or(node_agent_id);
                bug_monitor_posture_push_finding(
                    findings,
                    seen,
                    policy,
                    "high_risk_tool_without_approval",
                    "missing_approval",
                    "high",
                    format!("Automation node `{node_id}` can call approval-required tools without a required gate"),
                    format!(
                        "Node `{node_id}` for agent `{node_agent_name}` can use approval-required tools and does not declare a required approval gate."
                    ),
                    vec![json!({
                        "kind": "automation_node",
                        "id": node_id,
                        "automation_id": automation_id,
                        "agent_id": node_agent_id,
                    })],
                    vec![
                        bug_monitor_posture_evidence_ref(tool_policy_evidence_path),
                        bug_monitor_posture_evidence_ref(format!(
                            "inventory.automation_specs[{automation_id}].nodes[{node_id}].gate.required"
                        )),
                    ],
                    "Add a required approval gate to the node or down-scope the node tool policy to read-only tools.",
                );
            }
        }
    }
}

fn bug_monitor_posture_check_sources(
    inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    let external_destinations = bug_monitor_posture_array(inventory, &["destinations"])
        .iter()
        .filter(|destination| {
            destination
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
                && bug_monitor_posture_destination_is_external(destination)
        })
        .filter_map(|destination| bug_monitor_posture_str(destination, "destination_id"))
        .collect::<Vec<_>>();

    for source in bug_monitor_posture_array(inventory, &["monitored_sources"]) {
        if !source
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            continue;
        }
        let project_id = bug_monitor_posture_str(source, "project_id").unwrap_or("unknown_project");
        let source_id = bug_monitor_posture_str(source, "source_id").unwrap_or(project_id);
        let source_kind = bug_monitor_posture_str(source, "source_kind").unwrap_or("unknown");
        let affected = vec![json!({
            "kind": "monitored_source",
            "id": source_id,
            "project_id": project_id,
            "source_kind": source_kind,
        })];
        if bug_monitor_posture_option_gap(source.get("tenant_id"))
            || bug_monitor_posture_option_gap(source.get("workspace_id"))
        {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "missing_tenant_context",
                "tenant_context_gap",
                "high",
                format!("Monitored source `{source_id}` is missing tenant or workspace context"),
                "External incident sources should carry explicit tenant and workspace binding before routing to external destinations.".to_string(),
                affected.clone(),
                vec![bug_monitor_posture_evidence_ref(format!(
                    "inventory.monitored_sources[{source_id}].tenant_id"
                ))],
                "Set tenant_id and workspace_id on the project or log source binding.",
            );
        }

        if external_destinations.is_empty() {
            continue;
        }
        let allowed_destination_ids =
            bug_monitor_posture_string_array(source, &["allowed_destination_ids"]);
        let broad_scope = allowed_destination_ids.is_empty()
            || external_destinations
                .iter()
                .all(|destination_id| allowed_destination_ids.iter().any(|value| value == destination_id));
        if broad_scope {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "broad_source_destination_scope",
                "scoped_intake_gap",
                "medium",
                format!("Monitored source `{source_id}` can route to every external destination"),
                "A monitored source with no destination allowlist, or one that covers every external destination, can bypass destination-specific scoping.".to_string(),
                affected,
                vec![bug_monitor_posture_evidence_ref(format!(
                    "inventory.monitored_sources[{source_id}].allowed_destination_ids"
                ))],
                "Constrain allowed_destination_ids to the smallest approved destination set and use route tags for controlled fan-out.",
            );
        }
    }
}

fn bug_monitor_posture_check_destinations(
    inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    let safety_defaults = inventory
        .get("bug_monitor")
        .and_then(|config| config.get("safety_defaults"))
        .unwrap_or(&Value::Null);
    let block_unready = safety_defaults
        .get("block_unready_destinations")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let require_high_risk_approval = safety_defaults
        .get("require_approval_for_high_risk")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    for destination in bug_monitor_posture_array(inventory, &["destinations"]) {
        if !destination
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            continue;
        }
        if !bug_monitor_posture_destination_is_external(destination) {
            continue;
        }
        let destination_id =
            bug_monitor_posture_str(destination, "destination_id").unwrap_or("unknown_destination");
        let name = bug_monitor_posture_str(destination, "name").unwrap_or(destination_id);
        let kind = bug_monitor_posture_str(destination, "kind").unwrap_or("unknown");
        let affected = vec![json!({
            "kind": "bug_monitor_destination",
            "id": destination_id,
            "name": name,
            "destination_kind": kind,
        })];
        let require_approval = destination
            .get("require_approval")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !require_approval {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "external_destination_without_approval",
                "unsafe_destination",
                "medium",
                format!("External destination `{name}` does not require approval"),
                "External destinations should require approval unless they are explicitly scoped to a non-mutating sink.".to_string(),
                affected.clone(),
                vec![bug_monitor_posture_evidence_ref(format!(
                    "inventory.destinations[{destination_id}].require_approval"
                ))],
                "Enable require_approval on the destination or restrict routes so only low-risk internal events can reach it.",
            );
        }
        if !require_high_risk_approval && !require_approval {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "external_destination_without_approval",
                "missing_approval",
                "high",
                format!("High-risk events can publish to `{name}` without explicit approval"),
                "The Bug Monitor high-risk approval default is disabled and this external destination does not require destination-level approval.".to_string(),
                affected.clone(),
                vec![
                    bug_monitor_posture_evidence_ref(
                        "inventory.bug_monitor.safety_defaults.require_approval_for_high_risk",
                    ),
                    bug_monitor_posture_evidence_ref(format!(
                        "inventory.destinations[{destination_id}].require_approval"
                    )),
                ],
                "Enable require_approval_for_high_risk and set require_approval on all external destinations that can create or mutate records.",
            );
        }
        if !block_unready && !bug_monitor_posture_destination_has_required_binding(destination) {
            bug_monitor_posture_push_finding(
                findings,
                seen,
                policy,
                "unready_destination_fail_open",
                "unsafe_destination",
                "medium",
                format!("Destination `{name}` can fail open when readiness is incomplete"),
                "The destination is enabled but its required binding fields are incomplete, and block_unready_destinations is disabled.".to_string(),
                affected,
                vec![
                    bug_monitor_posture_evidence_ref(format!(
                        "inventory.destinations[{destination_id}]"
                    )),
                    bug_monitor_posture_evidence_ref(
                        "inventory.bug_monitor.safety_defaults.block_unready_destinations",
                    ),
                ],
                "Complete the destination binding or enable block_unready_destinations so incomplete sinks fail closed.",
            );
        }
    }
}

fn bug_monitor_posture_check_external_actions(
    inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    for action in bug_monitor_posture_array(
        inventory,
        &["external_publish_surfaces", "recent_actions"],
    ) {
        let status = bug_monitor_posture_str(action, "status").unwrap_or_default();
        if status != "posted" && status != "success" {
            continue;
        }
        let idempotency = action
            .get("idempotency_key_present")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let receipt = action
            .get("receipt_present")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if idempotency && receipt {
            continue;
        }
        let action_id = bug_monitor_posture_str(action, "action_id").unwrap_or("unknown_action");
        let operation = bug_monitor_posture_str(action, "operation").unwrap_or("external_action");
        bug_monitor_posture_push_finding(
            findings,
            seen,
            policy,
            "external_action_missing_receipt",
            "audit_gap",
            "medium",
            format!("External action `{operation}` is missing idempotency or receipt evidence"),
            "A successful external action without an idempotency key and receipt weakens replay protection and audit evidence.".to_string(),
            vec![json!({
                "kind": "external_action",
                "id": action_id,
                "operation": operation,
            })],
            vec![bug_monitor_posture_evidence_ref(format!(
                "inventory.external_publish_surfaces.recent_actions[{action_id}]"
            ))],
            "Persist idempotency keys and external receipts for every successful destination publish.",
        );
    }
}

fn bug_monitor_posture_check_policy_decisions(
    inventory: &Value,
    policy: &BugMonitorPostureRulePolicy,
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    let mut denied = std::collections::BTreeMap::<String, (usize, String, String)>::new();
    for decision in bug_monitor_posture_array(inventory, &["policy_decisions"]) {
        let decision_value = bug_monitor_posture_str(decision, "decision").unwrap_or_default();
        if decision_value != "deny" && decision_value != "denied" {
            continue;
        }
        let tool = bug_monitor_posture_str(decision, "tool").unwrap_or("unknown_tool");
        let resource = bug_monitor_posture_str(decision, "resource").unwrap_or("unknown_resource");
        let key = format!("{tool}|{resource}");
        let entry = denied
            .entry(key)
            .or_insert_with(|| (0, tool.to_string(), resource.to_string()));
        entry.0 += 1;
    }
    for (_, (count, tool, resource)) in denied {
        if count < 2 {
            continue;
        }
        bug_monitor_posture_push_finding(
            findings,
            seen,
            policy,
            "recurring_denied_action",
            "recurrence_gap",
            "medium",
            format!("Policy repeatedly denied `{tool}` for the same resource"),
            format!(
                "The same tool/resource pair has {count} recent denied policy decisions, which indicates unresolved authority pressure."
            ),
            vec![json!({
                "kind": "policy_decision_cluster",
                "id": format!("{tool}:{resource}"),
                "tool": tool,
            })],
            vec![bug_monitor_posture_evidence_ref(
                "inventory.policy_decisions[decision=deny]",
            )],
            "Create a scoped approval, tighten the automation/tool policy, or route the denied pattern to security review.",
        );
    }
}

fn bug_monitor_posture_push_finding(
    findings: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    policy: &BugMonitorPostureRulePolicy,
    rule_id: &str,
    category: &str,
    severity: &str,
    title: String,
    detail: String,
    affected_objects: Vec<Value>,
    evidence_refs: Vec<Value>,
    recommendation: &str,
) {
    if !policy.rule_enabled(rule_id) || !policy.allows_severity(severity) {
        return;
    }
    let affected_key = affected_objects
        .iter()
        .filter_map(|object| object.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("|");
    let hash = crate::sha256_hex(&[rule_id, category, severity, affected_key.as_str()]);
    let fingerprint = format!("sha256:{hash}");
    if !seen.insert(fingerprint.clone()) {
        return;
    }
    let evidence_excerpt = evidence_refs
        .iter()
        .filter_map(|evidence| evidence.get("path").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let finding_id = format!(
        "bpf_{}",
        hash.get(..16).unwrap_or(hash.as_str())
    );
    findings.push(json!({
        "finding_id": finding_id,
        "fingerprint": fingerprint.clone(),
        "rule_id": rule_id,
        "category": category,
        "severity": severity,
        "title": title.clone(),
        "detail": detail.clone(),
        "affected_objects": affected_objects,
        "evidence_refs": evidence_refs,
        "recommendation": recommendation,
        "route_suggestion": {
            "route_tags": ["security_posture", category],
            "risk_level": bug_monitor_posture_risk_level(severity),
            "risk_category": category,
            "expected_destination": "security_review",
        },
        "destination_suggestion": {
            "kind": "linear_issue",
            "require_approval": true,
            "idempotency_key": "finding.fingerprint",
            "receipt_required": true,
        },
        "incident_draft_suggestion": {
            "source": "security_posture",
            "event_type": "security.posture.finding",
            "title": title,
            "fingerprint": fingerprint,
            "detail": format!("{detail}\n\nRecommended mitigation: {recommendation}"),
            "risk_level": bug_monitor_posture_risk_level(severity),
            "risk_category": category,
            "confidence": "high",
            "route_tags": ["security_posture", category],
            "excerpt": evidence_excerpt,
        },
        "dry_run": policy.dry_run(),
    }));
}

fn bug_monitor_posture_rule_states(policy: &BugMonitorPostureRulePolicy) -> Vec<Value> {
    BUG_MONITOR_POSTURE_RULES
        .iter()
        .map(|(rule_id, category, severity)| {
            json!({
                "rule_id": rule_id,
                "category": category,
                "default_severity": severity,
                "enabled": policy.rule_enabled(rule_id),
                "dry_run": policy.dry_run(),
            })
        })
        .collect()
}

fn bug_monitor_posture_mode(mode: Option<&str>) -> String {
    match mode
        .unwrap_or("dry_run")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "enabled" | "enable" | "enforced" => "enabled".to_string(),
        "disabled" | "disable" | "off" => "disabled".to_string(),
        _ => "dry_run".to_string(),
    }
}

fn bug_monitor_posture_query_set(value: Option<&str>) -> HashSet<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn bug_monitor_posture_sorted_set(values: &HashSet<String>) -> Vec<String> {
    let mut sorted = values.iter().cloned().collect::<Vec<_>>();
    sorted.sort();
    sorted
}

fn bug_monitor_posture_counts_by(findings: &[Value], key: &str) -> Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for finding in findings {
        let Some(value) = finding.get(key).and_then(Value::as_str) else {
            continue;
        };
        *counts.entry(value.to_string()).or_insert(0) += 1;
    }
    json!(counts)
}

fn bug_monitor_posture_array<'a>(value: &'a Value, path: &[&str]) -> &'a [Value] {
    let mut current = value;
    for segment in path {
        let Some(next) = current.get(segment) else {
            return &[];
        };
        current = next;
    }
    current.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn bug_monitor_posture_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|value| {
        let trimmed = value.trim();
        !trimmed.is_empty() && trimmed != "null"
    })
}

fn bug_monitor_posture_string_array(value: &Value, path: &[&str]) -> Vec<String> {
    let mut current = value;
    for segment in path {
        let Some(next) = current.get(segment) else {
            return Vec::new();
        };
        current = next;
    }
    current
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn bug_monitor_posture_has_non_empty(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

fn bug_monitor_posture_agent_has_nodes(nodes: &[Value], agent_id: &str) -> bool {
    nodes
        .iter()
        .any(|node| bug_monitor_posture_str(node, "agent_id") == Some(agent_id))
}

fn bug_monitor_posture_approval_required_tools(allowlist: &[String]) -> Vec<String> {
    if tandem_tools::approval_classifier::allowlist_is_wildcard(allowlist) {
        return allowlist.to_vec();
    }
    allowlist
        .iter()
        .filter(|tool| {
            matches!(
                tandem_tools::approval_classifier::classify(tool),
                tandem_tools::approval_classifier::ApprovalClassification::RequiresApproval
            )
        })
        .cloned()
        .collect()
}

fn bug_monitor_posture_tenant_context_gap(value: Option<&Value>) -> bool {
    let Some(Value::Object(context)) = value else {
        return true;
    };
    let org_id = context
        .get("org_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let workspace_id = context
        .get("workspace_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let source = context
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    org_id.is_empty()
        || workspace_id.is_empty()
        || (org_id == "local" && workspace_id == "local")
        || source == "LocalImplicit"
}

fn bug_monitor_posture_option_gap(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => {
            let value = value.trim();
            value.is_empty() || value == "local"
        }
        _ => true,
    }
}

fn bug_monitor_posture_destination_is_external(destination: &Value) -> bool {
    matches!(
        bug_monitor_posture_str(destination, "kind"),
        Some("github_issue" | "linear_issue" | "webhook" | "mcp_tool")
    )
}

fn bug_monitor_posture_destination_has_required_binding(destination: &Value) -> bool {
    match bug_monitor_posture_str(destination, "kind") {
        Some("github_issue") => bug_monitor_posture_has_non_empty(destination.get("repo")),
        Some("linear_issue") => bug_monitor_posture_has_non_empty(destination.get("linear_team")),
        Some("webhook") => destination
            .get("webhook_url_present")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        Some("mcp_tool") => {
            bug_monitor_posture_has_non_empty(destination.get("mcp_server"))
                && bug_monitor_posture_has_non_empty(destination.get("mcp_tool"))
                && destination
                    .get("allow_publish")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        }
        _ => true,
    }
}

fn bug_monitor_posture_evidence_ref(path: impl Into<String>) -> Value {
    json!({
        "kind": "authority_inventory",
        "path": path.into(),
    })
}

fn bug_monitor_posture_severity_rank(severity: &str) -> u8 {
    match severity.trim().to_ascii_lowercase().as_str() {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn bug_monitor_posture_risk_level(severity: &str) -> &'static str {
    match severity {
        "critical" | "high" => "high",
        "medium" => "medium",
        _ => "low",
    }
}
