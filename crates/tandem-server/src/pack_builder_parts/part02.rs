
fn render_routine_yaml(
    routine_id: &str,
    schedule: &RoutineSchedule,
    schedule_label: &str,
    timezone: &str,
    allowed_tools: &[String],
) -> String {
    let mut lines = vec![format!("id: {}", routine_id), "trigger:".to_string()];

    match schedule {
        RoutineSchedule::Cron { expression } => {
            lines.push("  type: cron".to_string());
            lines.push(format!("  expression: \"{}\"", expression));
        }
        RoutineSchedule::IntervalSeconds { seconds } => {
            lines.push("  type: interval_seconds".to_string());
            lines.push(format!("  seconds: {}", seconds));
        }
    }
    lines.push("mission_id: default".to_string());
    lines.push("enabled_by_default: false".to_string());
    lines.push("".to_string());

    lines.push(format!("routine_id: {}", routine_id));
    lines.push(format!("name: {}", schedule_label));
    lines.push(format!("timezone: {}", timezone));
    match schedule {
        RoutineSchedule::Cron { expression } => {
            lines.push("schedule:".to_string());
            lines.push(format!("  cron: \"{}\"", expression));
        }
        RoutineSchedule::IntervalSeconds { seconds } => {
            lines.push("schedule:".to_string());
            lines.push(format!("  interval_seconds: {}", seconds));
        }
    }
    lines.push("entrypoint: mission.default".to_string());
    lines.push("allowed_tools:".to_string());
    for tool in allowed_tools {
        lines.push(format!("  - {}", tool));
    }
    lines.push("output_targets:".to_string());
    lines.push(format!("  - run/{}/report.md", routine_id));
    lines.push("requires_approval: false".to_string());
    lines.push("external_integrations_allowed: true".to_string());
    lines.join("\n") + "\n"
}

fn render_manifest_yaml(
    pack_id: &str,
    pack_name: &str,
    version: &str,
    required: &[String],
    optional: &[String],
    mission_id: &str,
    routine_id: &str,
) -> String {
    let mut lines = vec![
        "manifest_schema_version: 1".to_string(),
        format!("pack_id: \"{}\"", pack_id),
        format!("name: {}", pack_name),
        format!("version: {}", version),
        "type: workflow".to_string(),
        "entrypoints:".to_string(),
        format!("  missions: [\"{}\"]", mission_id),
        format!("  routines: [\"{}\"]", routine_id),
        "capabilities:".to_string(),
        "  required:".to_string(),
    ];

    if required.is_empty() {
        lines.push("    - websearch".to_string());
    } else {
        for cap in required {
            lines.push(format!("    - {}", cap));
        }
    }

    lines.push("  optional:".to_string());
    for cap in optional {
        lines.push(format!("    - {}", cap));
    }
    if optional.is_empty() {
        lines.push("    - question".to_string());
    }

    lines.push("contents:".to_string());
    lines.push("  agents:".to_string());
    lines.push("    - id: default".to_string());
    lines.push("      path: agents/default.md".to_string());
    lines.push("  missions:".to_string());
    lines.push(format!("    - id: {}", mission_id));
    lines.push("      path: missions/default.yaml".to_string());
    lines.push("  routines:".to_string());
    lines.push(format!("    - id: {}", routine_id));
    lines.push("      path: routines/default.yaml".to_string());
    lines.join("\n") + "\n"
}

fn infer_capabilities_from_goal(goal: &str) -> Vec<CapabilityNeed> {
    let g = goal.to_ascii_lowercase();
    let mut out = Vec::<CapabilityNeed>::new();
    let push_need = |id: &str, external: bool, terms: &[&str], out: &mut Vec<CapabilityNeed>| {
        if out.iter().any(|n| n.id == id) {
            return;
        }
        out.push(CapabilityNeed {
            id: id.to_string(),
            external,
            query_terms: terms.iter().map(|v| v.to_string()).collect(),
        });
    };

    if g.contains("notion") {
        push_need("notion.read_write", true, &["notion"], &mut out);
    }
    if g.contains("slack") {
        push_need("slack.post_message", true, &["slack"], &mut out);
    }
    if g.contains("stripe") || g.contains("payment") {
        push_need("stripe.read_write", true, &["stripe"], &mut out);
    }
    if g.contains("github") || g.contains("pr") {
        push_need("github.read_write", true, &["github"], &mut out);
    }
    if g.contains("headline") || g.contains("news") {
        push_need("news.latest", true, &["news", "zapier"], &mut out);
    }
    if g.contains("email") || contains_email_address(goal) {
        push_need("email.send", true, &["gmail", "email", "zapier"], &mut out);
    }

    push_need("question.ask", false, &["question"], &mut out);
    if out.len() == 1 {
        push_need("web.research", false, &["websearch"], &mut out);
    }
    out
}

