// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::missions_teams::*;
use crate::http::AppState;
use axum::{
    routing::{get, patch, post},
    Router,
};

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/mission", get(mission_list).post(mission_create))
        .route("/mission/{id}", get(mission_get))
        .route("/mission/{id}/event", post(mission_apply_event))
        .route(
            "/agent-team/templates",
            get(agent_team_templates).post(agent_team_template_create),
        )
        .route(
            "/agent-team/templates/{id}",
            patch(agent_team_template_patch).delete(agent_team_template_delete),
        )
        .route("/automations/compose/standup", post(compose_standup))
        .route("/automations/compose/monitor", post(compose_monitor))
        // Legacy alias — kept for backwards compat.
        .route("/agent-standup/compose", post(compose_standup))
        .route("/agent-team/instances", get(agent_team_instances))
        .route("/agent-team/missions", get(agent_team_missions))
        .route("/agent-team/approvals", get(agent_team_approvals))
        .route(
            "/agent-team/approvals/spawn/{id}/approve",
            post(agent_team_approve_spawn),
        )
        .route(
            "/agent-team/approvals/spawn/{id}/deny",
            post(agent_team_deny_spawn),
        )
        .route("/agent-team/spawn", post(agent_team_spawn))
        .route(
            "/agent-team/instance/{id}/cancel",
            post(agent_team_cancel_instance),
        )
        .route(
            "/agent-team/mission/{id}/cancel",
            post(agent_team_cancel_mission),
        )
}
