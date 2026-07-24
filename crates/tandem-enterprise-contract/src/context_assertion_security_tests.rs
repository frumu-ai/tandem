use super::*;
use crate::{
    AuthorityChain, HumanActor, KeyStatus, RequestPrincipal, SigningKeyPurpose, TenantContext,
    VerifierKeyEntry, VerifierKeyring,
};
use ed25519_dalek::{Signer, SigningKey};
use std::sync::{Arc, Barrier};

const NOW_MS: u64 = 1_800_000_000_000;
const AUDIENCE: &str = "tandem-runtime";

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn key_entry(key: &SigningKey, kid: &str, status: KeyStatus) -> VerifierKeyEntry {
    VerifierKeyEntry::new(
        kid,
        SigningKeyPurpose::ContextAssertion,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes()),
    )
    .with_organization_id("org-a")
    .with_deployment_id("dep-a")
    .with_allowed_audiences(vec![AUDIENCE.to_string()])
    .with_status(status)
}

fn claims(
    assertion_id: &str,
    issued_at_ms: u64,
    expires_at_ms: u64,
) -> TenantContextAssertionClaims {
    let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
    TenantContextAssertionClaims::new_v1(
        DEFAULT_CONTEXT_ASSERTION_ISSUER,
        AUDIENCE,
        issued_at_ms,
        expires_at_ms,
        assertion_id,
        TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        ),
        HumanActor::tandem_user("user-a"),
        AuthorityChain::from_request(principal),
        vec!["workspace:user".to_string()],
    )
}

fn verifier(entries: impl IntoIterator<Item = VerifierKeyEntry>) -> ContextAssertionVerifier {
    ContextAssertionVerifier::new(
        VerifierKeyring::from_entries(entries),
        ContextAssertionPolicy::new(
            DEFAULT_CONTEXT_ASSERTION_ISSUER,
            AUDIENCE,
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
        )
        .expect("policy"),
    )
    .expect("verifier")
}

fn sign(key: &SigningKey, kid: &str, claims: &TenantContextAssertionClaims) -> String {
    let header = TenantContextAssertionHeader::ed25519(kid);
    let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("header"));
    let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(claims).expect("claims"));
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

fn replay_record(
    assertion_id: &str,
    fingerprint_byte: u8,
    issuer: &str,
    audience: &str,
    expires_at_ms: u64,
) -> VerifiedContextAssertion {
    let mut claims = claims(assertion_id, NOW_MS, expires_at_ms);
    claims.issuer = issuer.to_string();
    claims.audience = audience.to_string();
    VerifiedContextAssertion {
        claims,
        key_id: "key-a".to_string(),
        fingerprint: [fingerprint_byte; 32],
    }
}

#[test]
fn signed_overlong_lifetime_is_rejected_but_exact_boundary_is_valid() {
    let key = signing_key(7);
    let verifier = verifier([key_entry(&key, "key-a", KeyStatus::Active)]);
    let overlong = claims("overlong", NOW_MS, NOW_MS + 48 * 60 * 60 * 1_000);
    assert_eq!(
        verifier.verify_at(&sign(&key, "key-a", &overlong), NOW_MS),
        Err(ContextAssertionError::LifetimeExceeded)
    );

    let boundary = claims(
        "boundary",
        NOW_MS + DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
        NOW_MS
            + DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS
            + DEFAULT_CONTEXT_ASSERTION_MAX_LIFETIME_MS,
    );
    assert!(verifier
        .verify_at(&sign(&key, "key-a", &boundary), NOW_MS)
        .is_ok());
}

#[test]
fn invalid_time_order_and_checked_arithmetic_fail_closed() {
    let key = signing_key(8);
    let verifier = verifier([key_entry(&key, "key-a", KeyStatus::Active)]);
    let zero_lifetime = claims("zero", NOW_MS, NOW_MS);
    assert_eq!(
        verifier.verify_at(&sign(&key, "key-a", &zero_lifetime), NOW_MS),
        Err(ContextAssertionError::Expired)
    );
    let ordinary = claims("overflow", 1, 2);
    assert_eq!(
        verifier.verify_at(&sign(&key, "key-a", &ordinary), u64::MAX),
        Err(ContextAssertionError::TimeOverflow)
    );
    assert_eq!(
        ContextAssertionPolicy::new("issuer", "audience", 1, u64::MAX),
        Err(ContextAssertionError::InvalidPolicy)
    );
}

