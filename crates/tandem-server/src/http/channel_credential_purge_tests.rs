// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use fs2::FileExt;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

const WORKER_TEST: &str =
    "http::channels_api::channel_credential_purge::tests::channel_credential_purge_worker";
const SLACK_BOT: &str = "channel::slack::bot_token";
const SLACK_REMOVED_BOT: &str = "channel::slack::connection::t1::a1::removed::bot_token";
const SLACK_REMOVED_SIGNING: &str = "channel::slack::connection::t1::a1::removed::signing_secret";
const SLACK_KEPT_BOT: &str = "channel::slack::connection::t1::a1::kept::bot_token";
const SLACK_SIGNING: &str = "channel::slack::signing_secret::t1::a1";
const TELEGRAM_BOT: &str = "channel::telegram::bot_token";
const UNRELATED_PROVIDER: &str = "synthetic-unrelated-provider";

fn run_isolated_worker(action: &str) {
    let dir = tempfile::tempdir().expect("isolated credential home");
    // Child-only overrides avoid racing other tests' process-global provider
    // paths or ever opening the real keychain or credential store.
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", WORKER_TEST, "--nocapture"])
        .env("TANDEM_TEST_CHANNEL_CREDENTIAL_PURGE_WORKER", action)
        .env("TANDEM_TEST_CHANNEL_CREDENTIAL_PURGE_HOME", dir.path())
        .env("TANDEM_HOME", dir.path())
        .env("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING", "1")
        .output()
        .expect("run isolated credential purge worker");
    assert!(
        output.status.success(),
        "{action}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        dir.path()
            .join("security/provider_auth_index.json")
            .is_file(),
        "the exact worker test must run, not silently match zero tests"
    );
}

#[test]
fn selected_channel_credential_purge_yields_and_awaits_native_lock() {
    run_isolated_worker("selected");
}

#[test]
fn deleted_slack_credential_purge_yields_and_awaits_whole_purge() {
    run_isolated_worker("delete-slack");
}

#[test]
fn deleted_telegram_credential_purge_preserves_slack_credentials() {
    run_isolated_worker("delete-telegram");
}

#[test]
fn channel_credential_purge_worker() {
    let Ok(action) = std::env::var("TANDEM_TEST_CHANNEL_CREDENTIAL_PURGE_WORKER") else {
        return;
    };
    let home = std::path::PathBuf::from(
        std::env::var_os("TANDEM_TEST_CHANNEL_CREDENTIAL_PURGE_HOME")
            .expect("isolated worker credential home"),
    );
    assert_eq!(
        std::env::var_os("TANDEM_HOME").as_deref(),
        Some(home.as_os_str())
    );
    assert_eq!(
        std::env::var("TANDEM_PROVIDER_AUTH_DISABLE_KEYRING").as_deref(),
        Ok("1")
    );
    assert!(home.starts_with(std::env::temp_dir()));

    let seeded: HashMap<String, String> = [
        SLACK_BOT,
        SLACK_REMOVED_BOT,
        SLACK_REMOVED_SIGNING,
        SLACK_KEPT_BOT,
        SLACK_SIGNING,
        TELEGRAM_BOT,
        UNRELATED_PROVIDER,
    ]
    .into_iter()
    .map(|id| (id.to_string(), format!("synthetic-{id}")))
    .collect();
    for (id, secret) in &seeded {
        assert_eq!(
            tandem_core::set_provider_auth(id, secret).expect("seed isolated file credential"),
            tandem_core::ProviderAuthBackend::File
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread worker runtime");
    runtime.block_on(assert_purge_yields_and_completes(&home, &action, seeded));
}

async fn assert_purge_yields_and_completes(
    home: &Path,
    action: &str,
    mut expected: HashMap<String, String>,
) {
    let lock_path = home.join("security/provider_credentials.lock");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let blocker = std::thread::spawn(move || {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("existing isolated credential lock");
        file.lock_exclusive().expect("hold native credential lock");
        ready_tx.send(()).expect("announce held lock");
        // A regression must fail instead of hanging the process while its
        // blocked current-thread runtime cannot fire the async timer.
        let _ = release_rx.recv_timeout(Duration::from_secs(3));
        FileExt::unlock(&file).expect("release native credential lock");
    });
    ready_rx.recv().expect("credential lock is held");

    let purge = async {
        match action {
            "selected" => {
                purge_selected(HashSet::from([
                    SLACK_REMOVED_BOT.to_string(),
                    SLACK_REMOVED_SIGNING.to_string(),
                ]))
                .await
            }
            "delete-slack" => purge_deleted("slack").await,
            "delete-telegram" => purge_deleted("telegram").await,
            _ => panic!("unknown credential purge worker action: {action}"),
        }
    };
    tokio::pin!(purge);
    let started = Instant::now();
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut purge)
            .await
            .is_err(),
        "purge must await the held file lock"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the async timer must remain responsive while deletion waits"
    );
    assert_eq!(
        tandem_core::load_provider_auth(),
        expected,
        "no selected credential can be removed before the file lock releases"
    );
    release_tx.send(()).expect("allow purge to acquire lock");
    tokio::time::timeout(Duration::from_secs(3), &mut purge)
        .await
        .expect("purge finishes after file lock releases")
        .expect("blocking purge joins successfully");
    blocker.join().expect("native lock holder exits");

    match action {
        "selected" => {
            expected.remove(SLACK_REMOVED_BOT);
            expected.remove(SLACK_REMOVED_SIGNING);
        }
        "delete-slack" => expected.retain(|id, _| !id.starts_with("channel::slack::")),
        "delete-telegram" => {
            expected.remove(TELEGRAM_BOT);
        }
        _ => unreachable!(),
    }
    assert_eq!(
        tandem_core::load_provider_auth(),
        expected,
        "awaited purge must finish all selected deletions and preserve unrelated credentials"
    );
}
