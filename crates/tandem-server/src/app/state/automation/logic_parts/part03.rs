pub(crate) fn normalize_automation_path_text(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    let is_absolute = path.is_absolute();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !is_absolute {
                    normalized.push("..");
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    let normalized = normalized.to_string_lossy().trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn automation_run_artifact_root(run_id: &str) -> Option<String> {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!(".tandem/runs/{trimmed}/artifacts"))
    }
}

pub(crate) fn automation_run_scoped_output_path(run_id: &str, output_path: &str) -> Option<String> {
    let normalized = normalize_automation_path_text(output_path)?.replace('\\', "/");
    let prefix = ".tandem/artifacts/";
    if let Some(suffix) = normalized.strip_prefix(prefix) {
        let root = automation_run_artifact_root(run_id)?;
        return Some(if suffix.is_empty() {
            root
        } else {
            format!("{root}/{suffix}")
        });
    }
    Some(normalized)
}

pub(crate) fn automation_run_scoped_absolute_output_path(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> Option<String> {
    let candidate = PathBuf::from(normalize_automation_path_text(output_path)?);
    if !candidate.is_absolute() {
        return None;
    }
    let workspace = PathBuf::from(normalize_automation_path_text(workspace_root)?);
    let relative = candidate.strip_prefix(&workspace).ok()?;
    let relative_text =
        normalize_automation_path_text(relative.to_string_lossy().as_ref())?.replace('\\', "/");
    if relative_text == ".tandem/artifacts" {
        return automation_run_artifact_root(run_id);
    }
    let suffix = relative_text.strip_prefix(".tandem/artifacts/")?;
    let root = automation_run_artifact_root(run_id)?;
    Some(if suffix.is_empty() {
        root
    } else {
        format!("{root}/{suffix}")
    })
}

pub(crate) fn resolve_automation_output_path_for_run(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let scoped_output_path =
        automation_run_scoped_absolute_output_path(workspace_root, run_id, output_path)
            .or_else(|| automation_run_scoped_output_path(run_id, output_path))
            .unwrap_or_else(|| output_path.trim().to_string());
    resolve_automation_output_path(workspace_root, &scoped_output_path)
}

pub(crate) fn automation_node_output_sibling_extensions(
    node: &AutomationFlowNode,
) -> &'static [&'static str] {
    let kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim())
        .unwrap_or("structured_json");
    if kind.eq_ignore_ascii_case("report_markdown") {
        &["html", "htm", "md", "markdown", "txt"]
    } else {
        &[]
    }
}

pub(crate) fn automation_output_path_candidates(
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let mut candidates = vec![resolved.clone()];
    let sibling_extensions = automation_node_output_sibling_extensions(node);
    if sibling_extensions.is_empty() {
        return Ok(candidates);
    }

    let Some(parent) = resolved.parent() else {
        return Ok(candidates);
    };
    let Some(stem) = resolved.file_stem().and_then(|value| value.to_str()) else {
        return Ok(candidates);
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Ok(candidates);
    };
    let mut siblings = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && *path != resolved)
        .filter(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|candidate_stem| candidate_stem == stem)
        })
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    sibling_extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
        })
        .collect::<Vec<_>>();
    siblings.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    siblings.dedup();
    candidates.extend(siblings);
    candidates.dedup();
    Ok(candidates)
}

pub(crate) fn session_write_paths_for_output_candidates(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    candidate_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let candidate_paths = candidate_paths.iter().cloned().collect::<HashSet<_>>();
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) =
                resolve_automation_output_path_for_run(workspace_root, run_id, path)
            else {
                continue;
            };
            if !candidate_paths.contains(&candidate_path) {
                continue;
            }
            if !paths.iter().any(|existing| existing == &candidate_path) {
                paths.push(candidate_path);
            }
        }
    }
    paths
}

pub(crate) fn automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let candidates = automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
    let session_written_candidates =
        session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
    Ok(session_written_candidates
        .into_iter()
        .chain(candidates.into_iter())
        .find(|candidate| candidate.exists() && candidate.is_file()))
}