fn contains_email_address(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| {
            ch.is_ascii_punctuation() && ch != '@' && ch != '.' && ch != '_' && ch != '-'
        });
        let mut parts = token.split('@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        let no_extra = parts.next().is_none();
        no_extra
            && !local.is_empty()
            && domain.contains('.')
            && domain
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    })
}

fn is_confirmation_goal_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "ok" | "okay"
            | "yes"
            | "y"
            | "confirm"
            | "confirmed"
            | "approve"
            | "approved"
            | "go"
            | "go ahead"
            | "proceed"
            | "do it"
            | "ship it"
            | "run it"
            | "apply"
    )
}

fn catalog_servers() -> Vec<CatalogServer> {
    let mut out = Vec::<CatalogServer>::new();
    let Some(index) = mcp_catalog::index() else {
        return out;
    };
    let rows = index
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let slug = row.get("slug").and_then(Value::as_str).unwrap_or("").trim();
        if slug.is_empty() {
            continue;
        }
        let transport = row
            .get("transport_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let tool_names = row
            .get("tool_names")
            .and_then(Value::as_array)
            .map(|vals| {
                vals.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(CatalogServer {
            slug: slug.to_string(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(slug)
                .to_string(),
            description: row
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            documentation_url: row
                .get("documentation_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            transport_url: transport,
            requires_auth: row
                .get("requires_auth")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            requires_setup: row
                .get("requires_setup")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            tool_names,
        });
    }
    out
}

fn score_candidates_for_need(
    catalog: &[CatalogServer],
    need: &CapabilityNeed,
) -> Vec<ConnectorCandidate> {
    let mut out = Vec::<ConnectorCandidate>::new();
    for server in catalog {
        let mut score = 0usize;
        let hay = format!(
            "{} {} {} {}",
            server.slug,
            server.name.to_ascii_lowercase(),
            server.description.to_ascii_lowercase(),
            server.tool_names.join(" ").to_ascii_lowercase()
        );
        for term in &need.query_terms {
            if hay.contains(&term.to_ascii_lowercase()) {
                score += 3;
            }
        }
        if need.id.contains("news") && hay.contains("news") {
            score += 4;
        }
        if score == 0 {
            continue;
        }
        out.push(ConnectorCandidate {
            slug: server.slug.clone(),
            name: server.name.clone(),
            description: server.description.clone(),
            documentation_url: server.documentation_url.clone(),
            transport_url: server.transport_url.clone(),
            requires_auth: server.requires_auth,
            requires_setup: server.requires_setup,
            tool_count: server.tool_names.len(),
            score,
        });
    }
    out
}

fn should_auto_select_connector(need: &CapabilityNeed, candidate: &ConnectorCandidate) -> bool {
    match need.id.as_str() {
        "email.send" => {
            if candidate.score < 6 {
                return false;
            }
            let hay = format!(
                "{} {} {}",
                candidate.slug.to_ascii_lowercase(),
                candidate.name.to_ascii_lowercase(),
                candidate.description.to_ascii_lowercase()
            );
            let looks_like_marketing = ["crm", "campaign", "marketing", "sales"]
                .iter()
                .any(|term| hay.contains(term));
            let looks_like_mail_delivery = [
                "email",
                "mail",
                "gmail",
                "smtp",
                "sendgrid",
                "mailgun",
                "outlook",
                "office365",
            ]
            .iter()
            .any(|term| hay.contains(term));
            if looks_like_marketing && !looks_like_mail_delivery {
                return false;
            }
            true
        }
        _ => true,
    }
}

async fn available_builtin_tools(state: &AppState) -> BTreeSet<String> {
    state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .filter(|name| !name.starts_with("mcp."))
        .collect()
}

fn need_satisfied_by_builtin(builtin_tools: &BTreeSet<String>, need: &CapabilityNeed) -> bool {
    let has = |name: &str| builtin_tools.contains(name);
    match need.id.as_str() {
        "news.latest" | "web.research" => has("websearch") && has("webfetch"),
        "question.ask" => has("question"),
        _ => false,
    }
}

fn derive_required_secret_refs_for_selected(
    catalog: &[CatalogServer],
    selected_connectors: &[String],
) -> Vec<String> {
    let mut refs = BTreeSet::<String>::new();
    for slug in selected_connectors {
        if let Some(connector) = catalog.iter().find(|row| &row.slug == slug) {
            if !connector.requires_auth {
                continue;
            }
            refs.insert(format!(
                "{}_TOKEN",
                connector.slug.to_ascii_uppercase().replace('-', "_")
            ));
        }
    }
    refs.into_iter().collect()
}

fn goal_to_slug(goal: &str) -> String {
    let mut out = String::new();
    for ch in goal.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 42 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "automation".to_string()
    } else {
        trimmed.to_string()
    }
}

fn namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn sync_mcp_tools_for_server(state: &AppState, name: &str) -> usize {
    let prefix = format!("mcp.{}.", namespace_segment(name));
    state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.server_tools(name).await;
    for tool in &tools {
        let schema = ToolSchema::new(
            tool.namespaced_name.clone(),
            if tool.description.trim().is_empty() {
                format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
            } else {
                tool.description.clone()
            },
            tool.input_schema.clone(),
        );
        state
            .tools
            .register_tool(
                schema.name.clone(),
                Arc::new(McpBridgeTool {
                    schema,
                    mcp: state.mcp.clone(),
                    server_name: tool.server_name.clone(),
                    tool_name: tool.tool_name.clone(),
                }),
            )
            .await;
    }
    tools.len()
}

fn save_pack_preset(plan: &PreparedPlan, registered_servers: &[String]) -> anyhow::Result<PathBuf> {
    let paths = tandem_core::resolve_shared_paths().context("resolve shared paths")?;
    let dir = paths
        .canonical_root
        .join("presets")
        .join("overrides")
        .join("pack_presets");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.yaml", plan.pack_id));

    let mut content = String::new();
    content.push_str(&format!("id: {}\n", plan.pack_id));
    content.push_str(&format!("version: {}\n", plan.version));
    content.push_str("kind: pack_preset\n");
    content.push_str("pack:\n");
    content.push_str(&format!("  pack_id: {}\n", plan.pack_id));
    content.push_str(&format!("  name: {}\n", plan.pack_name));
    content.push_str(&format!(
        "  goal: |\n    {}\n",
        plan.goal.replace('\n', "\n    ")
    ));
    content.push_str("connectors:\n");
    for row in &plan.recommended_connectors {
        let selected = registered_servers.iter().any(|v| v == &row.slug);
        content.push_str(&format!("  - slug: {}\n", row.slug));
        content.push_str(&format!("    name: {}\n", row.name));
        content.push_str(&format!(
            "    documentation_url: {}\n",
            row.documentation_url
        ));
        content.push_str(&format!("    transport_url: {}\n", row.transport_url));
        content.push_str(&format!("    requires_auth: {}\n", row.requires_auth));
        content.push_str(&format!("    selected: {}\n", selected));
    }
    content.push_str("registered_servers:\n");
    for srv in registered_servers {
        content.push_str(&format!("  - {}\n", srv));
    }
    content.push_str("required_credentials:\n");
    for sec in &plan.required_secrets {
        content.push_str(&format!("  - {}\n", sec));
    }
    content.push_str("selected_mcp_tools:\n");
    for tool in &plan.selected_mcp_tools {
        content.push_str(&format!("  - {}\n", tool));
    }

    fs::write(&path, content)?;
    Ok(path)
}

fn zip_dir(src_dir: &PathBuf, output_zip: &PathBuf) -> anyhow::Result<()> {
    let file =
        File::create(output_zip).with_context(|| format!("create {}", output_zip.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut stack = vec![src_dir.clone()];
    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)?
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(src_dir)
                .context("strip prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                if !rel.is_empty() {
                    zip.add_directory(format!("{}/", rel), opts)?;
                }
                stack.push(path);
                continue;
            }
            zip.start_file(rel, opts)?;
            let bytes = fs::read(&path)?;
            zip.write_all(&bytes)?;
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_send_does_not_auto_select_low_confidence_connector() {
        let need = CapabilityNeed {
            id: "email.send".to_string(),
            external: true,
            query_terms: vec!["email".to_string()],
        };
        let candidate = ConnectorCandidate {
            slug: "activecampaign".to_string(),
            name: "ActiveCampaign".to_string(),
            description: "Marketing automation and CRM workflows".to_string(),
            documentation_url: String::new(),
            transport_url: String::new(),
            requires_auth: true,
            requires_setup: false,
            tool_count: 5,
            score: 3,
        };
        assert!(!should_auto_select_connector(&need, &candidate));
    }

    #[test]
    fn email_send_allows_high_confidence_mail_connector() {
        let need = CapabilityNeed {
            id: "email.send".to_string(),
            external: true,
            query_terms: vec!["email".to_string()],
        };
        let candidate = ConnectorCandidate {
            slug: "gmail".to_string(),
            name: "Gmail".to_string(),
            description: "Send and manage email messages".to_string(),
            documentation_url: String::new(),
            transport_url: String::new(),
            requires_auth: true,
            requires_setup: false,
            tool_count: 8,
            score: 9,
        };
        assert!(should_auto_select_connector(&need, &candidate));
    }

    #[test]
    fn build_pack_builder_automation_mirrors_routine_template() {
        let plan = PreparedPlan {
            plan_id: "plan-pack-builder-test".to_string(),
            goal: "Create a daily digest pack".to_string(),
            pack_id: "daily_digest_pack".to_string(),
            pack_name: "Daily Digest Pack".to_string(),
            version: "0.1.0".to_string(),
            capabilities_required: vec!["web.search".to_string()],
            capabilities_optional: Vec::new(),
            recommended_connectors: Vec::new(),
            selected_connector_slugs: Vec::new(),
            selected_mcp_tools: Vec::new(),
            fallback_warnings: Vec::new(),
            required_secrets: Vec::new(),
            generated_zip_path: PathBuf::from("/tmp/daily-digest-pack.zip"),
            routine_ids: vec!["routine.daily_digest_pack".to_string()],
            routine_template: RoutineTemplate {
                routine_id: "routine.daily_digest_pack".to_string(),
                name: "Daily Digest Pack".to_string(),
                timezone: "UTC".to_string(),
                schedule: RoutineSchedule::Cron {
                    expression: "0 8 * * *".to_string(),
                },
                entrypoint: "packs/daily_digest_pack/run".to_string(),
                allowed_tools: vec!["web_search".to_string(), "write_file".to_string()],
            },
            created_at_ms: 0,
        };

        let automation = build_pack_builder_automation(
            &plan,
            "routine.daily_digest_pack",
            "team",
            6,
            &["slack".to_string(), "github".to_string()],
            true,
        );

        assert_eq!(
            automation.automation_id,
            "automation.routine.daily_digest_pack"
        );
        assert_eq!(automation.status, crate::AutomationV2Status::Paused);
        assert_eq!(
            automation.schedule.schedule_type,
            crate::AutomationV2ScheduleType::Cron
        );
        assert_eq!(
            automation.schedule.cron_expression.as_deref(),
            Some("0 8 * * *")
        );
        assert_eq!(automation.agents.len(), 1);
        assert_eq!(automation.flow.nodes.len(), 1);
        assert_eq!(automation.flow.nodes[0].node_id, "pack_builder_execute");
        assert_eq!(
            automation.flow.nodes[0]
                .output_contract
                .as_ref()
                .map(|contract| contract.validator.clone()),
            Some(Some(crate::AutomationOutputValidatorKind::GenericArtifact))
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("origin"))
                .and_then(|v| v.as_str()),
            Some("pack_builder")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("activation_mode"))
                .and_then(|v| v.as_str()),
            Some("routine_wrapper_mirror")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("pack_builder_plan_id"))
                .and_then(|v| v.as_str()),
            Some("plan-pack-builder-test")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_id"))
                .and_then(|v| v.as_str()),
            Some("routine.daily_digest_pack")
        );
    }
}
