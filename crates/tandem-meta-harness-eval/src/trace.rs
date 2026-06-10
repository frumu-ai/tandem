use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Stable identifier for a durable trace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceId(String);

impl TraceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TraceId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for TraceId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Stable identifier for a replayable trace step.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceStepId(String);

impl TraceStepId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TraceStepId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for TraceStepId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Stable identifier for a replayable trace event.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceEventId(String);

impl TraceEventId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for TraceEventId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for TraceEventId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Trace-level metadata required to bind replay data to a workflow version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub workflow_id: String,
    pub version_id: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

impl TraceMetadata {
    pub fn new(workflow_id: impl Into<String>, version_id: impl Into<String>) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            version_id: version_id.into(),
            attributes: BTreeMap::new(),
        }
    }
}

/// A durable replay step ordered by a monotonically increasing sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceStep {
    pub id: TraceStepId,
    pub sequence: u64,
    pub name: String,
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

impl TraceStep {
    pub fn new(
        id: impl Into<TraceStepId>,
        sequence: u64,
        name: impl Into<String>,
        payload: BTreeMap<String, String>,
    ) -> Self {
        Self {
            id: id.into(),
            sequence,
            name: name.into(),
            payload,
        }
    }
}

/// A durable replay event ordered by a monotonically increasing sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub id: TraceEventId,
    pub sequence: u64,
    pub step_id: TraceStepId,
    pub kind: String,
    #[serde(default)]
    pub payload: BTreeMap<String, String>,
}

impl TraceEvent {
    pub fn new(
        id: impl Into<TraceEventId>,
        sequence: u64,
        step_id: impl Into<TraceStepId>,
        kind: impl Into<String>,
        payload: BTreeMap<String, String>,
    ) -> Self {
        Self {
            id: id.into(),
            sequence,
            step_id: step_id.into(),
            kind: kind.into(),
            payload,
        }
    }
}

/// A deterministic replay item yielded from a [`Trace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceReplayEntry<'a> {
    Step(&'a TraceStep),
    Event(&'a TraceEvent),
}

impl TraceReplayEntry<'_> {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Step(step) => step.sequence,
            Self::Event(event) => event.sequence,
        }
    }
}

/// Durable trace capture for later deterministic meta-harness replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    pub id: TraceId,
    pub metadata: TraceMetadata,
    #[serde(default)]
    pub steps: Vec<TraceStep>,
    #[serde(default)]
    pub events: Vec<TraceEvent>,
}

impl Trace {
    pub fn new(id: impl Into<TraceId>, metadata: TraceMetadata) -> Self {
        Self {
            id: id.into(),
            metadata,
            steps: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.attributes.insert(key.into(), value.into());
        self
    }

    pub fn push_step(&mut self, step: TraceStep) {
        self.steps.push(step);
        self.steps.sort_by_key(|step| step.sequence);
    }

    pub fn push_event(&mut self, event: TraceEvent) {
        self.events.push(event);
        self.events.sort_by_key(|event| event.sequence);
    }

    pub fn steps(&self) -> impl Iterator<Item = &TraceStep> {
        self.steps.iter()
    }

    pub fn events(&self) -> impl Iterator<Item = &TraceEvent> {
        self.events.iter()
    }

    pub fn replay(&self) -> impl Iterator<Item = TraceReplayEntry<'_>> {
        let mut entries: Vec<_> = self
            .steps
            .iter()
            .map(TraceReplayEntry::Step)
            .chain(self.events.iter().map(TraceReplayEntry::Event))
            .collect();
        entries.sort_by_key(|entry| entry.sequence());
        entries.into_iter()
    }
}
