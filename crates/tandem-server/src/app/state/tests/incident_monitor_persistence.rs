// Regression coverage for TAN-541: incident-monitor state files must be written
// atomically and a corrupt file must be quarantined (preserved) on load rather
// than silently discarded and overwritten with empty state.

use crate::IncidentMonitorPostRecord;

use super::{test_state_with_path, tmp_resource_file};

#[test]
fn constant_time_str_eq_matches_only_identical_values() {
    // TAN-558: secret/token/hash comparisons use a constant-time helper.
    assert!(crate::constant_time_str_eq("token-abc", "token-abc"));
    assert!(!crate::constant_time_str_eq("token-abc", "token-abd"));
    assert!(!crate::constant_time_str_eq(
        "token-abc",
        "token-abc-longer"
    ));
    assert!(!crate::constant_time_str_eq("", "x"));
    assert!(crate::constant_time_str_eq("", ""));
}

fn sample_post(post_id: &str) -> IncidentMonitorPostRecord {
    IncidentMonitorPostRecord {
        post_id: post_id.to_string(),
        draft_id: "draft-1".to_string(),
        fingerprint: "fingerprint-1".to_string(),
        repo: "frumu-ai/tandem".to_string(),
        operation: "create_issue".to_string(),
        status: "posted".to_string(),
        idempotency_key: format!("idem-{post_id}"),
        created_at_ms: 1,
        updated_at_ms: 1,
        ..Default::default()
    }
}

#[tokio::test]
async fn incident_monitor_posts_round_trip_persists_atomically() {
    let mut state = test_state_with_path(tmp_resource_file("im-posts-roundtrip"));
    let posts_path = tmp_resource_file("im-posts-roundtrip-posts");
    state.incident_monitor_posts_path = posts_path.clone();

    state
        .incident_monitor_posts
        .write()
        .await
        .insert("p1".to_string(), sample_post("p1"));
    state
        .persist_incident_monitor_posts()
        .await
        .expect("persist posts");

    // The persisted file is valid JSON and the temp file used for the atomic
    // write did not linger.
    assert!(posts_path.exists());
    assert!(
        !posts_path.with_extension("tmp").exists(),
        "temp file leaked"
    );

    // A fresh state pointed at the same file rehydrates the receipt.
    let mut reloaded = test_state_with_path(tmp_resource_file("im-posts-roundtrip-2"));
    reloaded.incident_monitor_posts_path = posts_path.clone();
    reloaded
        .load_incident_monitor_posts()
        .await
        .expect("load posts");
    let posts = reloaded.incident_monitor_posts.read().await;
    assert_eq!(posts.len(), 1);
    assert!(posts.contains_key("p1"));
}

#[tokio::test]
async fn corrupt_incident_monitor_posts_file_is_quarantined_not_discarded() {
    let mut state = test_state_with_path(tmp_resource_file("im-posts-corrupt"));
    let posts_path = tmp_resource_file("im-posts-corrupt-posts");
    state.incident_monitor_posts_path = posts_path.clone();

    // Simulate a torn/corrupt state file left behind by a crash mid-write.
    tokio::fs::write(&posts_path, b"{ this is not valid json")
        .await
        .expect("write corrupt file");

    // Load must not error and must not leave the corrupt bytes where the next
    // persist would overwrite (and permanently lose) them.
    state
        .load_incident_monitor_posts()
        .await
        .expect("load must not fail on corrupt file");
    assert!(state.incident_monitor_posts.read().await.is_empty());

    // The corrupt file was moved aside, not left in place to be overwritten.
    assert!(
        !posts_path.exists(),
        "corrupt file should have been quarantined away from the canonical path"
    );

    // A `.corrupt-` sibling preserves the original bytes for recovery.
    let dir = posts_path.parent().expect("temp dir");
    let original_name = posts_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name")
        .to_string();
    let mut found_quarantine = false;
    let mut entries = tokio::fs::read_dir(dir).await.expect("read temp dir");
    while let Some(entry) = entries.next_entry().await.expect("dir entry") {
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        if name.starts_with(&original_name) && name.contains(".corrupt-") {
            let contents = tokio::fs::read_to_string(entry.path())
                .await
                .expect("read quarantined file");
            assert!(
                contents.contains("not valid json"),
                "quarantined file should preserve the original bytes"
            );
            found_quarantine = true;
        }
    }
    assert!(
        found_quarantine,
        "expected a quarantined .corrupt- sibling preserving the corrupt bytes"
    );
}

#[tokio::test]
async fn retention_prune_removes_stale_receipts_but_keeps_fresh() {
    // TAN-556: safety_defaults.retention_days must actually prune old receipts /
    // incidents instead of letting them accumulate unbounded.
    let mut state = test_state_with_path(tmp_resource_file("im-retention"));
    state.incident_monitor_posts_path = tmp_resource_file("im-retention-posts");
    state.incident_monitor_incidents_path = tmp_resource_file("im-retention-incidents");

    let now = crate::now_ms();
    let day_ms = 24 * 60 * 60 * 1_000u64;

    let mut stale = sample_post("stale");
    stale.updated_at_ms = now.saturating_sub(30 * day_ms);
    let mut fresh = sample_post("fresh");
    fresh.updated_at_ms = now;
    {
        let mut guard = state.incident_monitor_posts.write().await;
        guard.insert("stale".to_string(), stale);
        guard.insert("fresh".to_string(), fresh);
    }

    // A zero window prunes nothing.
    assert_eq!(
        state
            .prune_incident_monitor_retention(0)
            .await
            .expect("no-op prune"),
        (0, 0, 0)
    );
    assert_eq!(state.incident_monitor_posts.read().await.len(), 2);

    // A 7-day window drops the 30-day-old receipt and keeps the fresh one.
    let (posts, _incidents, _artifacts) = state
        .prune_incident_monitor_retention(7)
        .await
        .expect("prune");
    assert_eq!(posts, 1, "the stale receipt should have been pruned");
    let guard = state.incident_monitor_posts.read().await;
    assert!(guard.contains_key("fresh"));
    assert!(!guard.contains_key("stale"));
}
