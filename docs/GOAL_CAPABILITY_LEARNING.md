# Goal Capability Learning (GCL)

**Status:** Design (TAN-41 / GCL-01)  
**Date:** 2026-06-05

## Product Language and Distinctions

### Goal Capability Learning vs. Workflow Learning

**Workflow Learning (existing):** Analyzes past automation/workflow execution data to *repair and improve existing workflows*. It operates on completed runs, examining validation failures, repair attempts, forensic receipts, and timelines to understand why a workflow failed and how to fix it.

**Goal Capability Learning (GCL, new):** Discovers and composes *new capabilities to reach specified goals*. It operates on a declarative goal specification, identifies capability requirements, searches available capabilities (tools, sub-automations, connectors), and composes them into executable workflows that were not previously known or available.

### Key Distinctions

| Aspect | Workflow Learning | Goal Capability Learning |
|--------|-------------------|------------------------|
| **Input** | Execution traces, validator failures, repair history | Goal specification: "I want to..." |
| **Purpose** | Fix broken; improve existing | Discover new; compose novel |
| **Output** | Patched workflow, repair strategy | New workflow composition, capability report |
| **Trigger** | Automation execution; failure observed | User request for new capability; goal stated |
| **Scope** | Single workflow, execution context | Multi-step composition; tool/connector discovery |

### Workflow Composition vs. Repair

- **Workflow Repair** (existing): "The PDF converter failed; let's try a different parser."
- **Workflow Composition** (GCL): "I need to convert a PDF to JSON; what tools/connectors do I need to chain?"

## Demo Goal: "Read and parse a CSV file"

**Selected Goal:** Given a CSV file path, read its contents and parse it into structured records (rows as objects).

**Rationale for Demo Goal:**
1. **Minimal scope:** Single source (file read), no external APIs, no multi-step orchestration complexity
2. **Clear capability chain:** Requires file read capability → CSV parser capability
3. **Testable:** Output is deterministic; can validate against sample CSV
4. **Extensible:** Future goals can build on file I/O and data transformation patterns
5. **No hidden complexity:** No auth, no state management, no approval gates needed for MVP

**Expected Workflow:**
```
Goal: "Read and parse CSV from /tmp/data.csv"
  ├─ Capability: FileRead(path: "/tmp/data.csv")
  ├─ Capability: CSVParse(content: <file content>, format: "records")
  └─ Output: [{"col1": "val1", "col2": "val2"}, ...]
```

## Design Constraints for First Slice (TAN-41 / TAN-42)

1. **Narrow discovery:** Start with hardcoded discovery (no ML-based search). Map known tools/capabilities.
2. **Linear composition:** Support sequential capability chains only (no branching, no conditionals in MVP).
3. **No runtime learning:** GCL is about pre-runtime composition, not runtime repair. Separate from Workflow Learning.
4. **Declarative goals:** GoalSpec is human-readable; capability requirements are explicit.
5. **Audit trail:** Record which capabilities were selected, why, and which were rejected (for future learning).

## Data Structures (TAN-42 prep)

### GoalSpec
Declarative specification of a desired outcome.

```rust
pub struct GoalSpec {
    pub goal_id: String,
    pub title: String,
    pub description: String,
    pub input_parameters: Vec<GoalParameter>,
    pub expected_output_format: String, // JSON schema or description
    pub constraints: Vec<GoalConstraint>, // e.g., "no external APIs", "must complete in <5s"
}

pub struct GoalParameter {
    pub name: String,
    pub data_type: String,
    pub description: String,
}

pub enum GoalConstraint {
    NoExternalApis,
    MaxExecutionTimeMs(u64),
    RequiredDataClasses(Vec<DataClass>),
    ForbiddenTools(Vec<String>),
}
```

### CapabilityDiscoveryReport
Result of analyzing available capabilities against a goal.

```rust
pub struct CapabilityDiscoveryReport {
    pub goal_id: String,
    pub discovered_capabilities: Vec<AvailableCapability>,
    pub composition_candidates: Vec<CompositionPath>,
    pub gaps: Vec<CapabilityGap>,
    pub confidence_score: f64,
    pub reasoning: String,
}

pub struct AvailableCapability {
    pub capability_id: String,
    pub tool_name: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub tags: Vec<String>, // e.g., ["file_io", "data_transform"]
}

pub struct CompositionPath {
    pub sequence: Vec<String>, // ordered capability_ids
    pub compatibility_score: f64,
    pub estimated_duration_ms: u64,
}

pub enum CapabilityGap {
    NoCapabilityFound(String), // description of missing capability
    CapabilityNotAuthorized(String), // tool exists but not accessible
    CapabilityRejectedByConstraint(String, String), // (capability, constraint reason)
}
```

