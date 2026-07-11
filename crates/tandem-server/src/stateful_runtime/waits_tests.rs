use std::path::PathBuf;

use serde_json::json;
use tandem_types::TenantContext;
use uuid::Uuid;

use super::*;

fn tenant(org: &str, workspace: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
}

fn timer_wait(
    wait_id: &str,
    run_id: &str,
    tenant_context: TenantContext,
    wake_at_ms: u64,
) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: 1,
        wait_id: wait_id.to_string(),
        run_id: run_id.to_string(),
        wait_kind: StatefulWaitKind::Timer,
        status: StatefulWaitStatus::Waiting,
        scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
        phase_id: Some("phase-a".to_string()),
        reason: Some("sleep until downstream system is ready".to_string()),
        created_at_ms: wake_at_ms.saturating_sub(100),
        updated_at_ms: wake_at_ms.saturating_sub(100),
        wake_at_ms: Some(wake_at_ms),
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(json!({ "source": "test" })),
    }
}

fn webhook_wait(
    wait_id: &str,
    run_id: &str,
    tenant_context: TenantContext,
    match_rules: StatefulWebhookWaitMatch,
) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: 1,
        wait_id: wait_id.to_string(),
        run_id: run_id.to_string(),
        wait_kind: StatefulWaitKind::Webhook,
        status: StatefulWaitStatus::Waiting,
        scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
        phase_id: Some("phase-webhook".to_string()),
        reason: Some("wait for correlated webhook".to_string()),
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        wake_at_ms: None,
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(stateful_webhook_wait_metadata(match_rules, None)),
    }
}

fn webhook_event(trigger_id: &str, provider_event_id: Option<&str>) -> StatefulWebhookWaitEvent {
    StatefulWebhookWaitEvent {
        trigger_id: trigger_id.to_string(),
        provider: "github".to_string(),
        provider_event_kind: Some("issues.opened".to_string()),
        provider_event_id: provider_event_id.map(ToOwned::to_owned),
        body_digest: "sha256:body".to_string(),
        idempotency_key: provider_event_id
            .map(|event_id| format!("github:{event_id}"))
            .unwrap_or_else(|| "sha256:body".to_string()),
    }
}

fn temp_wait_store(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{name}-{}.json", Uuid::new_v4()))
}

#[tokio::test]
async fn wait_store_round_trips_and_filters_by_tenant() {
    let path = temp_wait_store("stateful-waits-filtered");
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait-a");
    upsert_stateful_wait(&path, timer_wait("wait-b", "run-a", tenant_b.clone(), 900))
        .await
        .expect("insert wait-b");

    let visible = list_stateful_waits(
        &path,
        &tenant_a,
        StatefulWaitQuery {
            run_id: Some("run-a"),
            ..StatefulWaitQuery::default()
        },
    );

    assert_eq!(
        visible
            .iter()
            .map(|wait| wait.wait_id.as_str())
            .collect::<Vec<_>>(),
        vec!["wait-a"]
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn wait_mutations_sideline_corrupt_store_instead_of_overwriting() {
    let path = temp_wait_store("stateful-waits-corrupt");
    std::fs::write(&path, "{not-valid-json").expect("write corrupt wait store");
    let corrupt_path = path.with_extension("json.corrupt");

    let result = upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant("org-a", "workspace-a"), 1_000),
    )
    .await;

    let error = result.expect_err("corrupt store should block mutation");
    assert!(error.to_string().contains("corrupt store moved"));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&corrupt_path).expect("read corrupt wait store"),
        "{not-valid-json"
    );
    let _ = tokio::fs::remove_file(corrupt_path).await;
}