#[test]
fn signature_is_verified_before_untrusted_claim_semantics() {
    let trusted = signing_key(9);
    let attacker = signing_key(10);
    let verifier = verifier([key_entry(&trusted, "key-a", KeyStatus::Active)]);
    let mut invalid = claims("bad-issuer", NOW_MS, NOW_MS + 60_000);
    invalid.issuer = "attacker".to_string();
    invalid.tenant_context.org_id = "attacker-org".to_string();
    assert_eq!(
        verifier.verify_at(&sign(&attacker, "key-a", &invalid), NOW_MS),
        Err(ContextAssertionError::BadSignature)
    );
}

#[test]
fn active_rotation_key_succeeds_while_retired_and_revoked_keys_fail() {
    let active = signing_key(11);
    let retired = signing_key(12);
    let revoked = signing_key(13);
    let verifier = verifier([
        key_entry(&active, "active", KeyStatus::Active),
        key_entry(&retired, "retired", KeyStatus::Retired),
        key_entry(&revoked, "revoked", KeyStatus::Revoked),
    ]);
    let claims = claims("rotation", NOW_MS, NOW_MS + 60_000);
    assert!(verifier
        .verify_at(&sign(&active, "active", &claims), NOW_MS)
        .is_ok());
    assert_eq!(
        verifier.verify_at(&sign(&retired, "retired", &claims), NOW_MS),
        Err(ContextAssertionError::KeyringDenied(
            KeyringDenial::KeyRetired
        ))
    );
    assert_eq!(
        verifier.verify_at(&sign(&revoked, "revoked", &claims), NOW_MS),
        Err(ContextAssertionError::KeyringDenied(
            KeyringDenial::KeyRevoked
        ))
    );
}

#[test]
fn persistent_replay_store_coordinates_one_shot_and_bound_across_instances() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("replay.json");
    let first = ContextAssertionReplayStore::persistent(&path).expect("first store");
    let second = ContextAssertionReplayStore::persistent(&path).expect("second store");
    let exact = replay_record("shared", 1, "issuer-a", "aud-a", NOW_MS + 60_000);
    let substituted = replay_record("shared", 2, "issuer-a", "aud-a", NOW_MS + 60_000);

    first
        .check_and_record(ContextAssertionReplayMode::Bound, &exact, NOW_MS)
        .expect("first bound presentation");
    second
        .check_and_record(ContextAssertionReplayMode::Bound, &exact, NOW_MS)
        .expect("exact retry on another instance");
    assert_eq!(
        second.check_and_record(ContextAssertionReplayMode::Bound, &substituted, NOW_MS),
        Err(ContextAssertionError::Replayed)
    );

    let one_shot = replay_record("one-shot", 3, "issuer-a", "aud-a", NOW_MS + 60_000);
    first
        .check_and_record(ContextAssertionReplayMode::OneShot, &one_shot, NOW_MS)
        .expect("first one-shot presentation");
    assert_eq!(
        second.check_and_record(ContextAssertionReplayMode::OneShot, &one_shot, NOW_MS),
        Err(ContextAssertionError::Replayed)
    );
}