pub(crate) async fn reconcile_automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
    max_wait_ms: u64,
    poll_interval_ms: u64,
) -> anyhow::Result<Option<AutomationVerifiedOutputResolution>> {
    let output_touched = session_write_touched_output_for_output(
        session,
        workspace_root,
        output_path,
        Some(run_id),
        None,
    );
    let poll_interval_ms = poll_interval_ms.max(1);
    let start_ms = now_ms() as u64;

    loop {
        let candidates =
            automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
        let session_written_candidates =
            session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
        if let Some(resolved) = automation_resolve_verified_output_path(
            session,
            workspace_root,
            run_id,
            node,
            output_path,
        )? {
            let materialized_by_current_attempt = session_written_candidates
                .iter()
                .any(|candidate| candidate == &resolved);
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: resolved,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt,
                resolution_kind: AutomationVerifiedOutputResolutionKind::Direct,
            }));
        }
        if let Some(promoted) = maybe_promote_legacy_workspace_artifact_for_run(
            session,
            workspace_root,
            run_id,
            output_path,
        )? {
            return Ok(Some(AutomationVerifiedOutputResolution {
                materialized_by_current_attempt: output_touched
                    || promoted.materialized_by_current_attempt,
                ..promoted
            }));
        }
        if let Some(recovered) =
            recover_required_output_from_session_text(session, workspace_root, run_id, output_path)?
        {
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: recovered,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt: true,
                resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
            }));
        }
        if !output_touched {
            return Ok(None);
        }
        let elapsed_ms = now_ms() as u64 - start_ms;
        if elapsed_ms >= max_wait_ms {
            return Ok(None);
        }
        let sleep_ms = poll_interval_ms.min(max_wait_ms.saturating_sub(elapsed_ms));
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

pub(crate) fn recover_required_output_from_session_text(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let Some(extension) = resolved.extension().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    if !extension.eq_ignore_ascii_case("json") {
        return Ok(None);
    }
    let payload = extract_recoverable_json_from_session(session);
    let Some(payload) = payload else {
        return Ok(None);
    };
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&resolved, serialized)?;
    Ok(Some(resolved))
}

pub(crate) fn is_suspicious_automation_marker_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with(".tandem")
        || lowered == "_automation_touch.txt"
        || lowered.contains("stage-touch")
        || lowered.ends_with("-status.txt")
        || lowered.contains("touch.txt")
}

pub(crate) fn list_suspicious_automation_marker_files(workspace_root: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_suspicious_automation_marker_file(path))
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn remove_suspicious_automation_marker_files(workspace_root: &str) {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_suspicious_automation_marker_file(&path) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

pub(crate) fn should_downgrade_auto_cleaned_marker_rejection(
    rejected_reason: Option<&str>,
    auto_cleaned: bool,
    semantic_block_reason: Option<&str>,
    accepted_output_present: bool,
) -> bool {
    auto_cleaned
        && semantic_block_reason.is_none()
        && accepted_output_present
        && rejected_reason
            .is_some_and(|reason| reason.starts_with("undeclared marker files created:"))
}

pub(crate) fn automation_workspace_root_file_snapshot(
    workspace_root: &str,
) -> std::collections::BTreeSet<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut snapshot = std::collections::BTreeSet::new();
    let mut stack = vec![workspace.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let display = path
                .strip_prefix(&workspace)
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            snapshot.insert(display);
        }
    }
    snapshot
}

pub(crate) fn resolve_case_insensitive_workspace_relative_path(
    workspace_root: &str,
    request: &str,
) -> Option<PathBuf> {
    let workspace_root_path = PathBuf::from(workspace_root);
    let trimmed = request.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    let direct = workspace_root_path.join(trimmed);
    if direct.exists() {
        return Some(direct);
    }
    let requested_parts = trimmed
        .split(std::path::MAIN_SEPARATOR)
        .filter(|segment| !segment.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if requested_parts.is_empty() {
        return None;
    }
    let mut stack = vec![workspace_root_path.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(stripped) = path.strip_prefix(&workspace_root_path) else {
                continue;
            };
            let candidate_parts = stripped
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if candidate_parts.len() < requested_parts.len() {
                continue;
            }
            let candidate_suffix =
                &candidate_parts[candidate_parts.len() - requested_parts.len()..];
            if candidate_suffix == requested_parts.as_slice() {
                return Some(path);
            }
        }
    }
    None
}