#[tokio::test]
async fn duplicate_wait_ids_are_scoped_by_tenant_boundary() {
    let path = temp_wait_store("stateful-waits-tenant-boundary");
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    upsert_stateful_wait(
        &path,
        timer_wait("shared-wait", "run-a", tenant_b.clone(), 900),
    )
    .await
    .expect("insert tenant-b wait");
    upsert_stateful_wait(
        &path,
        timer_wait("shared-wait", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert tenant-a wait");

    let all_waits = load_stateful_waits(&path);
    assert_eq!(all_waits.len(), 2);
    let tenant_a_due = due_stateful_waits(&path, &tenant_a, 1_500, None);
    assert_eq!(tenant_a_due.len(), 1);
    assert_eq!(tenant_a_due[0].scope.organization_id(), "org-a");

    let claimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "shared-wait",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("tenant-a claim")
    .expect("tenant-a wait");
    assert_eq!(claimed.scope.organization_id(), "org-a");
    let tenant_b_waits = list_stateful_waits(
        &path,
        &tenant_b,
        StatefulWaitQuery {
            status: Some(StatefulWaitStatus::Waiting),
            ..StatefulWaitQuery::default()
        },
    );
    assert_eq!(tenant_b_waits.len(), 1);
    assert_eq!(tenant_b_waits[0].scope.organization_id(), "org-b");
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn duplicate_wait_ids_are_claimed_by_run_identity() {
    let path = temp_wait_store("stateful-waits-run-boundary");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("shared-wait", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert run-a wait");
    upsert_stateful_wait(
        &path,
        timer_wait("shared-wait", "run-b", tenant_a.clone(), 1_100),
    )
    .await
    .expect("insert run-b wait");

    let run_a = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "shared-wait",
        "worker-a",
        1_500,
        500,
    )
    .await
    .expect("claim run-a")
    .expect("run-a wait");
    assert_eq!(run_a.run_id, "run-a");

    let run_b = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-b",
        "shared-wait",
        "worker-b",
        1_600,
        500,
    )
    .await
    .expect("claim run-b")
    .expect("run-b wait");
    assert_eq!(run_b.run_id, "run-b");
    let missing = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-c",
        "shared-wait",
        "worker-c",
        1_700,
        500,
    )
    .await
    .expect("claim missing run");
    assert!(missing.is_none());
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn due_waits_select_missed_timer_wakeups_in_order() {
    let path = temp_wait_store("stateful-waits-due");
    let tenant_a = tenant("org-a", "workspace-a");
    for (wait_id, wake_at_ms) in [("future", 2_000), ("oldest", 500), ("due", 1_000)] {
        upsert_stateful_wait(
            &path,
            timer_wait(wait_id, "run-a", tenant_a.clone(), wake_at_ms),
        )
        .await
        .expect("insert wait");
    }

    let due = due_stateful_waits(&path, &tenant_a, 1_500, Some(10));

    assert_eq!(
        due.iter()
            .map(|wait| wait.wait_id.as_str())
            .collect::<Vec<_>>(),
        vec!["oldest", "due"]
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn due_wait_claim_is_single_claimant_until_lease_expires() {
    let path = temp_wait_store("stateful-waits-claim");
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");

    assert!(claim_due_stateful_wait(
        &path,
        &tenant_b,
        "run-a",
        "wait-a",
        "scheduler-b",
        1_500,
        500
    )
    .await
    .expect("cross-tenant claim")
    .is_none());
    let claimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("first claim")
    .expect("claim record");
    assert_eq!(claimed.claimed_by.as_deref(), Some("scheduler-a"));
    assert!(claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-b",
        1_600,
        500
    )
    .await
    .expect("second claim")
    .is_none());
    assert!(due_stateful_waits(&path, &tenant_a, 1_600, None).is_empty());

    let expired_claims = due_stateful_waits(&path, &tenant_a, 2_100, None);
    assert_eq!(
        expired_claims
            .iter()
            .map(|wait| wait.wait_id.as_str())
            .collect::<Vec<_>>(),
        vec!["wait-a"]
    );

    let reclaimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-b",
        2_100,
        500,
    )
    .await
    .expect("reclaim")
    .expect("reclaimed record");
    assert_eq!(reclaimed.claimed_by.as_deref(), Some("scheduler-b"));
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn regressed_due_clock_does_not_expire_active_claim_lease() {
    let path = temp_wait_store("stateful-waits-regressed-claim-lease");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");

    let claimed = claim_due_stateful_wait_with_lease_clock(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        2_000,
        1_000,
        500,
    )
    .await
    .expect("claim wait")
    .expect("claim record");
    assert_eq!(claimed.claim_expires_at_ms, Some(1_500));

    assert!(claim_due_stateful_wait_with_lease_clock(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-b",
        2_000,
        1_400,
        500,
    )
    .await
    .expect("active lease reclaim")
    .is_none());

    let reclaimed = claim_due_stateful_wait_with_lease_clock(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-b",
        2_000,
        1_500,
        500,
    )
    .await
    .expect("expired lease reclaim")
    .expect("reclaimed record");
    assert_eq!(reclaimed.claimed_by.as_deref(), Some("scheduler-b"));
    assert_eq!(reclaimed.claim_expires_at_ms, Some(2_000));
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn version_scoped_claim_rejects_updated_wait() {
    let path = temp_wait_store("stateful-waits-version-claim");
    let tenant_a = tenant("org-a", "workspace-a");
    let mut original = timer_wait("wait-a", "run-a", tenant_a.clone(), 1_900);
    original.created_at_ms = 1_800;
    original.updated_at_ms = 1_800;
    upsert_stateful_wait(&path, original.clone())
        .await
        .expect("insert original wait");

    let mut updated = original.clone();
    updated.wake_at_ms = Some(1_500);
    updated.updated_at_ms = 1_050;
    upsert_stateful_wait(&path, updated)
        .await
        .expect("update wait in place");

    assert!(claim_due_stateful_wait_version_with_lease_clock(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        original.created_at_ms,
        original.updated_at_ms,
        "scheduler-a",
        2_000,
        1_000,
        500,
    )
    .await
    .expect("claim stale version")
    .is_none());

    let claimed = claim_due_stateful_wait_version_with_lease_clock(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        original.created_at_ms,
        1_050,
        "scheduler-a",
        2_000,
        1_000,
        500,
    )
    .await
    .expect("claim current version")
    .expect("current version claimed");
    assert_eq!(claimed.updated_at_ms, 1_000);
    assert_eq!(claimed.claimed_by.as_deref(), Some("scheduler-a"));
    let _ = tokio::fs::remove_file(path).await;
}

#[test]
fn expired_claimed_timer_wait_without_timeout_remains_claimable() {
    let tenant_a = tenant("org-a", "workspace-a");
    let mut wait = timer_wait("wait-a", "run-a", tenant_a, 1_000);
    wait.status = StatefulWaitStatus::Claimed;
    wait.claimed_by = Some("scheduler-a".to_string());
    wait.claimed_at_ms = Some(1_500);
    wait.claim_expires_at_ms = Some(2_000);

    assert!(!wait_is_claimable(&wait, 1_999, 1_999));
    assert!(wait_is_claimable(&wait, 2_000, 2_000));
}

#[tokio::test]
async fn wake_completion_is_idempotent_and_terminal() {
    let path = temp_wait_store("stateful-waits-woken");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");

    let woken =
        mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_600)
            .await
            .expect("mark woken")
            .expect("woken record");
    assert_eq!(woken.status, StatefulWaitStatus::Woken);
    assert_eq!(woken.event_seq, Some(42));

    let duplicate =
        mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_700)
            .await
            .expect("duplicate wake")
            .expect("duplicate record");
    assert_eq!(duplicate.completed_at_ms, Some(1_600));
    assert!(
        mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "other-key", 43, 1_800)
            .await
            .expect("conflicting wake")
            .is_none()
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn direct_completion_does_not_override_active_claim() {
    let path = temp_wait_store("stateful-waits-active-claim-completion");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");
    claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("claim wait")
    .expect("claimed wait");

    assert!(
        mark_stateful_wait_woken(&path, &tenant_a, "run-a", "wait-a", "wake-key", 42, 1_600)
            .await
            .expect("direct wake completion")
            .is_none()
    );
    assert!(mark_stateful_wait_timeout_result(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "timeout-key",
        43,
        StatefulWaitStatus::TimedOut,
        1_700
    )
    .await
    .expect("direct timeout completion")
    .is_none());

    let wait = list_stateful_waits(
        &path,
        &tenant_a,
        StatefulWaitQuery {
            run_id: Some("run-a"),
            ..StatefulWaitQuery::default()
        },
    )
    .into_iter()
    .next()
    .expect("wait remains");
    assert_eq!(wait.status, StatefulWaitStatus::Claimed);
    assert_eq!(wait.claimed_by.as_deref(), Some("scheduler-a"));
    assert_eq!(wait.event_seq, None);
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn claimed_wait_completion_requires_matching_active_claim() {
    let path = temp_wait_store("stateful-waits-claimed-completion");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");
    let claimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("claim wait")
    .expect("claimed record");

    let mut stale_claimant = claimed.clone();
    stale_claimant.claimed_by = Some("scheduler-b".to_string());
    assert!(begin_claimed_stateful_wait_wake_completion(
        &path,
        &tenant_a,
        &stale_claimant,
        "wake-key",
        1_600
    )
    .await
    .expect("stale claimant")
    .is_none());

    assert!(begin_claimed_stateful_wait_wake_completion(
        &path, &tenant_a, &claimed, "wake-key", 2_000
    )
    .await
    .expect("expired claim")
    .is_none());

    let reserved =
        begin_claimed_stateful_wait_wake_completion(&path, &tenant_a, &claimed, "wake-key", 1_700)
            .await
            .expect("reserve claimed wait completion")
            .expect("reserved record");
    assert_eq!(reserved.status, StatefulWaitStatus::Claimed);
    assert_eq!(reserved.wake_idempotency_key.as_deref(), Some("wake-key"));
    assert_eq!(reserved.event_seq, None);
    assert_eq!(reserved.claimed_by.as_deref(), Some("scheduler-a"));
    assert_eq!(reserved.claimed_at_ms, Some(1_500));
    assert_eq!(reserved.claim_expires_at_ms, Some(2_000));

    let reclaimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-b",
        2_000,
        500,
    )
    .await
    .expect("reclaim reserved wait")
    .expect("reclaimed wait");
    assert_eq!(reclaimed.claimed_by.as_deref(), Some("scheduler-b"));
    assert_eq!(reclaimed.wake_idempotency_key, None);

    let reserved = begin_claimed_stateful_wait_wake_completion(
        &path, &tenant_a, &reclaimed, "wake-key", 2_100,
    )
    .await
    .expect("reserve reclaimed wait completion")
    .expect("reserved reclaimed record");
    let completed = finish_claimed_stateful_wait_completion(
        &path,
        &tenant_a,
        &reserved,
        "wake-key",
        42,
        StatefulWaitStatus::Woken,
        2_150,
    )
    .await
    .expect("finish completion")
    .expect("completed wait");
    assert_eq!(completed.status, StatefulWaitStatus::Woken);
    assert_eq!(completed.event_seq, Some(42));
    assert!(completed.claimed_by.is_none());
    assert!(completed.claimed_at_ms.is_none());
    assert!(completed.claim_expires_at_ms.is_none());
    assert!(finish_claimed_stateful_wait_completion(
        &path,
        &tenant_a,
        &reserved,
        "wake-key",
        43,
        StatefulWaitStatus::Woken,
        2_200
    )
    .await
    .expect("conflicting event seq")
    .is_none());
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn release_claimed_wait_restores_waiting_status_and_clears_claim() {
    let path = temp_wait_store("stateful-waits-release-claim");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");
    let claimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("claim wait")
    .expect("claimed wait");

    let released = release_claimed_stateful_wait(&path, &tenant_a, &claimed, 1_550)
        .await
        .expect("release claim")
        .expect("released wait");

    assert_eq!(released.status, StatefulWaitStatus::Waiting);
    assert!(released.claimed_by.is_none());
    assert!(released.claimed_at_ms.is_none());
    assert!(released.claim_expires_at_ms.is_none());
    assert!(released.wake_idempotency_key.is_none());
    assert_eq!(released.updated_at_ms, 1_550);
    assert!(
        release_claimed_stateful_wait(&path, &tenant_a, &claimed, 1_600)
            .await
            .expect("stale release")
            .is_none()
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn phase_guard_denial_cancels_reserved_claimed_wait() {
    let path = temp_wait_store("stateful-waits-phase-guard-cancel");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        timer_wait("wait-a", "run-a", tenant_a.clone(), 1_000),
    )
    .await
    .expect("insert wait");
    let claimed = claim_due_stateful_wait(
        &path,
        &tenant_a,
        "run-a",
        "wait-a",
        "scheduler-a",
        1_500,
        500,
    )
    .await
    .expect("claim wait")
    .expect("claimed wait");
    let reserved =
        begin_claimed_stateful_wait_wake_completion(&path, &tenant_a, &claimed, "wake-key", 1_550)
            .await
            .expect("reserve wait")
            .expect("reserved wait");

    let cancelled = cancel_stateful_wait_after_phase_guard_denial(
        &path,
        &tenant_a,
        &reserved,
        "terminal phase completed",
        1_575,
    )
    .await
    .expect("cancel phase-denied wait")
    .expect("cancelled wait");

    assert_eq!(cancelled.status, StatefulWaitStatus::Cancelled);
    assert_eq!(
        cancelled.wake_idempotency_key.as_deref(),
        Some("phase-guard-denied:wait-a")
    );
    assert_eq!(cancelled.completed_at_ms, Some(1_575));
    assert!(cancelled.claimed_by.is_none());
    assert_eq!(
        cancelled
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("phase_guard_denied"))
            .and_then(|denied| denied.as_bool()),
        Some(true)
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn prune_wait_store_removes_old_terminal_waits() {
    let path = temp_wait_store("stateful-waits-prune-old-terminal");
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    let mut completed_old = timer_wait("old-completed", "run-a", tenant_a.clone(), 2_000);
    completed_old.status = StatefulWaitStatus::Woken;
    completed_old.completed_at_ms = Some(4_000);
    completed_old.updated_at_ms = 9_000;
    let mut updated_old = timer_wait("old-updated", "run-b", tenant_b, 3_000);
    updated_old.status = StatefulWaitStatus::Cancelled;
    updated_old.completed_at_ms = None;
    updated_old.updated_at_ms = 4_999;
    let retained = timer_wait("retained", "run-a", tenant_a, 6_000);

    upsert_stateful_wait(&path, completed_old)
        .await
        .expect("insert completed old wait");
    upsert_stateful_wait(&path, updated_old)
        .await
        .expect("insert updated old wait");
    upsert_stateful_wait(&path, retained)
        .await
        .expect("insert retained wait");

    let pruned = prune_stateful_wait_store(&path, 5_000, 10_000)
        .await
        .expect("prune wait store");

    assert_eq!(pruned, 2);
    let remaining = load_stateful_waits(&path);
    assert_eq!(
        remaining
            .iter()
            .map(|wait| (wait.run_id.as_str(), wait.wait_id.as_str()))
            .collect::<Vec<_>>(),
        vec![("run-a", "retained")]
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn prune_wait_store_retains_stale_non_terminal_waits() {
    let path = temp_wait_store("stateful-waits-prune-stale-non-terminal");
    let tenant_a = tenant("org-a", "workspace-a");
    let mut waiting = timer_wait("stale-waiting", "run-a", tenant_a.clone(), 1_000);
    waiting.updated_at_ms = 1_000;
    let mut claimed = timer_wait("stale-claimed", "run-a", tenant_a, 1_100);
    claimed.status = StatefulWaitStatus::Claimed;
    claimed.updated_at_ms = 1_100;
    claimed.claimed_by = Some("scheduler-a".to_string());
    claimed.claimed_at_ms = Some(1_200);
    claimed.claim_expires_at_ms = Some(1_300);

    upsert_stateful_wait(&path, waiting)
        .await
        .expect("insert stale waiting wait");
    upsert_stateful_wait(&path, claimed)
        .await
        .expect("insert stale claimed wait");

    let pruned = prune_stateful_wait_store(&path, 5_000, 10_000)
        .await
        .expect("prune wait store");

    assert_eq!(pruned, 0);
    let remaining = load_stateful_waits(&path);
    assert_eq!(
        remaining
            .iter()
            .map(|wait| (wait.wait_id.as_str(), &wait.status))
            .collect::<Vec<_>>(),
        vec![
            ("stale-waiting", &StatefulWaitStatus::Waiting),
            ("stale-claimed", &StatefulWaitStatus::Claimed),
        ]
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn prune_wait_store_retains_recent_terminal_waits() {
    let path = temp_wait_store("stateful-waits-prune-recent-terminal");
    let tenant_a = tenant("org-a", "workspace-a");
    let mut at_cutoff = timer_wait("at-cutoff", "run-a", tenant_a.clone(), 1_000);
    at_cutoff.status = StatefulWaitStatus::Woken;
    at_cutoff.completed_at_ms = Some(5_000);
    at_cutoff.updated_at_ms = 4_000;
    let mut recent_fallback = timer_wait("recent-fallback", "run-a", tenant_a, 1_100);
    recent_fallback.status = StatefulWaitStatus::TimedOut;
    recent_fallback.completed_at_ms = None;
    recent_fallback.updated_at_ms = 5_001;

    upsert_stateful_wait(&path, at_cutoff)
        .await
        .expect("insert cutoff terminal wait");
    upsert_stateful_wait(&path, recent_fallback)
        .await
        .expect("insert recent terminal wait");

    let pruned = prune_stateful_wait_store(&path, 5_000, 10_000)
        .await
        .expect("prune wait store");

    assert_eq!(pruned, 0);
    let remaining = load_stateful_waits(&path);
    assert_eq!(
        remaining
            .iter()
            .map(|wait| wait.wait_id.as_str())
            .collect::<Vec<_>>(),
        vec!["at-cutoff", "recent-fallback"]
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn webhook_wait_claim_matches_metadata_once() {
    let path = temp_wait_store("stateful-waits-webhook-match");
    let tenant_a = tenant("org-a", "workspace-a");
    upsert_stateful_wait(
        &path,
        webhook_wait(
            "wait-a",
            "run-a",
            tenant_a.clone(),
            StatefulWebhookWaitMatch {
                trigger_id: Some("trigger-a".to_string()),
                provider_event_id: Some("evt-a".to_string()),
                ..StatefulWebhookWaitMatch::default()
            },
        ),
    )
    .await
    .expect("insert wait");

    assert!(claim_matching_stateful_webhook_wait(
        &path,
        &tenant_a,
        &webhook_event("trigger-a", Some("evt-b")),
        "webhook-router",
        1_500,
        500,
    )
    .await
    .expect("nonmatching event")
    .is_none());
    let claimed = claim_matching_stateful_webhook_wait(
        &path,
        &tenant_a,
        &webhook_event("trigger-a", Some("evt-a")),
        "webhook-router",
        1_500,
        500,
    )
    .await
    .expect("claim")
    .expect("claimed wait");
    assert_eq!(claimed.wait_id, "wait-a");
    assert_eq!(claimed.claimed_by.as_deref(), Some("webhook-router"));
    assert!(claim_matching_stateful_webhook_wait(
        &path,
        &tenant_a,
        &webhook_event("trigger-a", Some("evt-a")),
        "webhook-router-2",
        1_600,
        500,
    )
    .await
    .expect("active duplicate claim")
    .is_none());
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn webhook_wait_claim_is_tenant_scoped_and_ordered() {
    let path = temp_wait_store("stateful-waits-webhook-tenant");
    let tenant_a = tenant("org-a", "workspace-a");
    let tenant_b = tenant("org-b", "workspace-b");
    let match_rules = StatefulWebhookWaitMatch {
        trigger_id: Some("trigger-a".to_string()),
        ..StatefulWebhookWaitMatch::default()
    };
    upsert_stateful_wait(
        &path,
        webhook_wait("wait-b", "run-b", tenant_b.clone(), match_rules.clone()),
    )
    .await
    .expect("insert tenant b");
    upsert_stateful_wait(
        &path,
        webhook_wait("wait-a", "run-a", tenant_a.clone(), match_rules),
    )
    .await
    .expect("insert tenant a");

    let claimed = claim_matching_stateful_webhook_wait(
        &path,
        &tenant_a,
        &webhook_event("trigger-a", Some("evt-a")),
        "webhook-router",
        1_500,
        500,
    )
    .await
    .expect("claim")
    .expect("tenant a wait");
    assert_eq!(claimed.run_id, "run-a");
    let tenant_b_waits = list_stateful_waits(
        &path,
        &tenant_b,
        StatefulWaitQuery {
            status: Some(StatefulWaitStatus::Waiting),
            wait_kind: Some(StatefulWaitKind::Webhook),
            ..StatefulWaitQuery::default()
        },
    );
    assert_eq!(tenant_b_waits.len(), 1);
    let _ = tokio::fs::remove_file(path).await;
}

#[test]
fn timeout_policy_serializes_timeout_action_metadata() {
    let policy = StatefulWaitTimeoutPolicy {
        timeout_at_ms: 2_000,
        on_timeout: StatefulWaitTimeoutAction::Escalate,
        escalate_to: Some("ops-oncall".to_string()),
        remind_every_ms: Some(300),
        metadata: Some(json!({ "channel": "pager" })),
    };

    let serialized = serde_json::to_value(&policy).expect("serialize policy");

    assert_eq!(serialized["on_timeout"], "escalate");
    assert_eq!(serialized["escalate_to"], "ops-oncall");
    assert_eq!(serialized["remind_every_ms"], 300);
}
