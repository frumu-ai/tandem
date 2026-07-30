// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn schedule_request(prompt: &str) -> PlannerBuildRequest<Value> {
    prepare_build_request(
        "wfplan-bare-hour-timezone".to_string(),
        "v1".to_string(),
        "unit_test".to_string(),
        prompt,
        None,
        "UTC",
        Value::String("run_once".to_string()),
        Vec::new(),
        Some("/tmp/project"),
        None,
    )
}

#[test]
fn accepts_timezone_after_cadence_contextual_bare_hour() {
    for (prompt, timezone) in [
        ("Create a report every weekday at 9 ET", "America/New_York"),
        (
            "Create a report every weekday at 9 America/Los_Angeles",
            "America/Los_Angeles",
        ),
        (
            "Summarize ET markets every weekday at 9 PT",
            "America/Los_Angeles",
        ),
    ] {
        let request = schedule_request(prompt);
        assert_eq!(
            request.fallback_schedule.cron_expression.as_deref(),
            Some("0 9 * * Mon-Fri")
        );
        assert_eq!(request.fallback_schedule.timezone, timezone);
    }
}

#[test]
fn rejects_unrecognized_suffix_after_bare_hour() {
    let request = schedule_request("Create a report every weekday at 9 repositories");
    assert_eq!(
        request.fallback_schedule.schedule_type,
        AutomationV2ScheduleType::Manual
    );
    assert!(request.explicit_schedule.is_none());
}