pub(crate) fn automation_read_only_file_snapshot_for_node(
    workspace_root: &str,
    read_only_paths: &[String],
) -> std::collections::BTreeMap<String, Vec<u8>> {
    let workspace_root_path = PathBuf::from(workspace_root);
    let mut snapshot = std::collections::BTreeMap::<String, Vec<u8>>::new();
    for path in read_only_paths {
        let Some(normalized) = resolve_automation_output_path(workspace_root, path)
            .ok()
            .and_then(|value| {
                value
                    .strip_prefix(&workspace_root_path)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty())
            })
        else {
            continue;
        };
        let Some(resolved) =
            resolve_case_insensitive_workspace_relative_path(workspace_root, &normalized)
        else {
            continue;
        };
        let Some(resolved_relative) = resolved
            .strip_prefix(&workspace_root_path)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if let Ok(content) = std::fs::read(&resolved) {
            snapshot.insert(resolved_relative, content);
        }
    }
    snapshot
}

pub(crate) fn revert_read_only_source_snapshot_files(
    workspace_root: &str,
    snapshot: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Vec<Value> {
    let workspace_root_path = PathBuf::from(workspace_root);
    let mut restored_events = Vec::new();
    for (path, before) in snapshot {
        let resolved = workspace_root_path.join(path);
        let was_missing = !resolved.exists();
        if let Some(parent) = resolved.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                restored_events.push(json!({
                    "path": path,
                    "issue": "restore_dir_failed",
                    "reason": format!("{error}")
                }));
                continue;
            }
        }
        match std::fs::write(&resolved, before) {
            Ok(()) => restored_events.push(json!({
                "path": path,
                "issue": if was_missing { "restored_missing" } else { "restored_modified" },
            })),
            Err(error) => restored_events.push(json!({
                "path": path,
                "issue": "restore_failed",
                "reason": format!("{error}"),
            })),
        }
    }
    restored_events
}

pub(crate) struct ReadOnlySourceSnapshotRollback<'a> {
    workspace_root: String,
    snapshot: &'a std::collections::BTreeMap<String, Vec<u8>>,
    active: bool,
}

impl<'a> ReadOnlySourceSnapshotRollback<'a> {
    pub(crate) fn armed(
        workspace_root: &str,
        snapshot: &'a std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.to_string(),
            snapshot,
            active: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.active = false;
    }
}

impl<'a> Drop for ReadOnlySourceSnapshotRollback<'a> {
    fn drop(&mut self) {
        if self.active {
            let _ = revert_read_only_source_snapshot_files(&self.workspace_root, self.snapshot);
            self.active = false;
        }
    }
}

pub(crate) fn placeholder_like_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    // TODO(coding-hardening): Replace this phrase-based placeholder detection with
    // structural artifact validation. The long-term design should score artifact
    // substance from session mutation history + contract-kind-specific structure
    // (sections, length, citations, required headings) rather than hard-coded text
    // markers that are brittle across providers, prompts, and languages.
    if trimmed.len() <= 160 {
        let compact = trimmed.to_ascii_lowercase();
        let status_only_markers = [
            "completed",
            "written to",
            "already written",
            "no content change",
            "no content changes",
            "confirmed",
            "preserving existing artifact",
            "finalization",
            "write completion",
        ];
        if status_only_markers
            .iter()
            .any(|marker| compact.contains(marker))
            && !compact.contains("## ")
            && !compact.contains("\n## ")
            && !compact.contains("files reviewed")
            && !compact.contains("proof points")
        {
            return true;
        }
    }
    let lowered = trimmed
        .chars()
        .take(800)
        .collect::<String>()
        .to_ascii_lowercase();
    let strong_markers = [
        "completed previously in this run",
        "preserving file creation requirement",
        "preserving current workspace output state",
        "created/updated to satisfy workflow artifact requirement",
        "see existing workspace research already completed in this run",
        "already written in prior step",
        "no content changes needed",
        "placeholder preservation note",
        "touch file",
        "status note",
        "marker file",
    ];
    if strong_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }
    let status_markers = [
        "# status",
        "## status",
        "status: blocked",
        "status: completed",
        "status: pending",
        "blocked handoff",
        "blocked note",
        "not approved yet",
        "pending approval",
    ];
    status_markers.iter().any(|marker| lowered.contains(marker)) && trimmed.len() < 280
}

