use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/skill", get(skill_list))
        .route("/skills", get(skills_list).post(skills_import))
        .route("/skills/catalog", get(skills_catalog))
        .route("/skills/import", post(skills_import))
        .route("/skills/import/preview", post(skills_import_preview))
        .route("/skills/validate", post(skills_validate))
        .route("/skills/router/match", post(skills_router_match))
        .route("/skills/compile", post(skills_compile))
        .route("/skills/generate", post(skills_generate))
        .route("/skills/generate/install", post(skills_generate_install))
        .route("/skills/evals/benchmark", post(skills_eval_benchmark))
        .route("/skills/evals/triggers", post(skills_eval_triggers))
        .route("/skills/templates", get(skills_templates_list))
        .route(
            "/skills/templates/{id}/install",
            post(skills_templates_install),
        )
        .route("/skills/{name}", get(skills_get).delete(skills_delete))
        .route("/memory/put", post(memory_put))
        .route("/memory/promote", post(memory_promote))
        .route("/memory/demote", post(memory_demote))
        .route("/memory/search", post(memory_search))
        .route("/memory/audit", get(memory_audit))
        .route("/memory", get(memory_list))
        .route("/memory/{id}", axum::routing::delete(memory_delete))
        .route("/memory/context/resolve", post(context_resolve_uri))
        .route("/memory/context/tree", get(context_tree))
        .route(
            "/memory/context/layers/generate",
            post(context_generate_layers),
        )
        .route("/memory/context/distill", post(context_distill))
        .route(
            "/workflow-learning/candidates",
            get(workflow_learning_candidates_list),
        )
        .route(
            "/workflow-learning/candidates/{candidate_id}/review",
            post(workflow_learning_candidate_review),
        )
        .route(
            "/workflow-learning/candidates/{candidate_id}/promote",
            post(workflow_learning_candidate_promote),
        )
        .route(
            "/workflow-learning/candidates/{candidate_id}/spawn-revision",
            post(workflow_learning_candidate_spawn_revision),
        )
}
