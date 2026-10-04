// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Keep the owning run separate from the wire payload until the frame leaves
// the queue. Producer authorization cannot protect a queued or blocked send.
#[derive(Debug)]
pub(super) enum ContextRunsQueuedFrame {
    Ready {
        workspace: String,
        subscribed_run_ids: Vec<String>,
        timestamp_ms: u64,
    },
    Envelope {
        run_id: String,
        payload: String,
    },
}

async fn context_runs_queued_frame_event(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    frame: ContextRunsQueuedFrame,
    #[cfg(test)] progress: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> Option<Event> {
    match frame {
        ContextRunsQueuedFrame::Ready {
            workspace,
            subscribed_run_ids,
            timestamp_ms,
        } => {
            super::context_run_authority::with_current_context_run_reads(
                state,
                tenant,
                verified,
                &subscribed_run_ids,
                |current_ids| {
                    Some(
                        Event::default().data(
                            serde_json::to_string(&json!({
                                "kind": "ready",
                                "workspace": workspace,
                                "subscribed_run_ids": current_ids,
                                "timestamp_ms": timestamp_ms,
                            }))
                            .unwrap_or_default(),
                        ),
                    )
                },
                #[cfg(test)]
                progress,
            )
            .await
        }
        ContextRunsQueuedFrame::Envelope { run_id, payload } => {
            super::context_run_authority::with_current_context_run_reads(
                state,
                tenant,
                verified,
                &[run_id],
                |current_ids| (!current_ids.is_empty()).then(|| Event::default().data(payload)),
                #[cfg(test)]
                progress,
            )
            .await
        }
    }
}