pub(crate) fn html_tag_count(text: &str, tag: &str) -> usize {
    let needle = format!("<{tag}");
    text.match_indices(&needle)
        .filter(|(index, _)| {
            let tail = &text[index + needle.len()..];
            tail.chars()
                .next()
                .is_none_or(|next| !next.is_ascii_alphabetic())
        })
        .count()
}

pub(crate) fn markdown_heading_count(text: &str) -> usize {
    let markdown = text
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count();
    let html = (1..=6)
        .map(|level| html_tag_count(text, &format!("h{level}")))
        .sum::<usize>();
    markdown + html
}

pub(crate) fn markdown_list_item_count(text: &str) -> usize {
    let markdown = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() && trimmed.contains('.'))
        })
        .count();
    markdown + html_tag_count(text, "li")
}

pub(crate) fn paragraph_block_count(text: &str) -> usize {
    let markdown = text
        .split("\n\n")
        .filter(|block| {
            let trimmed = block.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count();
    markdown + html_tag_count(text, "p")
}

pub(crate) fn structural_substantive_artifact_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.len() < 180 {
        return false;
    }
    let heading_count = markdown_heading_count(trimmed);
    let list_count = markdown_list_item_count(trimmed);
    let paragraph_count = paragraph_block_count(trimmed);
    heading_count >= 2
        || (heading_count >= 1 && paragraph_count >= 3)
        || (paragraph_count >= 4)
        || (list_count >= 5)
}

pub(crate) fn substantive_artifact_text(text: &str) -> bool {
    structural_substantive_artifact_text(text)
}

pub(crate) fn artifact_required_section_count(node: &AutomationFlowNode, text: &str) -> usize {
    let lowered = text.to_ascii_lowercase();
    let headings = if automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ResearchBrief
    {
        vec![
            "workspace source audit",
            "campaign goal",
            "target audience",
            "core pain points",
            "positioning angle",
            "competitor context",
            "proof points",
            "likely objections",
            "channel considerations",
            "recommended message hierarchy",
            "files reviewed",
            "files not reviewed",
            "web sources reviewed",
        ]
    } else {
        vec![
            "files reviewed",
            "review notes",
            "approved",
            "draft",
            "summary",
        ]
    };
    headings
        .iter()
        .filter(|heading| lowered.contains(**heading))
        .count()
}

pub(crate) fn normalize_workspace_display_path(
    workspace_root: &str,
    raw_path: &str,
) -> Option<String> {
    let trimmed = raw_path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return None;
    }
    resolve_automation_output_path(workspace_root, trimmed)
        .ok()
        .and_then(|resolved| {
            resolved
                .strip_prefix(PathBuf::from(workspace_root))
                .ok()
                .and_then(|value| value.to_str().map(str::to_string))
        })
        .filter(|value| !value.is_empty())
}

pub(crate) fn tool_args_object(
    args: &Value,
) -> Option<std::borrow::Cow<'_, serde_json::Map<String, Value>>> {
    match args {
        Value::Object(map) => Some(std::borrow::Cow::Borrowed(map)),
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|value| match value {
                    Value::Object(map) => Some(std::borrow::Cow::Owned(map)),
                    _ => None,
                })
        }
        _ => None,
    }
}

