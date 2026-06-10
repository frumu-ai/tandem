use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Stable workflow identity used across meta-harness evaluations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowId(String);

impl WorkflowId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for WorkflowId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for WorkflowId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Stable version identity for one workflow candidate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionId(String);

impl VersionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for VersionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for VersionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Named score dimension, such as accuracy, latency, or cost.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScoreDimension(String);

impl ScoreDimension {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ScoreDimension {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ScoreDimension {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Finite score value for deterministic comparison and serialization.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Serialize)]
pub struct ScoreValue(f64);

impl ScoreValue {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ScoreValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| D::Error::custom("score values must be finite"))
    }
}

impl Eq for ScoreValue {}

impl Ord for ScoreValue {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .partial_cmp(&other.0)
            .expect("ScoreValue can only contain finite floating point values")
    }
}

/// Scores for a workflow/version pair, ordered by workflow, score, version, then dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoredWorkflowVersion {
    pub workflow_id: WorkflowId,
    pub version_id: VersionId,
    pub aggregate_score: ScoreValue,
    #[serde(default)]
    pub dimensions: BTreeMap<ScoreDimension, ScoreValue>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl ScoredWorkflowVersion {
    pub fn new(
        workflow_id: impl Into<WorkflowId>,
        version_id: impl Into<VersionId>,
        aggregate_score: ScoreValue,
    ) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            version_id: version_id.into(),
            aggregate_score,
            dimensions: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_dimension(
        mut self,
        dimension: impl Into<ScoreDimension>,
        value: ScoreValue,
    ) -> Self {
        self.dimensions.insert(dimension.into(), value);
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl Ord for ScoredWorkflowVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.workflow_id
            .cmp(&other.workflow_id)
            .then_with(|| self.aggregate_score.cmp(&other.aggregate_score))
            .then_with(|| self.version_id.cmp(&other.version_id))
            .then_with(|| self.dimensions.cmp(&other.dimensions))
            .then_with(|| self.metadata.cmp(&other.metadata))
    }
}

impl PartialOrd for ScoredWorkflowVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::{value::Error, IntoDeserializer};

    #[test]
    fn score_value_deserialization_rejects_non_finite_values() {
        let finite: Result<ScoreValue, Error> = ScoreValue::deserialize(0.42.into_deserializer());
        assert_eq!(finite.expect("finite score deserializes").get(), 0.42);

        let nan: Result<ScoreValue, Error> = ScoreValue::deserialize(f64::NAN.into_deserializer());
        assert!(nan.is_err());

        let infinity: Result<ScoreValue, Error> =
            ScoreValue::deserialize(f64::INFINITY.into_deserializer());
        assert!(infinity.is_err());
    }
}