struct ContextRunsDequeueState {
    receiver: tokio::sync::mpsc::Receiver<ContextRunsQueuedFrame>,
    state: AppState,
    tenant: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
    #[cfg(test)]
    progress: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

pub(super) fn context_runs_multiplex_dequeue_stream(
    state: AppState,
    tenant: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
    receiver: tokio::sync::mpsc::Receiver<ContextRunsQueuedFrame>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    context_runs_multiplex_dequeue(ContextRunsDequeueState {
        receiver,
        state,
        tenant,
        verified,
        #[cfg(test)]
        progress: None,
    })
}

// Per-stream progress makes a real native lookup wait deterministic without
// global hooks, sleep-based scheduling, or changing production authority.
#[cfg(test)]
pub(super) fn context_runs_multiplex_dequeue_stream_observed(
    state: AppState,
    tenant: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
    receiver: tokio::sync::mpsc::Receiver<ContextRunsQueuedFrame>,
    progress: tokio::sync::mpsc::UnboundedSender<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    context_runs_multiplex_dequeue(ContextRunsDequeueState {
        receiver,
        state,
        tenant,
        verified,
        progress: Some(progress),
    })
}

fn context_runs_multiplex_dequeue(
    queued: ContextRunsDequeueState,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    futures::stream::unfold(queued, |mut queued| async move {
        loop {
            let frame = queued.receiver.recv().await?;
            if let Some(event) = context_runs_queued_frame_event(
                &queued.state,
                &queued.tenant,
                queued.verified.as_ref(),
                frame,
                #[cfg(test)]
                queued.progress.as_ref(),
            )
            .await
            {
                return Some((Ok(event), queued));
            }
        }
    })
}

pub(super) fn context_runs_multiplex_frame_receiver(
    state: AppState,
    tenant_context: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
    workspace: String,
    subscribed_run_ids: Vec<String>,
    cursor: ContextRunsStreamCursor,
    tail: Option<usize>,
) -> tokio::sync::mpsc::Receiver<ContextRunsQueuedFrame> {
    let (tx, rx) = tokio::sync::mpsc::channel::<ContextRunsQueuedFrame>(512);
    tokio::spawn(async move {
        let mut current_ids = Vec::new();
        for run_id in subscribed_run_ids {
            if super::context_run_authority::run_stream_resource_visible(
                &state,
                &tenant_context,
                verified.as_ref(),
                &super::context_run_authority::RunStreamResource::ContextRun(run_id.clone()),
            )
            .await
            {
                current_ids.push(run_id);
            }
        }
        let subscribed_set: HashSet<String> = current_ids.iter().cloned().collect();
        if tx
            .send(ContextRunsQueuedFrame::Ready {
                workspace: workspace.clone(),
                subscribed_run_ids: current_ids,
                timestamp_ms: crate::now_ms(),
            })
            .await
            .is_err()
        {
            return;
        }

        let mut replay = Vec::<ContextRunsStreamEnvelope>::new();
        for run_id in &subscribed_set {
            if !super::context_run_authority::run_stream_resource_visible(
                &state,
                &tenant_context,
                verified.as_ref(),
                &super::context_run_authority::RunStreamResource::ContextRun(run_id.clone()),
            )
            .await
            {
                continue;
            }
            let run_events = load_context_run_events_jsonl(
                &context_run_events_path(&state, run_id),
                cursor.events.get(run_id).copied(),
                if cursor.events.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for row in run_events {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "context_run_event".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: row.seq,
                    ts_ms: row.ts_ms,
                    payload: serde_json::to_value(row).unwrap_or_else(|_| json!({})),
                });
            }
            let run_patches = load_context_blackboard_patches(
                &state,
                run_id,
                cursor.patches.get(run_id).copied(),
                if cursor.patches.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for patch in run_patches {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "blackboard_patch".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: patch.seq,
                    ts_ms: patch.ts_ms,
                    payload: serde_json::to_value(patch).unwrap_or_else(|_| json!({})),
                });
            }
        }
        replay.sort_by(|a, b| {
            a.ts_ms
                .cmp(&b.ts_ms)
                .then_with(|| a.run_id.cmp(&b.run_id))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.seq.cmp(&b.seq))
        });
        for row in replay {
            if !super::context_run_authority::run_stream_resource_visible(
                &state,
                &tenant_context,
                verified.as_ref(),
                &super::context_run_authority::RunStreamResource::ContextRun(row.run_id.clone()),
            )
            .await
            {
                continue;
            }
            let payload = serde_json::to_string(&row).unwrap_or_default();
            if tx
                .send(ContextRunsQueuedFrame::Envelope {
                    run_id: row.run_id,
                    payload,
                })
                .await
                .is_err()
            {
                return;
            }
        }

        let mut live = state.event_bus.subscribe();
        loop {
            match live.recv().await {
                Ok(event) => {
                    if event.event_type != "context.run.stream" {
                        continue;
                    }
                    let run_id = event
                        .properties
                        .get("run_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if run_id.is_empty() || !subscribed_set.contains(run_id) {
                        continue;
                    }
                    let event_workspace = event
                        .properties
                        .get("workspace")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if event_workspace != workspace {
                        continue;
                    }
                    if !super::context_run_authority::run_stream_resource_visible(
                        &state,
                        &tenant_context,
                        verified.as_ref(),
                        &super::context_run_authority::RunStreamResource::ContextRun(
                            run_id.to_owned(),
                        ),
                    )
                    .await
                    {
                        continue;
                    }
                    let payload = serde_json::to_string(&event.properties).unwrap_or_default();
                    if tx
                        .send(ContextRunsQueuedFrame::Envelope {
                            run_id: run_id.to_owned(),
                            payload,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    rx
}

pub(super) fn context_runs_events_multiplex_sse_stream(
    state: AppState,
    tenant_context: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
    workspace: String,
    subscribed_run_ids: Vec<String>,
    cursor: ContextRunsStreamCursor,
    tail: Option<usize>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let receiver = context_runs_multiplex_frame_receiver(
        state.clone(),
        tenant_context.clone(),
        verified.clone(),
        workspace,
        subscribed_run_ids,
        cursor,
        tail,
    );
    context_runs_multiplex_dequeue_stream(state, tenant_context, verified, receiver)
}