pub(crate) fn session_read_paths(session: &Session, workspace_root: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("read")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str) else {
                continue;
            };
            if let Some(normalized) = normalize_workspace_display_path(workspace_root, path) {
                paths.push(normalized);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutomationUpstreamEvidence {
    pub(crate) read_paths: Vec<String>,
    pub(crate) discovered_relevant_paths: Vec<String>,
    pub(crate) web_research_attempted: bool,
    pub(crate) web_research_succeeded: bool,
    pub(crate) citation_count: usize,
    pub(crate) citations: Vec<String>,
}

async fn collect_automation_upstream_research_evidence(
    state: &AppState,
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> AutomationUpstreamEvidence {
    let mut evidence = AutomationUpstreamEvidence::default();
    let mut upstream_node_ids = node
        .input_refs
        .iter()
        .map(|input| input.from_step_id.clone())
        .collect::<Vec<_>>();
    upstream_node_ids.extend(node.depends_on.clone());
    upstream_node_ids.sort();
    upstream_node_ids.dedup();
    let flow_nodes = automation
        .flow
        .nodes
        .iter()
        .map(|entry| (entry.node_id.as_str(), entry))
        .collect::<std::collections::HashMap<_, _>>();
    for upstream_node_id in upstream_node_ids {
        let Some(output) = run.checkpoint.node_outputs.get(&upstream_node_id) else {
            continue;
        };
        if let Some(validation) = output.get("artifact_validation") {
            if let Some(rows) = validation.get("read_paths").and_then(Value::as_array) {
                evidence
                    .read_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            if let Some(rows) = validation
                .get("discovered_relevant_paths")
                .and_then(Value::as_array)
            {
                evidence
                    .discovered_relevant_paths
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
            evidence.web_research_attempted |= validation
                .get("web_research_attempted")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= validation
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(count) = validation.get("citation_count").and_then(Value::as_u64) {
                evidence.citation_count += count as usize;
            }
            if let Some(rows) = validation.get("citations").and_then(Value::as_array) {
                evidence
                    .citations
                    .extend(rows.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
        if let Some(tool_telemetry) = output.get("tool_telemetry") {
            evidence.web_research_attempted |= tool_telemetry
                .get("web_research_used")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            evidence.web_research_succeeded |= tool_telemetry
                .get("web_research_succeeded")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        if let Some(session_id) = automation_output_session_id(output) {
            if let Some(session) = state.storage.get_session(&session_id).await {
                evidence
                    .read_paths
                    .extend(session_read_paths(&session, workspace_root));
                evidence
                    .discovered_relevant_paths
                    .extend(session_discovered_relevant_paths(&session, workspace_root));
                if let Some(upstream_node) = flow_nodes.get(upstream_node_id.as_str()) {
                    let requested_tools = output
                        .get("tool_telemetry")
                        .and_then(|value| value.get("requested_tools"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let telemetry = summarize_automation_tool_activity(
                        upstream_node,
                        &session,
                        &requested_tools,
                    );
                    evidence.web_research_attempted |= telemetry
                        .get("web_research_used")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    evidence.web_research_succeeded |= telemetry
                        .get("web_research_succeeded")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                }
            }
        }
    }
    evidence.read_paths.sort();
    evidence.read_paths.dedup();
    evidence.discovered_relevant_paths.sort();
    evidence.discovered_relevant_paths.dedup();
    evidence.citations.sort();
    evidence.citations.dedup();
    evidence
}

pub(crate) fn session_discovered_relevant_paths(
    session: &Session,
    workspace_root: &str,
) -> Vec<String> {
    let workspace = PathBuf::from(workspace_root);
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool,
                result,
                error,
                ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("glob")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(output) = automation_tool_result_output_text(result.as_ref()) else {
                continue;
            };
            for line in output.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let path = PathBuf::from(trimmed);
                let resolved = if path.is_absolute() {
                    path
                } else {
                    let Ok(resolved) = resolve_automation_output_path(workspace_root, trimmed)
                    else {
                        continue;
                    };
                    resolved
                };
                if !resolved.starts_with(&workspace) {
                    continue;
                }
                if !std::fs::metadata(&resolved)
                    .map(|metadata| metadata.is_file())
                    .unwrap_or(false)
                {
                    continue;
                }
                let display = resolved
                    .strip_prefix(&workspace)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty());
                if let Some(display) = display {
                    paths.push(display);
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn session_write_candidates_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Vec<String> {
    let target_path = automation_session_write_target_path(
        workspace_root,
        declared_output_path,
        run_id,
        runtime_values,
    );
    let Some(target_path) = target_path else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = automation_write_arg_path(&args) else {
                continue;
            };
            let Ok(candidate_path) = (if let Some(run_id) = run_id {
                resolve_automation_output_path_with_runtime_for_run(
                    workspace_root,
                    run_id,
                    path,
                    runtime_values,
                )
            } else {
                resolve_automation_output_path(
                    workspace_root,
                    &automation_runtime_placeholder_replace(path, runtime_values),
                )
            }) else {
                continue;
            };
            if candidate_path != target_path {
                continue;
            }
            let Some(content) = automation_write_arg_content(&args) else {
                continue;
            };
            if !content.trim().is_empty() {
                candidates.push(content.to_string());
            }
        }
    }
    candidates
}

pub(crate) fn automation_session_write_target_path(
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Option<PathBuf> {
    run_id
        .and_then(|run_id| {
            resolve_automation_output_path_with_runtime_for_run(
                workspace_root,
                run_id,
                declared_output_path,
                runtime_values,
            )
            .ok()
        })
        .or_else(|| {
            resolve_automation_output_path(
                workspace_root,
                &automation_runtime_placeholder_replace(declared_output_path, runtime_values),
            )
            .ok()
        })
}

pub(crate) fn session_write_touched_output_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> bool {
    let target_path = automation_session_write_target_path(
        workspace_root,
        declared_output_path,
        run_id,
        runtime_values,
    );
    let Some(target_path) = target_path else {
        return false;
    };
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = automation_write_arg_path(&args) else {
                continue;
            };
            let Ok(candidate_path) = (if let Some(run_id) = run_id {
                resolve_automation_output_path_with_runtime_for_run(
                    workspace_root,
                    run_id,
                    path,
                    runtime_values,
                )
            } else {
                resolve_automation_output_path(
                    workspace_root,
                    &automation_runtime_placeholder_replace(path, runtime_values),
                )
            }) else {
                continue;
            };
            if candidate_path == target_path {
                return true;
            }
        }
    }
    false
}

pub(crate) fn session_write_materialized_output_for_output(
    session: &Session,
    workspace_root: &str,
    declared_output_path: &str,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> bool {
    let target_path = automation_session_write_target_path(
        workspace_root,
        declared_output_path,
        run_id,
        runtime_values,
    );
    let Some(target_path) = target_path else {
        return false;
    };
    if !session_write_touched_output_for_output(
        session,
        workspace_root,
        declared_output_path,
        run_id,
        runtime_values,
    ) {
        return false;
    }
    std::fs::metadata(&target_path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

pub(crate) fn automation_verified_output_differs_from_preexisting(
    preexisting_output: Option<&str>,
    verified_output: &(String, String),
) -> bool {
    preexisting_output.is_none_or(|previous| previous != verified_output.1)
}

pub(crate) fn automation_repair_output_differs_from_preexisting(
    preexisting_output: Option<&str>,
    accepted_output: Option<&(String, String)>,
) -> bool {
    accepted_output.is_some_and(|output| {
        automation_verified_output_differs_from_preexisting(preexisting_output, output)
    })
}

pub(crate) fn automation_write_arg_path(args: &serde_json::Map<String, Value>) -> Option<&str> {
    args.get("path")
        .or_else(|| args.get("filePath"))
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("output_path"))
        .or_else(|| args.get("target_path"))
        .or_else(|| args.get("file"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_write_arg_content(args: &serde_json::Map<String, Value>) -> Option<&str> {
    args.get("content")
        .or_else(|| args.get("contents"))
        .or_else(|| args.get("text"))
        .or_else(|| args.get("body"))
        .or_else(|| args.get("value"))
        .or_else(|| args.get("data"))
        .and_then(Value::as_str)
}

pub(crate) fn session_file_mutation_summary(session: &Session, workspace_root: &str) -> Value {
    let mut touched_files = Vec::<String>::new();
    let mut mutation_tool_by_file = serde_json::Map::new();
    let workspace_root_path = PathBuf::from(workspace_root);
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if error.as_ref().is_some_and(|value| !value.trim().is_empty()) {
                continue;
            }
            let tool_name = tool.trim().to_ascii_lowercase().replace('-', "_");
            let parsed_args = tool_args_object(args);
            let candidate_paths = if tool_name == "apply_patch" {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("patchText"))
                    .and_then(Value::as_str)
                    .map(|patch| {
                        patch
                            .lines()
                            .filter_map(|line| {
                                let trimmed = line.trim();
                                trimmed
                                    .strip_prefix("*** Add File: ")
                                    .or_else(|| trimmed.strip_prefix("*** Update File: "))
                                    .or_else(|| trimmed.strip_prefix("*** Delete File: "))
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                parsed_args
                    .as_ref()
                    .and_then(|args| args.get("path"))
                    .and_then(Value::as_str)
                    .map(|value| vec![value.trim().to_string()])
                    .unwrap_or_default()
            };
            for candidate in candidate_paths {
                let Some(resolved) = resolve_automation_output_path(workspace_root, &candidate)
                    .ok()
                    .or_else(|| {
                        let path = PathBuf::from(candidate.trim());
                        if path.is_absolute()
                            && tandem_core::is_within_workspace_root(&path, &workspace_root_path)
                        {
                            Some(path)
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };
                let display = resolved
                    .strip_prefix(&workspace_root_path)
                    .ok()
                    .and_then(|value| value.to_str().map(str::to_string))
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| resolved.to_string_lossy().to_string());
                if !touched_files.iter().any(|existing| existing == &display) {
                    touched_files.push(display.clone());
                }
                match mutation_tool_by_file.get_mut(&display) {
                    Some(Value::Array(values)) => {
                        if !values
                            .iter()
                            .any(|value| value.as_str() == Some(tool_name.as_str()))
                        {
                            values.push(json!(tool_name.clone()));
                        }
                    }
                    _ => {
                        mutation_tool_by_file.insert(display.clone(), json!([tool_name.clone()]));
                    }
                }
            }
        }
    }
    touched_files.sort();
    json!({
        "touched_files": touched_files,
        "mutation_tool_by_file": mutation_tool_by_file,
    })
}

pub(crate) fn git_diff_summary_for_paths(workspace_root: &str, paths: &[String]) -> Option<Value> {
    if paths.is_empty() || !workspace_has_git_repo(workspace_root) {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace_root)
        .arg("diff")
        .arg("--stat")
        .arg("--");
    for path in paths {
        cmd.arg(path);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        None
    } else {
        Some(json!({
            "stat": summary
        }))
    }
}

#[cfg(test)]
pub(crate) fn validate_automation_artifact_output(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
) -> (Option<(String, String)>, Value, Option<String>) {
    validate_automation_artifact_output_with_upstream(
        node,
        session,
        workspace_root,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        None,
    )
}

pub(crate) fn validate_automation_artifact_output_with_upstream(
    node: &AutomationFlowNode,
    session: &Session,
    workspace_root: &str,
    run_id: Option<&str>,
    session_text: &str,
    tool_telemetry: &Value,
    preexisting_output: Option<&str>,
    verified_output: Option<(String, String)>,
    workspace_snapshot_before: &std::collections::BTreeSet<String>,
    upstream_evidence: Option<&AutomationUpstreamEvidence>,
) -> (Option<(String, String)>, Value, Option<String>) {
    let automation = AutomationV2Spec {
        automation_id: "validation".to_string(),
        name: "validation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "validation".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    validate_automation_artifact_output_with_context(
        &automation,
        node,
        session,
        workspace_root,
        run_id,
        None,
        session_text,
        tool_telemetry,
        preexisting_output,
        verified_output,
        workspace_snapshot_before,
        upstream_evidence,
        None,
    )
}
