// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) fn context_driver_select_next_step(
    run: &ContextRunState,
) -> (Option<usize>, String, ContextRunStatus) {
    if context_run_is_terminal(&run.status) {
        return (
            None,
            format!(
                "run is terminal (`{}`); no next step can be selected",
                serde_json::to_string(&run.status).unwrap_or_else(|_| "\"terminal\"".to_string())
            ),
            run.status.clone(),
        );
    }
    if let Some(step) = run
        .steps
        .iter()
        .find(|step| matches!(step.status, ContextStepStatus::InProgress))
    {
        return (
            None,
            format!(
                "step `{}` is already in_progress; keep current execution focus",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Runnable))
    {
        return (
            Some(idx),
            format!(
                "selected runnable step `{}` as next execution target",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Pending))
    {
        return (
            Some(idx),
            format!(
                "no runnable step available; promoted pending step `{}` for execution",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if !run.steps.is_empty()
        && run
            .steps
            .iter()
            .all(|step| matches!(step.status, ContextStepStatus::Done))
    {
        return (
            None,
            "all steps are done; marking run completed".to_string(),
            ContextRunStatus::Completed,
        );
    }
    if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::Failed))
    {
        return (
            None,
            "one or more steps failed and no runnable work remains; run is blocked".to_string(),
            ContextRunStatus::Blocked,
        );
    }
    (
        None,
        "no actionable steps found; run remains blocked".to_string(),
        ContextRunStatus::Blocked,
    )
}
