// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[cfg(unix)]
use super::*;
#[cfg(unix)]
use std::io::Write;

#[cfg(unix)]
fn receipt(record_id: &str) -> Value {
    serde_json::json!({
        "provider": "incident_monitor_telemetry",
        "operation": "record_telemetry",
        "status": "posted",
        "record_id": record_id,
        "idempotency_key": record_id,
        "destination_id": "telemetry-primary",
        "target_ref": "telemetry:events",
    })
}

#[cfg(unix)]
#[test]
fn directory_sync_walk_includes_parent_of_concurrently_observed_directory() {
    let temp = tempfile::tempdir().unwrap();
    let shared = temp.path().join("created-by-another-process");
    std::fs::create_dir(&shared).unwrap();
    let sink = shared.join("events.jsonl");
    std::fs::write(&sink, b"").unwrap();
    let mut visited = Vec::new();
    visit_telemetry_sink_durable_ancestors(&sink, |directory| {
        visited.push(directory.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(visited.first(), Some(&shared));
    assert!(visited.contains(&temp.path().to_path_buf()));
    assert_eq!(visited.last(), Some(&std::path::PathBuf::from("/")));
}

#[cfg(unix)]
#[test]
fn directory_sync_walk_includes_symlink_target_and_alias_parents() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target_parent = temp.path().join("target-parent");
    let target = target_parent.join("shared");
    let alias_parent = temp.path().join("alias-parent");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir(&alias_parent).unwrap();
    let alias = alias_parent.join("sink-link");
    symlink(&target, &alias).unwrap();
    let sink = alias.join("events.jsonl");
    std::fs::write(&sink, b"").unwrap();

    let mut visited = Vec::new();
    visit_telemetry_sink_durable_ancestors(&sink, |directory| {
        visited.push(directory.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert!(visited.contains(&target));
    assert!(visited.contains(&target_parent));
    assert!(visited.contains(&alias));
    assert!(visited.contains(&alias_parent));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn publish_succeeds_through_execute_only_existing_ancestor() {
    use std::os::unix::fs::PermissionsExt;

    struct RestorePermissions {
        path: std::path::PathBuf,
        original: std::fs::Permissions,
    }

    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.path, self.original.clone()).unwrap();
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
    let ancestor = temp.path().join("execute-only");
    let sink_directory = ancestor.join("sink");
    std::fs::create_dir_all(&sink_directory).unwrap();
    let original = std::fs::metadata(&ancestor).unwrap().permissions();
    let mut execute_only = original.clone();
    execute_only.set_mode(0o111);
    std::fs::set_permissions(&ancestor, execute_only).unwrap();
    let _restore = RestorePermissions {
        path: ancestor.clone(),
        original,
    };

    // A privileged test process can bypass directory permission checks, so it
    // cannot reproduce the EACCES regression. The normal CI user can.
    if std::fs::File::open(&ancestor).is_ok() {
        return;
    }

    let sink = sink_directory.join("events.jsonl");
    let expected = receipt("bmtel_execute-only-ancestor");
    assert_eq!(
        persist_incident_monitor_telemetry(&state, &sink, &expected)
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        std::fs::read_to_string(&sink).unwrap(),
        format!("{expected}\n")
    );
}

#[cfg(unix)]
#[test]
fn same_inode_external_append_invalidates_and_rebuilds_index() {
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir().unwrap();
    let sink = temp.path().join("events.jsonl");
    let first = receipt("bmtel_first");
    let external = receipt("bmtel_external");
    let first_line = format!("{first}\n");
    std::fs::write(&sink, &first_line).unwrap();

    let mut file = std::fs::OpenOptions::new().read(true).open(&sink).unwrap();
    let mut index = TelemetrySinkIndex::default();
    let first_metadata = file.metadata().unwrap();
    index.prepare_file(&first_metadata);
    assert!(!scan_telemetry_sink(&mut file, &mut index).unwrap());
    index.mark_synced(&file.metadata().unwrap());
    let checkpoint = index.scanned_len;
    let first_key = telemetry_receipt_lookup_key(&first).unwrap();
    assert_eq!(checkpoint, first_line.len() as u64);
    assert_eq!(
        index.receipt_offsets.get(&first_key),
        Some(&(0, checkpoint))
    );

    let external_line = format!("{external}\n");
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&sink)
        .unwrap();
    writer.write_all(external_line.as_bytes()).unwrap();
    writer.flush().unwrap();
    writer
        .set_modified(first_metadata.modified().unwrap() + std::time::Duration::from_secs(1))
        .unwrap();
    let appended_metadata = file.metadata().unwrap();
    assert_eq!(first_metadata.dev(), appended_metadata.dev());
    assert_eq!(first_metadata.ino(), appended_metadata.ino());
    assert!(appended_metadata.len() > checkpoint);

    index.prepare_file(&appended_metadata);
    assert_eq!(index.scanned_len, 0);
    assert!(index.receipt_offsets.is_empty());

    assert!(!scan_telemetry_sink(&mut file, &mut index).unwrap());
    let external_key = telemetry_receipt_lookup_key(&external).unwrap();
    assert_eq!(
        index.receipt_offsets.get(&first_key),
        Some(&(0, checkpoint))
    );
    assert_eq!(
        index.receipt_offsets.get(&external_key),
        Some(&(checkpoint, external_line.len() as u64))
    );
}

#[cfg(unix)]
#[test]
fn same_inode_rewrite_and_growth_invalidates_old_receipts() {
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir().unwrap();
    let sink = temp.path().join("events.jsonl");
    let first = receipt("bmtel_first");
    std::fs::write(&sink, format!("{first}\n")).unwrap();
    let mut file = std::fs::OpenOptions::new().read(true).open(&sink).unwrap();
    let mut index = TelemetrySinkIndex::default();
    let before = file.metadata().unwrap();
    index.prepare_file(&before);
    assert!(!scan_telemetry_sink(&mut file, &mut index).unwrap());
    index.mark_synced(&before);
    let first_key = telemetry_receipt_lookup_key(&first).unwrap();
    assert!(index.receipt_offsets.contains_key(&first_key));

    let replacement = receipt("bmtel_replacement");
    let extra = receipt("bmtel_extra");
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&sink)
        .unwrap();
    write!(writer, "{replacement}\n{extra}\n").unwrap();
    writer.flush().unwrap();
    writer
        .set_modified(before.modified().unwrap() + std::time::Duration::from_secs(1))
        .unwrap();
    let after = file.metadata().unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert!(after.len() > before.len());

    index.prepare_file(&after);
    assert_eq!(index.scanned_len, 0);
    assert!(index.receipt_offsets.is_empty());
    assert!(!scan_telemetry_sink(&mut file, &mut index).unwrap());
    assert!(!index.receipt_offsets.contains_key(&first_key));
    assert!(index
        .receipt_offsets
        .contains_key(&telemetry_receipt_lookup_key(&replacement).unwrap()));
    assert!(index
        .receipt_offsets
        .contains_key(&telemetry_receipt_lookup_key(&extra).unwrap()));
}

#[cfg(unix)]
#[tokio::test]
async fn successful_append_keeps_its_index_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
    let sink = temp.path().join("events.jsonl");
    let first = receipt("bmtel_first");
    let second = receipt("bmtel_second");
    persist_incident_monitor_telemetry(&state, &sink, &first)
        .await
        .unwrap();

    let shared = telemetry_sink_shared(&sink).unwrap();
    let first_len = std::fs::metadata(&sink).unwrap().len();
    {
        let mut index = shared.index.lock().unwrap();
        assert_eq!(index.scanned_len, first_len);
        index.prepare_file(&std::fs::metadata(&sink).unwrap());
        assert_eq!(index.scanned_len, first_len);
        assert!(index
            .receipt_offsets
            .contains_key(&telemetry_receipt_lookup_key(&first).unwrap()));
    }

    persist_incident_monitor_telemetry(&state, &sink, &second)
        .await
        .unwrap();
    let mut index = shared.index.lock().unwrap();
    let second_len = std::fs::metadata(&sink).unwrap().len();
    assert!(second_len > first_len);
    assert_eq!(index.scanned_len, second_len);
    index.prepare_file(&std::fs::metadata(&sink).unwrap());
    assert_eq!(index.scanned_len, second_len);
    assert!(index
        .receipt_offsets
        .contains_key(&telemetry_receipt_lookup_key(&first).unwrap()));
    assert!(index
        .receipt_offsets
        .contains_key(&telemetry_receipt_lookup_key(&second).unwrap()));
}

#[cfg(unix)]
#[test]
fn same_sink_waiter_does_not_occupy_blocking_worker() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        let sink = temp.path().join("events.jsonl");
        let expected = receipt("bmtel_admitted");
        let shared = telemetry_sink_shared(&sink).unwrap();
        let admission = shared.admission.clone().lock_owned().await;

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let publisher = tokio::spawn(async move {
            let _ = started_tx.send(());
            persist_incident_monitor_telemetry(&state, &sink, &expected).await
        });
        // Sending happens immediately before the publisher attempts admission.
        // It then yields at the held gate before reaching spawn_blocking.
        started_rx.await.unwrap();
        let unrelated = tokio::task::spawn_blocking(|| 42);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(5), unrelated)
                .await
                .unwrap()
                .unwrap(),
            42
        );
        assert!(!publisher.is_finished());

        drop(admission);
        assert_eq!(publisher.await.unwrap().unwrap(), receipt("bmtel_admitted"));
    });
}
