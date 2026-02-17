use crate::{
    MissionCommand, MissionEvent, MissionSpec, MissionState, MissionStatus, WorkItem,
    WorkItemStatus,
};
use serde_json::json;

pub trait MissionReducer {
    fn init(spec: MissionSpec) -> MissionState;
    fn on_event(state: &MissionState, event: MissionEvent) -> Vec<MissionCommand>;
}

pub struct NoopMissionReducer;
pub struct DefaultMissionReducer;

impl MissionReducer for NoopMissionReducer {
    fn init(spec: MissionSpec) -> MissionState {
        MissionState {
            mission_id: spec.mission_id.clone(),
            status: MissionStatus::Draft,
            spec,
            work_items: Vec::new(),
            revision: 1,
            updated_at_ms: 0,
        }
    }

    fn on_event(_state: &MissionState, _event: MissionEvent) -> Vec<MissionCommand> {
        Vec::new()
    }
}

impl DefaultMissionReducer {
    pub fn reduce(
        state: &MissionState,
        event: MissionEvent,
    ) -> (MissionState, Vec<MissionCommand>) {
        let mut next = state.clone();
        let mut commands = Vec::new();
        let mut changed = false;

        match event {
            MissionEvent::MissionStarted { mission_id } if mission_id == next.mission_id => {
                if next.status != MissionStatus::Running {
                    next.status = MissionStatus::Running;
                    changed = true;
                }
            }
            MissionEvent::RunStarted {
                mission_id,
                work_item_id,
                run_id,
            } if mission_id == next.mission_id => {
                if let Some(item) = get_work_item_mut(&mut next.work_items, &work_item_id) {
                    item.status = WorkItemStatus::InProgress;
                    item.run_id = Some(run_id);
                    changed = true;
                }
            }
            MissionEvent::RunFinished {
                mission_id,
                work_item_id,
                status,
                ..
            } if mission_id == next.mission_id => {
                if let Some(item) = get_work_item_mut(&mut next.work_items, &work_item_id) {
                    if is_success_status(&status) {
                        item.status = WorkItemStatus::Review;
                        commands.push(MissionCommand::RequestApproval {
                            mission_id: next.mission_id.clone(),
                            work_item_id: work_item_id.clone(),
                            kind: "review".to_string(),
                            summary: format!("Review required for `{}` output", item.title),
                        });
                    } else {
                        item.status = WorkItemStatus::Rework;
                        commands.push(MissionCommand::EmitNotice {
                            mission_id: next.mission_id.clone(),
                            event_type: "mission.work_item.rework_requested".to_string(),
                            properties: json!({
                                "workItemID": work_item_id,
                                "reason": "run_failed",
                                "runStatus": status,
                            }),
                        });
                    }
                    changed = true;
                }
            }
            MissionEvent::ApprovalGranted {
                mission_id,
                work_item_id,
                approval_id,
            } if mission_id == next.mission_id => {
                if let Some(item) = get_work_item_mut(&mut next.work_items, &work_item_id) {
                    match item.status {
                        WorkItemStatus::Review => {
                            item.status = WorkItemStatus::Test;
                            commands.push(MissionCommand::RequestApproval {
                                mission_id: next.mission_id.clone(),
                                work_item_id: work_item_id.clone(),
                                kind: "test".to_string(),
                                summary: format!(
                                    "Tester validation required for `{}` after review {}",
                                    item.title, approval_id
                                ),
                            });
                            changed = true;
                        }
                        WorkItemStatus::Test => {
                            item.status = WorkItemStatus::Done;
                            commands.push(MissionCommand::EmitNotice {
                                mission_id: next.mission_id.clone(),
                                event_type: "mission.work_item.completed".to_string(),
                                properties: json!({
                                    "workItemID": work_item_id,
                                    "approvalID": approval_id,
                                }),
                            });
                            if next
                                .work_items
                                .iter()
                                .all(|candidate| candidate.status == WorkItemStatus::Done)
                            {
                                next.status = MissionStatus::Succeeded;
                                commands.push(MissionCommand::EmitNotice {
                                    mission_id: next.mission_id.clone(),
                                    event_type: "mission.completed".to_string(),
                                    properties: json!({
                                        "missionID": next.mission_id,
                                    }),
                                });
                            }
                            changed = true;
                        }
                        _ => {}
                    }
                }
            }
            MissionEvent::ApprovalDenied {
                mission_id,
                work_item_id,
                reason,
                ..
            } if mission_id == next.mission_id => {
                if let Some(item) = get_work_item_mut(&mut next.work_items, &work_item_id) {
                    let gate = match item.status {
                        WorkItemStatus::Review => Some("review"),
                        WorkItemStatus::Test => Some("test"),
                        _ => None,
                    };
                    if let Some(gate) = gate {
                        item.status = WorkItemStatus::Rework;
                        commands.push(MissionCommand::EmitNotice {
                            mission_id: next.mission_id.clone(),
                            event_type: "mission.work_item.rework_requested".to_string(),
                            properties: json!({
                                "workItemID": work_item_id,
                                "gate": gate,
                                "reason": reason,
                            }),
                        });
                        changed = true;
                    }
                }
            }
            _ => {}
        }

        if changed {
            next.revision = next.revision.saturating_add(1);
        }
        (next, commands)
    }
}

