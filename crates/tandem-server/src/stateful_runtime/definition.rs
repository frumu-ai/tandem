use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::automation_v2::types::{AutomationV2RunRecord, AutomationV2Spec};

pub fn automation_definition_snapshot_hash(snapshot: &AutomationV2Spec) -> String {
    stable_definition_snapshot_hash(snapshot)
}

pub fn automation_definition_version(snapshot: &AutomationV2Spec, snapshot_hash: &str) -> String {
    metadata_definition_version(snapshot.metadata.as_ref()).unwrap_or_else(|| {
        format!(
            "automation:{}@{}",
            snapshot.automation_id,
            short_hash(snapshot_hash)
        )
    })
}

pub fn automation_run_definition_metadata(snapshot: &AutomationV2Spec) -> (String, String) {
    let snapshot_hash = automation_definition_snapshot_hash(snapshot);
    let version = automation_definition_version(snapshot, &snapshot_hash);
    (version, snapshot_hash)
}

pub fn automation_run_definition_fields(
    run: &AutomationV2RunRecord,
) -> (Option<String>, Option<String>) {
    let snapshot_metadata = run
        .automation_snapshot
        .as_ref()
        .map(automation_run_definition_metadata);
    let version = run.workflow_definition_version.clone().or_else(|| {
        snapshot_metadata
            .as_ref()
            .map(|(version, _)| version.clone())
    });
    let snapshot_hash = run
        .workflow_definition_snapshot_hash
        .clone()
        .or_else(|| snapshot_metadata.map(|(_, snapshot_hash)| snapshot_hash));
    (version, snapshot_hash)
}

pub fn ensure_automation_run_definition_metadata(run: &mut AutomationV2RunRecord) {
    let Some(snapshot) = run.automation_snapshot.as_ref() else {
        return;
    };
    let (version, snapshot_hash) = automation_run_definition_metadata(snapshot);
    if run.workflow_definition_version.is_none() {
        run.workflow_definition_version = Some(version);
    }
    if run.workflow_definition_snapshot_hash.is_none() {
        run.workflow_definition_snapshot_hash = Some(snapshot_hash);
    }
}

pub fn stamp_automation_run_definition_metadata(run: &mut AutomationV2RunRecord) {
    let Some(snapshot) = run.automation_snapshot.as_ref() else {
        return;
    };
    let (version, snapshot_hash) = automation_run_definition_metadata(snapshot);
    run.workflow_definition_version = Some(version);
    run.workflow_definition_snapshot_hash = Some(snapshot_hash);
}

pub fn automation_run_definition_snapshot_hash_mismatch(
    run: &AutomationV2RunRecord,
) -> Option<(String, String)> {
    let recorded = run.workflow_definition_snapshot_hash.as_ref()?;
    let snapshot = run.automation_snapshot.as_ref()?;
    let actual = automation_definition_snapshot_hash(snapshot);
    (recorded != &actual).then(|| (recorded.clone(), actual))
}

pub fn stable_definition_snapshot_hash<T: Serialize>(snapshot: &T) -> String {
    let canonical = serde_json::to_vec(snapshot).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    format!("sha256:{:x}", hasher.finalize())
}

fn metadata_definition_version(metadata: Option<&Value>) -> Option<String> {
    let metadata = metadata?;
    for key in [
        "definition_version",
        "workflow_definition_version",
        "automation_definition_version",
        "automation_version",
        "source_pack_version",
        "version",
    ] {
        if let Some(value) = metadata.get(key).and_then(value_string) {
            return Some(value);
        }
    }

    plan_revision_version(metadata, &["plan_package_bundle", "plan"])
        .or_else(|| plan_revision_version(metadata, &["approved_plan_materialization"]))
}

fn plan_revision_version(metadata: &Value, path: &[&str]) -> Option<String> {
    let value = value_at_path(metadata, path)?;
    let plan_id = value_string(value.get("plan_id")?)?;
    let plan_revision = value.get("plan_revision").and_then(Value::as_u64)?;
    Some(format!("plan:{plan_id}:rev:{plan_revision}"))
}

fn value_at_path<'a>(mut value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    for segment in path {
        value = value.get(*segment)?;
    }
    Some(value)
}

fn value_string(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn short_hash(snapshot_hash: &str) -> String {
    snapshot_hash
        .strip_prefix("sha256:")
        .unwrap_or(snapshot_hash)
        .chars()
        .take(16)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn metadata_definition_version_prefers_explicit_version() {
        let version = metadata_definition_version(Some(&json!({
            "definition_version": "release-17",
            "plan_package_bundle": {
                "plan": {
                    "plan_id": "plan-a",
                    "plan_revision": 4
                }
            }
        })));

        assert_eq!(version.as_deref(), Some("release-17"));
    }

    #[test]
    fn metadata_definition_version_uses_plan_revision_when_available() {
        let version = metadata_definition_version(Some(&json!({
            "plan_package_bundle": {
                "plan": {
                    "plan_id": "plan-a",
                    "plan_revision": 4
                }
            }
        })));

        assert_eq!(version.as_deref(), Some("plan:plan-a:rev:4"));
    }

    #[test]
    fn stable_definition_snapshot_hash_is_prefixed_and_deterministic() {
        let left = stable_definition_snapshot_hash(&json!({ "a": 1 }));
        let right = stable_definition_snapshot_hash(&json!({ "a": 1 }));
        let changed = stable_definition_snapshot_hash(&json!({ "a": 2 }));

        assert!(left.starts_with("sha256:"));
        assert_eq!(left, right);
        assert_ne!(left, changed);
    }
}
