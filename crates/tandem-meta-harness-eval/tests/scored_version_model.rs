use std::collections::BTreeSet;

use tandem_meta_harness_eval::{ScoreDimension, ScoreValue, ScoredWorkflowVersion};

#[test]
fn scored_versions_serialize_and_sort_deterministically_for_best_selection() {
    let baseline = ScoredWorkflowVersion::new(
        "workflow-alpha",
        "baseline",
        ScoreValue::new(0.72).expect("finite score"),
    )
    .with_dimension("accuracy", ScoreValue::new(0.80).unwrap())
    .with_dimension("cost", ScoreValue::new(0.40).unwrap())
    .with_metadata("trace", "trace-baseline");

    let candidate = ScoredWorkflowVersion::new(
        "workflow-alpha",
        "candidate",
        ScoreValue::new(0.91).expect("finite score"),
    )
    .with_dimension(ScoreDimension::new("accuracy"), ScoreValue::new(0.95).unwrap())
    .with_dimension("cost", ScoreValue::new(0.51).unwrap())
    .with_metadata("trace", "trace-candidate");

    let json = serde_json::to_string(&candidate).expect("score record serializes");
    let decoded: ScoredWorkflowVersion = serde_json::from_str(&json).expect("score record deserializes");
    assert_eq!(decoded.version_id.as_str(), "candidate");
    assert_eq!(decoded.aggregate_score.get(), 0.91);

    let ordered = BTreeSet::from([candidate, baseline]);
    let best = ordered.iter().next_back().expect("best scored version");

    assert_eq!(best.version_id.as_str(), "candidate");
    assert_eq!(best.aggregate_score.get(), 0.91);
}

#[test]
fn score_value_rejects_non_finite_values() {
    assert!(ScoreValue::new(f64::NAN).is_none());
    assert!(ScoreValue::new(f64::INFINITY).is_none());
}