fn get_work_item_mut<'a>(
    items: &'a mut [WorkItem],
    work_item_id: &str,
) -> Option<&'a mut WorkItem> {
    items
        .iter_mut()
        .find(|item| item.work_item_id == work_item_id)
}

fn is_success_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "ok" | "success" | "passed"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkItemStatus;

    fn base_state() -> MissionState {
        let spec = MissionSpec::new("Default flow", "Ship with gates");
        MissionState {
            mission_id: spec.mission_id.clone(),
            status: MissionStatus::Running,
            spec,
            work_items: vec![WorkItem {
                work_item_id: "w-1".to_string(),
                title: "Implement patch".to_string(),
                detail: None,
                status: WorkItemStatus::Review,
                depends_on: Vec::new(),
                assigned_agent: None,
                run_id: Some("r-1".to_string()),
                artifact_refs: Vec::new(),
                metadata: None,
            }],
            revision: 1,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn noop_reducer_initializes_mission_state() {
        let spec = MissionSpec::new("Mission", "Ship the first scaffold");
        let mission_id = spec.mission_id.clone();
        let state = NoopMissionReducer::init(spec);
        assert_eq!(state.mission_id, mission_id);
        assert!(matches!(state.status, MissionStatus::Draft));
        assert!(state.work_items.is_empty());
    }

    #[test]
    fn noop_reducer_emits_no_commands() {
        let spec = MissionSpec::new("Mission", "Noop");
        let state = NoopMissionReducer::init(spec.clone());
        let commands = NoopMissionReducer::on_event(
            &state,
            MissionEvent::MissionStarted {
                mission_id: spec.mission_id,
            },
        );
        assert!(commands.is_empty());
    }

    #[test]
    fn reviewer_denial_sends_item_to_rework() {
        let state = base_state();
        let (next, commands) = DefaultMissionReducer::reduce(
            &state,
            MissionEvent::ApprovalDenied {
                mission_id: state.mission_id.clone(),
                work_item_id: "w-1".to_string(),
                approval_id: "appr-1".to_string(),
                reason: "missing edge-case test".to_string(),
            },
        );
        assert_eq!(next.work_items[0].status, WorkItemStatus::Rework);
        assert_eq!(next.revision, 2);
        assert!(commands
            .iter()
            .any(|command| matches!(command, MissionCommand::EmitNotice { event_type, .. } if event_type == "mission.work_item.rework_requested")));
    }

    #[test]
    fn reviewer_approval_advances_to_test_gate() {
        let state = base_state();
        let (next, commands) = DefaultMissionReducer::reduce(
            &state,
            MissionEvent::ApprovalGranted {
                mission_id: state.mission_id.clone(),
                work_item_id: "w-1".to_string(),
                approval_id: "review-1".to_string(),
            },
        );
        assert_eq!(next.work_items[0].status, WorkItemStatus::Test);
        assert!(commands.iter().any(|command| {
            matches!(
                command,
                MissionCommand::RequestApproval { kind, .. } if kind == "test"
            )
        }));
    }

    #[test]
    fn tester_approval_marks_done_and_mission_complete() {
        let mut state = base_state();
        state.work_items[0].status = WorkItemStatus::Test;
        let (next, commands) = DefaultMissionReducer::reduce(
            &state,
            MissionEvent::ApprovalGranted {
                mission_id: state.mission_id.clone(),
                work_item_id: "w-1".to_string(),
                approval_id: "test-1".to_string(),
            },
        );
        assert_eq!(next.work_items[0].status, WorkItemStatus::Done);
        assert_eq!(next.status, MissionStatus::Succeeded);
        assert!(commands.iter().any(|command| {
            matches!(
                command,
                MissionCommand::EmitNotice { event_type, .. } if event_type == "mission.completed"
            )
        }));
    }
}
