use axum::routing::{get, post};
use axum::Router;

use super::context_packs::*;
use super::context_run_ledger::*;
use super::context_run_mutation_checkpoints::*;
use super::context_runs::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/context/runs",
            post(context_run_create).get(context_run_list),
        )
        .route(
            "/context/runs/events/stream",
            get(context_runs_events_stream),
        )
        .route(
            "/context/packs",
            get(context_pack_list).post(context_pack_publish),
        )
        .route("/context/packs/{pack_id}", get(context_pack_get))
        .route("/context/packs/{pack_id}/bind", post(context_pack_bind))
        .route("/context/packs/{pack_id}/revoke", post(context_pack_revoke))
        .route(
            "/context/packs/{pack_id}/supersede",
            post(context_pack_supersede),
        )
        .route(
            "/context/runs/{run_id}",
            get(context_run_get).put(context_run_put),
        )
        .route(
            "/context/runs/{run_id}/events",
            get(context_run_events).post(context_run_event_append),
        )
        .route("/context/runs/{run_id}/ledger", get(context_run_ledger))
        .route(
            "/context/runs/{run_id}/governance-evidence",
            get(context_run_governance_evidence_export),
        )
        .route(
            "/context/runs/{run_id}/todos/sync",
            post(context_run_todos_sync),
        )
        .route(
            "/context/runs/{run_id}/events/stream",
            get(context_run_events_stream),
        )
        .route(
            "/context/runs/{run_id}/lease/validate",
            post(context_run_lease_validate),
        )
        .route(
            "/context/runs/{run_id}/blackboard",
            get(context_run_blackboard_get),
        )
        .route(
            "/context/runs/{run_id}/blackboard/patches",
            get(context_run_blackboard_patches_get).post(context_run_blackboard_patch),
        )
        .route(
            "/context/runs/{run_id}/tasks",
            post(context_run_tasks_create),
        )
        .route(
            "/context/runs/{run_id}/tasks/claim",
            post(context_run_tasks_claim),
        )
        .route(
            "/context/runs/{run_id}/tasks/{task_id}/transition",
            post(context_run_task_transition),
        )
        .route(
            "/context/runs/{run_id}/checkpoints",
            post(context_run_checkpoint_create),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/mutations",
            get(context_run_mutation_checkpoints),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/mutations/rollback-preview",
            get(context_run_mutation_checkpoint_rollback_preview),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/mutations/rollback-history",
            get(context_run_mutation_checkpoint_rollback_history),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/mutations/rollback-execute",
            post(context_run_mutation_checkpoint_rollback_execute),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/latest",
            get(context_run_checkpoint_latest),
        )
        .route("/context/runs/{run_id}/replay", get(context_run_replay))
        .route(
            "/context/runs/{run_id}/driver/next",
            post(context_run_driver_next),
        )
}
