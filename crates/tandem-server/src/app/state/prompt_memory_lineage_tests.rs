use super::super::prompt_context_hook::{PromptHookBudget, SOURCE_DOCS};
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use tandem_core::PromptContextHookStats;

fn hit(id: &str, malformed: bool) -> GlobalMemorySearchHit {
    let content = "canonical evidence from a governed source with a verified owner".to_string();
    GlobalMemorySearchHit {
        score: 0.9,
        record: tandem_memory::types::GlobalMemoryRecord {
            id: id.into(),
            user_id: "alice".into(),
            source_type: "note".into(),
            content_hash: format!("{:x}", Sha256::digest(content.as_bytes())),
            content,
            run_id: "source-run".into(),
            session_id: None,
            message_id: None,
            tool_name: None,
            project_tag: Some("project".into()),
            channel_tag: None,
            host_tag: None,
            metadata: Some(if malformed {
                json!({"enterprise_source_binding": {"binding_id": "broken"}})
            } else {
                json!({"owner_org_unit_id": "engineering", "owner_subject": "alice"})
            }),
            provenance: Some(
                json!({"tenant_context": {"org_id": "org", "workspace_id": "workspace"}}),
            ),
            redaction_status: "passed".into(),
            redaction_count: 0,
            visibility: "private".into(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: 1,
            updated_at_ms: 1,
            expires_at_ms: None,
        },
    }
}

fn tenant() -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: "org".into(),
        workspace_id: "workspace".into(),
        deployment_id: None,
    }
}

fn budget(chars: usize) -> PromptHookBudget {
    PromptHookBudget {
        stats: PromptContextHookStats {
            budget_chars: Some(chars),
            remaining_chars: Some(chars),
            ..PromptContextHookStats::default()
        },
    }
}

#[test]
fn manifest_contains_only_budget_included_canonical_records() {
    let first = hit("included", false);
    let one = build_memory_block_with_lineage(&[first.clone()], 4_000, &tenant());
    let block = build_memory_block_with_lineage(
        &[first.clone(), hit("dropped", true)],
        one.content.len(),
        &tenant(),
    );
    assert_eq!(block.included_count, 1);
    assert_eq!(block.dropped_count, 1);
    assert!(
        block.lineage_complete,
        "a dropped malformed source did not contribute"
    );
    assert_eq!(
        block.included_memory,
        vec![
            tandem_memory::derived_lineage::CanonicalMemoryRestriction::from_global_record(
                &first.record,
                &tenant()
            )
            .unwrap()
            .source_reference()
        ]
    );
    assert!(!block.content.contains("id=dropped"));
}

#[test]
fn whole_block_deferral_has_no_source_manifest_or_provider_text() {
    let block = build_memory_block_with_lineage(&[hit("deferred", false)], 4_000, &tenant());
    assert_eq!(block.included_memory.len(), 1);
    let mut budget = budget(block.content.len() - 1);
    let mut messages = Vec::new();
    let (injected, sources, complete) = budget.push_memory_context(&mut messages, &block);
    assert!(!injected);
    let result = budget.finish_result(messages, sources, complete);
    assert!(result.messages.is_empty());
    assert!(result.included_memory.is_empty());
    assert!(result.lineage_complete);
    assert_eq!(result.stats.deferred_count(), 1);
}

#[test]
fn malformed_included_restriction_preserves_rendering_but_is_incomplete() {
    let block = build_memory_block_with_lineage(&[hit("malformed", true)], 4_000, &tenant());
    assert_eq!(block.included_count, 1);
    assert!(block.content.contains("id=malformed"));
    assert!(block.included_memory.is_empty());
    assert!(!block.lineage_complete);
    let mut budget = budget(4_000);
    let mut messages = Vec::new();
    let (injected, sources, complete) = budget.push_memory_context(&mut messages, &block);
    assert!(injected);
    let result = budget.finish_result(messages, sources, complete);
    assert_eq!(result.messages.len(), 1);
    assert!(!result.lineage_complete);
}

#[test]
fn successful_memory_gate_captures_reference_but_unknown_docs_are_incomplete() {
    let block = build_memory_block_with_lineage(&[hit("included", false)], 4_000, &tenant());
    let mut budget = budget(8_000);
    let mut messages = Vec::new();
    assert!(budget.push_system_message(
        &mut messages,
        SOURCE_DOCS,
        "unresolved document evidence".into(),
        1,
        false
    ));
    let (injected, sources, complete) = budget.push_memory_context(&mut messages, &block);
    assert!(injected);
    let result = budget.finish_result(messages, sources, complete);
    assert_eq!(result.included_memory, block.included_memory);
    assert_eq!(result.messages.len(), 2);
    assert!(!result.lineage_complete);
}