### GoalCapabilityLearningRequest
Request to discover capabilities for a goal.

```rust
pub struct GoalCapabilityLearningRequest {
    pub goal: GoalSpec,
    pub max_candidates: usize,
    pub available_tools: Vec<ToolDescriptor>, // scoped to tenant
}

pub struct GoalCapabilityLearningResponse {
    pub request_id: String,
    pub report: CapabilityDiscoveryReport,
    pub primary_recommendation: Option<CompositionPath>,
    pub alternatives: Vec<CompositionPath>,
}
```

## First Implementation (TAN-41 scope)

1. **Define GoalSpec, CapabilityDiscoveryReport** in tandem-types crate
2. **Hardcode the demo goal** ("read and parse CSV")
3. **Hardcode available capabilities** (list of known tools with their input/output)
4. **Implement capability matching** (which tools satisfy "file read" and "CSV parse" requirements)
5. **Generate composition path** (ordered sequence of capabilities)
6. **Record discovery decision** (similar to PolicyDecisionRecord, for audit trail)

## Second Implementation (TAN-42 scope)

1. **Extend GoalSpec** with more complex goals (branching, conditionals, loops)
2. **Improve capability discovery** (rule-based tag matching instead of hardcoding)
3. **Composition validation** (check compatibility of output → input schemas)
4. **Integration with automation runtime** (convert CompositionPath → AutomationV2Spec)

## Relationship to Workflow Learning

- **No overlap:** GCL discovers new workflows; Workflow Learning improves existing ones
- **Complementary:** Once a GCL-discovered workflow runs and fails, Workflow Learning may repair it
- **Separate audit streams:** GCL records in `goal_capability_learning_decisions` table; Workflow Learning in existing `policy_decision_records`

## Success Criteria (TAN-41) — COMPLETE ✅

- [x] GoalSpec, CapabilityDiscoveryReport types defined and serializable
- [x] Demo goal ("read and parse CSV") has a working composition path
- [x] Hardcoded capability matcher can identify file-read and CSV-parse tools
- [x] Composition path generates correct sequence (file read → CSV parse)
- [x] Discovery decision is audited (can replay what decision was made and why)
- [x] All tests pass; codebase linted
- [x] Product language (GCL vs. Workflow Learning) documented and reviewed

## Implementation (TAN-41 & TAN-42) — COMPLETE ✅

**TAN-41 (Completed):**
- Defined GoalSpec, CapabilityDiscoveryReport, CompositionPath, CapabilityGap types in tandem-types
- Implemented 7 tests covering structure, composition, serialization, gap variants
- Created design doc establishing product language and distinctions

**TAN-42 (Completed):**
- Implemented CapabilityMatcher: keyword-based discovery (file_read: "read|file|open|load"; csv_parse: "csv|parse")
- Defined hardcoded capability fixtures: FileRead, CSVParse, JSONSerialize with JSON schemas and tags
- Implemented discover_capabilities_for_goal(): matches goals to capabilities, generates composition paths
- Generated audit IDs: gcl_<uuid> format for discovery decision trails
- Added 10 tests covering discovery, fixture validation, compatibility, ID generation
- All tests passing; codebase formatted

**Demo CSV Goal Flow:**
```
GoalSpec(title: "Read and parse CSV file")
  → CapabilityMatcher detects "read" + "csv" keywords
  → Discovered: FileRead (file_io, read tags) + CSVParse (csv, parse tags)
  → CompositionPath: [file_read → csv_parse] (0.95 confidence)
  → CapabilityDiscoveryReport with primary_recommendation()
```

## Next Steps (TAN-43+)

1. **Server Integration (TAN-43):** Wire discover_capabilities_for_goal into runtime state
2. **Decision Persistence:** Store discovery decisions in goal_capability_learning_decisions table
3. **API Exposure:** Create REST/gRPC endpoints for goal discovery requests
4. **Composition Validation:** Verify output schema of step N matches input schema of step N+1
5. **Composition Execution:** Convert CompositionPath → AutomationV2Spec and execute
