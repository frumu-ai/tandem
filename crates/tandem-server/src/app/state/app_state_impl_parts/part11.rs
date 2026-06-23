// Bug Monitor source binding validation helpers split from part01.rs for the
// file-size gate (same module via include!).

fn normalize_bug_monitor_source_binding_values(project: &mut BugMonitorMonitoredProject) {
    normalize_bug_monitor_optional_source_binding_value(&mut project.tenant_id);
    normalize_bug_monitor_optional_source_binding_value(&mut project.workspace_id);
    normalize_bug_monitor_optional_source_binding_value(&mut project.event_schema_version);
    normalize_bug_monitor_optional_source_binding_value(&mut project.redaction_profile);
    normalize_bug_monitor_optional_source_binding_value(&mut project.retention_profile);
    normalize_bug_monitor_source_binding_vec(&mut project.allowed_destination_ids);
    normalize_bug_monitor_source_binding_vec(&mut project.default_destination_ids);
    normalize_bug_monitor_source_binding_vec(&mut project.default_route_tags);

    for source in &mut project.log_sources {
        normalize_bug_monitor_optional_source_binding_value(&mut source.tenant_id);
        normalize_bug_monitor_optional_source_binding_value(&mut source.workspace_id);
        normalize_bug_monitor_optional_source_binding_value(&mut source.event_schema_version);
        normalize_bug_monitor_optional_source_binding_value(&mut source.redaction_profile);
        normalize_bug_monitor_optional_source_binding_value(&mut source.retention_profile);
        normalize_bug_monitor_source_binding_vec(&mut source.allowed_destination_ids);
        normalize_bug_monitor_source_binding_vec(&mut source.default_destination_ids);
        normalize_bug_monitor_source_binding_vec(&mut source.default_route_tags);
    }
}

fn normalize_bug_monitor_optional_source_binding_value(value: &mut Option<String>) {
    *value = value
        .as_ref()
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
}

fn normalize_bug_monitor_source_binding_vec(values: &mut Vec<String>) {
    let mut out = Vec::new();
    for value in std::mem::take(values) {
        let value = value.trim().to_string();
        if value.is_empty() || out.iter().any(|existing| existing == &value) {
            continue;
        }
        out.push(value);
    }
    *values = out;
}

fn validate_bug_monitor_source_binding_destinations(
    project: &BugMonitorMonitoredProject,
    configured_destination_ids: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    validate_bug_monitor_destination_ids(
        &format!(
            "monitored project `{}` allowed_destination_ids",
            project.project_id
        ),
        &project.allowed_destination_ids,
        configured_destination_ids,
    )?;
    validate_bug_monitor_destination_ids(
        &format!(
            "monitored project `{}` default_destination_ids",
            project.project_id
        ),
        &project.default_destination_ids,
        configured_destination_ids,
    )?;
    if !project.allowed_destination_ids.is_empty() {
        for destination_id in &project.default_destination_ids {
            if !project
                .allowed_destination_ids
                .iter()
                .any(|allowed| allowed == destination_id)
            {
                anyhow::bail!(
                    "monitored project `{}` default destination `{destination_id}` is not allowed by allowed_destination_ids",
                    project.project_id
                );
            }
        }
    }

    for source in &project.log_sources {
        validate_bug_monitor_destination_ids(
            &format!(
                "log source `{}` in monitored project `{}` allowed_destination_ids",
                source.source_id, project.project_id
            ),
            &source.allowed_destination_ids,
            configured_destination_ids,
        )?;
        validate_bug_monitor_destination_ids(
            &format!(
                "log source `{}` in monitored project `{}` default_destination_ids",
                source.source_id, project.project_id
            ),
            &source.default_destination_ids,
            configured_destination_ids,
        )?;
        let effective_allowed = effective_bug_monitor_source_allowed_destination_ids(
            &project.allowed_destination_ids,
            &source.allowed_destination_ids,
        );
        let effective_defaults = if source.default_destination_ids.is_empty() {
            project.default_destination_ids.clone()
        } else {
            source.default_destination_ids.clone()
        };
        if !effective_allowed.is_empty() {
            for destination_id in &effective_defaults {
                if !effective_allowed
                    .iter()
                    .any(|allowed| allowed == destination_id)
                {
                    anyhow::bail!(
                        "log source `{}` in monitored project `{}` default destination `{destination_id}` is not allowed by source binding",
                        source.source_id,
                        project.project_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_bug_monitor_destination_ids(
    owner: &str,
    values: &[String],
    configured_destination_ids: &std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    for destination_id in values {
        if !configured_destination_ids.contains(destination_id) {
            anyhow::bail!("{owner} references unknown destination `{destination_id}`");
        }
    }
    Ok(())
}

fn effective_bug_monitor_source_allowed_destination_ids(
    project_allowed: &[String],
    source_allowed: &[String],
) -> Vec<String> {
    if source_allowed.is_empty() {
        return project_allowed.to_vec();
    }
    if project_allowed.is_empty() {
        return source_allowed.to_vec();
    }
    source_allowed
        .iter()
        .filter(|destination_id| project_allowed.iter().any(|allowed| allowed == *destination_id))
        .cloned()
        .collect()
}
