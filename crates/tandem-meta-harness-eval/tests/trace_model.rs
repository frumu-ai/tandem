use std::collections::BTreeMap;

use tandem_meta_harness_eval::{
    Trace, TraceEvent, TraceEventId, TraceMetadata, TraceStep, TraceStepId,
};

#[test]
fn trace_model_serializes_deserializes_and_replays_in_sequence_order() {
    let mut trace = Trace::new("trace-mh-01", TraceMetadata::new("workflow-alpha", "v1"))
        .with_metadata("source", "integration-test");

    trace.push_step(TraceStep::new(
        TraceStepId::new("step-plan"),
        10,
        "plan",
        BTreeMap::from([("prompt".to_string(), "define trace model".to_string())]),
    ));
    trace.push_event(TraceEvent::new(
        TraceEventId::new("event-plan-finished"),
        11,
        "step-plan",
        "finished",
        BTreeMap::from([("status".to_string(), "ok".to_string())]),
    ));
    trace.push_step(TraceStep::new(
        TraceStepId::new("step-score"),
        20,
        "score",
        BTreeMap::from([("dimension".to_string(), "quality".to_string())]),
    ));

    let serialized = serde_json::to_string_pretty(&trace).expect("trace serializes");
    assert!(serialized.contains("trace-mh-01"));
    assert!(serialized.contains("sequence"));

    let decoded: Trace = serde_json::from_str(&serialized).expect("trace deserializes");
    let replayed: Vec<_> = decoded.replay().map(|entry| entry.sequence()).collect();

    assert_eq!(replayed, vec![10, 11, 20]);
    assert_eq!(
        decoded.steps().map(|step| step.id.as_str()).collect::<Vec<_>>(),
        vec!["step-plan", "step-score"]
    );
    assert_eq!(
        decoded.events().map(|event| event.id.as_str()).collect::<Vec<_>>(),
        vec!["event-plan-finished"]
    );
}