#[test]
fn concurrent_one_shot_race_has_exactly_one_winner() {
    const PARTICIPANTS: usize = 8;
    let temp = tempfile::tempdir().expect("tempdir");
    let path = Arc::new(temp.path().join("race.json"));
    let barrier = Arc::new(Barrier::new(PARTICIPANTS));
    let record = Arc::new(replay_record(
        "race",
        4,
        "issuer-a",
        "aud-a",
        NOW_MS + 60_000,
    ));
    let handles = (0..PARTICIPANTS)
        .map(|_| {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            let record = Arc::clone(&record);
            std::thread::spawn(move || {
                let store = ContextAssertionReplayStore::persistent(path.as_ref())
                    .expect("concurrent store");
                barrier.wait();
                store.check_and_record(ContextAssertionReplayMode::OneShot, &record, NOW_MS)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(ContextAssertionError::Replayed))
            .count(),
        PARTICIPANTS - 1
    );
}

#[test]
fn replay_expiry_namespace_and_capacity_limits_are_enforced() {
    let store = ContextAssertionReplayStore::in_memory().with_limits(2, 1);
    let first = replay_record("first", 5, "issuer-a", "aud-a", NOW_MS + 1_000);
    store
        .check_and_record(ContextAssertionReplayMode::OneShot, &first, NOW_MS)
        .expect("first namespace entry");
    let same_namespace = replay_record("second", 6, "issuer-a", "aud-a", NOW_MS + 1_000);
    assert_eq!(
        store.check_and_record(ContextAssertionReplayMode::OneShot, &same_namespace, NOW_MS),
        Err(ContextAssertionError::ReplayCapacityExceeded)
    );
    let other_namespace = replay_record("third", 7, "issuer-b", "aud-b", NOW_MS + 1_000);
    store
        .check_and_record(
            ContextAssertionReplayMode::OneShot,
            &other_namespace,
            NOW_MS,
        )
        .expect("isolated namespace");
    let global_limit = replay_record("fourth", 8, "issuer-c", "aud-c", NOW_MS + 1_000);
    assert_eq!(
        store.check_and_record(ContextAssertionReplayMode::OneShot, &global_limit, NOW_MS),
        Err(ContextAssertionError::ReplayCapacityExceeded)
    );

    let replacement = replay_record("first", 9, "issuer-a", "aud-a", NOW_MS + 120_000);
    store
        .check_and_record(
            ContextAssertionReplayMode::Bound,
            &replacement,
            NOW_MS + 61_000,
        )
        .expect("expired record pruned after grace");
}

#[cfg(unix)]
#[test]
fn persistent_replay_store_rejects_permissive_files_and_symlinks() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = tempfile::tempdir().expect("tempdir");
    let permissive = temp.path().join("permissive.json");
    std::fs::write(&permissive, "").expect("state file");
    std::fs::set_permissions(&permissive, std::fs::Permissions::from_mode(0o644))
        .expect("permissions");
    assert!(matches!(
        ContextAssertionReplayStore::persistent(&permissive),
        Err(ContextAssertionError::ReplayBackendUnavailable)
    ));

    let target = temp.path().join("target.json");
    std::fs::write(&target, "").expect("target file");
    let linked = temp.path().join("linked.json");
    symlink(&target, &linked).expect("symlink");
    assert!(matches!(
        ContextAssertionReplayStore::persistent(&linked),
        Err(ContextAssertionError::ReplayBackendUnavailable)
    ));
}

#[cfg(unix)]
#[test]
fn running_replay_store_fails_closed_and_releases_lock_after_state_unlink() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("unlinked.json");
    let store = ContextAssertionReplayStore::persistent(&path).expect("store");
    std::fs::remove_file(&path).expect("unlink state");
    let record = replay_record("unlinked", 11, "issuer-a", "aud-a", NOW_MS + 60_000);
    for _ in 0..2 {
        assert_eq!(
            store.check_and_record(ContextAssertionReplayMode::OneShot, &record, NOW_MS),
            Err(ContextAssertionError::ReplayBackendUnavailable)
        );
    }
}

#[test]
fn replay_backend_corruption_and_unavailable_paths_fail_closed_without_plaintext_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("private-replay.json");
    let store = ContextAssertionReplayStore::persistent(&path).expect("store");
    let record = replay_record(
        "sensitive-assertion-id",
        10,
        "private-issuer",
        "private-audience",
        NOW_MS + 60_000,
    );
    store
        .check_and_record(ContextAssertionReplayMode::OneShot, &record, NOW_MS)
        .expect("record");
    let raw = std::fs::read_to_string(&path).expect("state");
    assert!(!raw.contains("sensitive-assertion-id"));
    assert!(!raw.contains("private-issuer"));
    assert!(!raw.contains("private-audience"));

    std::fs::write(&path, "{corrupt").expect("corrupt state");
    assert!(matches!(
        ContextAssertionReplayStore::persistent(&path),
        Err(ContextAssertionError::ReplayBackendUnavailable)
    ));
    assert!(matches!(
        ContextAssertionReplayStore::persistent(temp.path().join("missing").join("state.json")),
        Err(ContextAssertionError::ReplayBackendUnavailable)
    ));
}
