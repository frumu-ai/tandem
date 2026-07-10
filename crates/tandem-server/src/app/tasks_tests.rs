use super::*;

#[test]
fn incident_monitor_candidate_detection_includes_terminal_failures() {
    for event_type in [
        "context.task.failed",
        "context.task.blocked",
        "context.run.failed",
        "workflow.run.failed",
        "workflow.validation.failed",
        "routine.run.failed",
        "session.error",
        "automation.run.failed",
        "automation_v2.run.failed",
        "automation_v2.run.paused_stale_no_provider_activity",
        "coder.run.failed",
    ] {
        assert!(
            is_incident_monitor_candidate_event(&EngineEvent::new(
                event_type,
                serde_json::json!({})
            )),
            "{event_type} should be monitored"
        );
    }
}

#[test]
fn incident_monitor_candidate_detection_ignores_progress_and_monitor_events() {
    for event_type in [
        "context.task.started",
        "context.task.requeued",
        "workflow.action.completed",
        "automation_v2.run.started",
        "routine.run.completed",
        "incident_monitor.incident.detected",
    ] {
        assert!(
            !is_incident_monitor_candidate_event(&EngineEvent::new(
                event_type,
                serde_json::json!({})
            )),
            "{event_type} should not be monitored"
        );
    }
}

#[test]
fn incident_monitor_candidate_detection_ignores_automation_v2_context_mirror_failures() {
    for event in [
        EngineEvent::new(
            "context.task.failed",
            serde_json::json!({
                "source": "automation_v2",
                "automation_id": "automation-v2-123",
                "run_id": "automation-v2-run-123",
                "task_id": "node-downstream",
            }),
        ),
        EngineEvent::new(
            "context.task.blocked",
            serde_json::json!({
                "automationID": "automation-v2-123",
                "runID": "automation-v2-run-123",
                "taskID": "node-downstream",
            }),
        ),
        EngineEvent::new(
            "context.run.failed",
            serde_json::json!({
                "runID": "automation-v2-automation-v2-run-123",
            }),
        ),
    ] {
        assert!(
            !is_incident_monitor_candidate_event(&event),
            "{} from automation v2 context mirror should be grouped under automation_v2.run.failed",
            event.event_type
        );
    }
}

#[test]
fn incident_monitor_candidate_detection_keeps_standalone_context_failures() {
    assert!(is_incident_monitor_candidate_event(&EngineEvent::new(
        "context.task.failed",
        serde_json::json!({
            "source": "context_run",
            "run_id": "context-run-123",
            "task_id": "inspect_failure",
        }),
    )));
}
