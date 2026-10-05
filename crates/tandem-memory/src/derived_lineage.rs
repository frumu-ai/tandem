//! Server-owned restrictions inherited by derived memories and learning rows.
//!
//! Serialization records provenance, not permission. A governed read also needs
//! an ephemeral proof from fresh canonical source reads and current grants.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tandem_enterprise_contract::{DataClass, ResourceKind, ResourceRef};
use tandem_types::MemorySourceReference;

use crate::knowledge_scope::{KnowledgeScopeDecision, KnowledgeScopePolicy};
use crate::{GovernedMemoryTier, MemoryPartition, PromotionReview};
use crate::types::{
    data_class_from_metadata, global_memory_record_resource_id, owner_org_unit_id_from_metadata,
    owner_subject_from_metadata, tenant_shared_from_metadata, GlobalMemoryRecord,
    GovernedReadDecision, GovernedReadEvidence, GovernedReadTarget, MemoryAccessFilter, MemoryError,
    MemoryResult, MemorySourceAccessTarget, MemoryTenantScope,
};

pub use crate::derived_lineage_store::resolve_derived_lineage;

pub const DERIVED_MEMORY_LINEAGE_METADATA_KEY: &str = "derived_memory_lineage";
pub const DERIVED_MEMORY_LINEAGE_SCHEMA_VERSION: u8 = 1;
pub const MAX_DERIVED_MEMORY_SOURCES: usize = 32;
pub const MAX_DERIVED_MEMORY_INPUTS: usize = 128;
pub(crate) const MAX_DERIVED_LINEAGE_DEPTH: usize = 8;
const MAX_LINEAGE_BYTES: usize = 131_072;

/// An embedding host resolves native session inputs through its own canonical
/// session repository before memory ranking can disclose a derived chunk.
pub type DerivedMemoryAccessResolver = Arc<dyn Fn(
    Arc<dyn crate::store::MemoryStore>, crate::store::MemoryReadScope,
    DerivedMemoryLineage, MemoryAccessFilter,
) -> Pin<Box<dyn Future<Output = Option<MemoryAccessFilter>> + Send>> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanonicalInputReference {
    SessionMessage { session_id: String, message_id: String, body_digest: String },
    Memory { source: MemorySourceReference },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedMemoryLineage {
    pub schema_version: u8,
    pub owner_subject: Option<String>,
    pub owner_org_unit_id: Option<String>,
    pub sources: Vec<CanonicalMemoryRestriction>,
    pub input_refs: Vec<CanonicalInputReference>,
}

/// A canonical source's semantic restrictions. No source plaintext is retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMemoryRestriction {
    pub memory_id: String,
    pub content_hash: String,
    pub restriction_digest: String,
    pub tenant_scope: MemoryTenantScope,
    pub target: GovernedReadTarget,
    pub knowledge_scope: Option<KnowledgeScopePolicy>,
    pub expires_at_ms: Option<u64>,
    pub demoted: bool,
    pub visibility: String,
    pub redaction_status: String,
    pub nested_lineage: Option<Box<DerivedMemoryLineage>>,
}

/// A proof cannot be deserialized or constructed from public metadata.
#[derive(Debug, Clone, Default)]
pub struct ResolvedDerivedLineage {
    pub(crate) digests: BTreeSet<String>,
}

fn invalid(reason: &str) -> MemoryError {
    MemoryError::InvalidConfig(format!("invalid_derived_memory_lineage:{reason}"))
}

fn bounded_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.trim() == value && !value.chars().any(char::is_control)
}

pub(crate) fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn optional_id(value: Option<&str>) -> bool {
    value.is_none_or(bounded_id)
}

fn data_class_storage_rank(class: DataClass) -> u8 {
    match class {
        DataClass::Public => 0, DataClass::Internal => 1, DataClass::CustomerData => 2,
        DataClass::SourceCode => 3, DataClass::FinancialRecord => 4, DataClass::Confidential => 5,
        DataClass::Regulated => 6, DataClass::Executive => 7, DataClass::Restricted => 8,
        DataClass::Credential => 9,
    }
}

pub fn canonical_lineage_digest<T: Serialize>(value: &T) -> MemoryResult<String> {
    fn sort(value: &mut Value) {
        match value {
            Value::Object(object) => {
                let mut entries = std::mem::take(object).into_iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                for (key, mut child) in entries { sort(&mut child); object.insert(key, child); }
            }
            Value::Array(array) => array.iter_mut().for_each(sort),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value).map_err(|_| invalid("not_serializable"))?;
    sort(&mut value);
    let bytes = serde_json::to_vec(&value).map_err(|_| invalid("not_serializable"))?;
    if bytes.len() > MAX_LINEAGE_BYTES { return Err(invalid("too_large")); }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl CanonicalMemoryRestriction {
    pub fn from_global_record(record: &GlobalMemoryRecord, tenant: &MemoryTenantScope) -> MemoryResult<Self> {
        if crate::store::tenant_scope_from_global_record(record) != *tenant {
            return Err(invalid("source_tenant_mismatch"));
        }
        let actual_hash = format!("{:x}", Sha256::digest(record.content.as_bytes()));
        if record.content_hash != actual_hash { return Err(invalid("source_content_hash_mismatch")); }
        let knowledge_scope = KnowledgeScopePolicy::from_metadata(record.metadata.as_ref())
            .map_err(|_| invalid("source_knowledge_scope"))?;
        let metadata = record.metadata.as_ref();
        for key in ["owner_subject", "owner_org_unit_id"] {
            if let Some(value) = metadata.and_then(|m| m.get(key)) {
                if !value.is_null() && !value.as_str().is_some_and(bounded_id) { return Err(invalid("source_owner")); }
            }
        }
        if metadata.and_then(|m| m.get("tenant_shared")).is_some_and(|v| !v.is_boolean()) {
            return Err(invalid("source_tenant_shared"));
        }
        let owner_subject = owner_subject_from_metadata(metadata);
        let owner_org_unit_id = owner_org_unit_id_from_metadata(metadata);
        let source_target = MemorySourceAccessTarget::from_metadata(metadata);
        if metadata.and_then(|m| m.get("enterprise_source_binding")).is_some()
            && source_target.as_ref().is_none_or(|source| source.source_binding_id.as_deref().is_none_or(|id| !bounded_id(id))) {
            return Err(invalid("source_binding"));
        }
        if metadata.and_then(|m| m.pointer("/memory_trust/label")).and_then(Value::as_str)
            == Some("connector_sourced") && source_target.is_none() {
            return Err(invalid("source_binding_missing"));
        }
        let mut target = if let Some(policy) = &knowledge_scope {
            policy.governed_read_target()
        } else if let Some(source) = source_target {
            GovernedReadTarget {
                resource_ref: source.resource_ref, data_class: source.data_class,
                source_binding_id: source.source_binding_id, source_object_id: source.source_object_id,
                evidence: GovernedReadEvidence::SourceBinding,
                owner_org_unit_id: None, owner_subject: None, tenant_shared: false,
            }
        } else {
            let mut resource_ref = ResourceRef::new(tenant.org_id.clone(), tenant.workspace_id.clone(),
                ResourceKind::MemorySpace, global_memory_record_resource_id(record));
            if let Some(project) = &record.project_tag { resource_ref = resource_ref.with_project_id(project.clone()); }
            GovernedReadTarget {
                resource_ref, data_class: data_class_from_metadata(metadata).unwrap_or(DataClass::Internal),
                source_binding_id: None, source_object_id: None,
                evidence: GovernedReadEvidence::TenantLocalMemory,
                owner_org_unit_id: owner_org_unit_id.clone(), owner_subject: None,
                tenant_shared: tenant_shared_from_metadata(metadata),
            }
        };
        // Subject privacy is additional to grants; department grant semantics
        // deliberately remain unchanged.
        target.owner_subject = owner_subject;
        target.owner_org_unit_id = owner_org_unit_id;
        let mut restriction = Self {
            memory_id: record.id.clone(), content_hash: record.content_hash.clone(), restriction_digest: String::new(),
            tenant_scope: tenant.clone(), target, knowledge_scope, expires_at_ms: record.expires_at_ms,
            demoted: record.demoted, visibility: record.visibility.clone(), redaction_status: record.redaction_status.clone(),
            nested_lineage: DerivedMemoryLineage::from_metadata(metadata)?.map(Box::new),
        };
        restriction.restriction_digest = restriction.semantic_digest()?;
        restriction.validate_at_depth(0)?;
        Ok(restriction)
    }

    pub fn source_reference(&self) -> MemorySourceReference {
        MemorySourceReference { memory_id: self.memory_id.clone(), content_hash: self.content_hash.clone(),
            restriction_digest: self.restriction_digest.clone() }
    }

    pub fn data_class(&self) -> DataClass { self.target.data_class }

    fn semantic_digest(&self) -> MemoryResult<String> {
        let mut payload = self.clone();
        payload.restriction_digest.clear();
        canonical_lineage_digest(&payload)
    }

    fn validate_at_depth(&self, depth: usize) -> MemoryResult<()> {
        if depth > MAX_DERIVED_LINEAGE_DEPTH || !bounded_id(&self.memory_id) || !valid_digest(&self.content_hash)
            || !valid_digest(&self.restriction_digest) || self.semantic_digest()? != self.restriction_digest
            || !bounded_id(&self.tenant_scope.org_id) || !bounded_id(&self.tenant_scope.workspace_id)
            || self.target.resource_ref.organization_id != self.tenant_scope.org_id
            || self.target.resource_ref.workspace_id != self.tenant_scope.workspace_id
            || !bounded_id(&self.target.resource_ref.resource_id)
            || !optional_id(self.target.owner_subject.as_deref())
            || !optional_id(self.target.owner_org_unit_id.as_deref())
            || !optional_id(self.target.source_binding_id.as_deref())
            || !optional_id(self.target.source_object_id.as_deref())
            || !matches!(self.visibility.as_str(), "private" | "shared")
            || !matches!(self.redaction_status.as_str(), "passed" | "redacted") {
            return Err(invalid("source_restriction"));
        }
        if self.target.evidence == GovernedReadEvidence::SourceBinding
            && self.target.source_binding_id.is_none() && self.knowledge_scope.is_none() {
            return Err(invalid("source_binding_id"));
        }
        if self.target.source_object_id.is_some() && self.target.source_binding_id.is_none() {
            return Err(invalid("source_object_binding_missing"));
        }
        if let Some(lineage) = &self.nested_lineage { lineage.validate_at_depth(depth + 1)?; }
        Ok(())
    }
}

impl DerivedMemoryLineage {
    pub fn new(owner_subject: Option<String>, owner_org_unit_id: Option<String>, sources: Vec<CanonicalMemoryRestriction>,
        input_refs: Vec<CanonicalInputReference>) -> MemoryResult<Self> {
        let lineage = Self { schema_version: DERIVED_MEMORY_LINEAGE_SCHEMA_VERSION, owner_subject,
            owner_org_unit_id, sources, input_refs };
        lineage.validate()?;
        Ok(lineage)
    }

    pub fn from_metadata(metadata: Option<&Value>) -> MemoryResult<Option<Self>> {
        let Some(value) = metadata.and_then(|m| m.get(DERIVED_MEMORY_LINEAGE_METADATA_KEY)) else { return Ok(None); };
        if serde_json::to_vec(value).map_err(|_| invalid("not_serializable"))?.len() > MAX_LINEAGE_BYTES {
            return Err(invalid("too_large"));
        }
        let lineage: Self = serde_json::from_value(value.clone()).map_err(|_| invalid("malformed"))?;
        lineage.validate()?;
        Ok(Some(lineage))
    }

    pub fn validate(&self) -> MemoryResult<()> { self.validate_at_depth(0) }
    pub fn digest(&self) -> MemoryResult<String> { self.validate()?; canonical_lineage_digest(self) }

    /// Pick an actual contributing class for the output's scalar storage label.
    /// The stable order is Public, Internal, CustomerData, SourceCode,
    /// FinancialRecord, Confidential, Regulated, Executive, Restricted,
    /// Credential. This is a deterministic representative, not a permission
    /// lattice: every original class/target remains in the read conjunction and
    /// provider egress union. Native session inputs contribute Internal.
    pub fn output_data_class(&self) -> DataClass {
        self.collected_data_classes().last().copied().unwrap_or(DataClass::Internal)
    }

    /// Exact, deduplicated class union for provider egress. Intermediate output
    /// labels cannot erase a more specific class in a nested source. The result
    /// is bounded by the finite DataClass enum; invalid lineage is rejected.
    pub fn source_data_classes(&self) -> MemoryResult<Vec<DataClass>> {
        self.all_input_refs()?;
        Ok(self.collected_data_classes())
    }

    fn collected_data_classes(&self) -> Vec<DataClass> {
        fn include(classes: &mut Vec<DataClass>, class: DataClass) {
            if !classes.contains(&class) { classes.push(class); }
        }
        fn visit(lineage: &DerivedMemoryLineage, depth: usize, classes: &mut Vec<DataClass>) {
            if depth > MAX_DERIVED_LINEAGE_DEPTH { return; }
            if lineage.input_refs.iter().any(|input| matches!(input, CanonicalInputReference::SessionMessage { .. })) {
                include(classes, DataClass::Internal);
            }
            for source in &lineage.sources { include(classes, source.data_class()); }
            for source in &lineage.sources {
                if let Some(nested) = &source.nested_lineage { visit(nested, depth + 1, classes); }
            }
        }
        let mut classes = Vec::new();
        visit(self, 0, &mut classes);
        classes.sort_unstable_by_key(|class| data_class_storage_rank(*class));
        classes
    }

    /// A new output policy cannot erase a contributing source's write limits.
    pub fn write_scope_decision(&self, partition: &MemoryPartition, now_ms: u64)
        -> MemoryResult<KnowledgeScopeDecision> {
        self.validate()?;
        Ok(self.inherited_scope_decision(partition, None, now_ms))
    }

    /// Promotion is constrained by every contributing source, including
    /// approval requirements retained through an intermediate derivative.
    pub fn promotion_scope_decision(&self, partition: &MemoryPartition, to_tier: GovernedMemoryTier,
        review: &PromotionReview, now_ms: u64) -> MemoryResult<KnowledgeScopeDecision> {
        self.validate()?;
        Ok(self.inherited_scope_decision(partition, Some((to_tier, review)), now_ms))
    }

    fn inherited_scope_decision(&self, partition: &MemoryPartition,
        promotion: Option<(GovernedMemoryTier, &PromotionReview)>, now_ms: u64) -> KnowledgeScopeDecision {
        for source in &self.sources {
            if source.tenant_scope.org_id != partition.org_id
                || source.tenant_scope.workspace_id != partition.workspace_id {
                return KnowledgeScopeDecision::deny("derived_lineage_source_tenant_mismatch");
            }
            if source.demoted || source.expires_at_ms.is_some_and(|expiry| expiry <= now_ms) {
                return KnowledgeScopeDecision::deny("derived_lineage_source_inactive");
            }
            if let Some(nested) = &source.nested_lineage {
                let decision = nested.inherited_scope_decision(partition, promotion, now_ms);
                if !decision.allowed { return decision; }
            }
            if let Some(policy) = &source.knowledge_scope {
                let decision = match promotion {
                    Some((to_tier, review)) => policy.promotion_decision(partition, to_tier, review, now_ms),
                    None => policy.write_decision(partition, now_ms),
                };
                if !decision.allowed { return decision; }
            }
        }
        KnowledgeScopeDecision::allow("derived_lineage_inherited_scope_allowed")
    }

    /// Return every native input, including inputs inherited through sources.
    /// Conflicting references to the same input fail closed rather than hiding
    /// one body revision behind deduplication.
    pub fn all_input_refs(&self) -> MemoryResult<Vec<CanonicalInputReference>> {
        fn visit(lineage: &DerivedMemoryLineage, depth: usize, path: &mut BTreeSet<String>,
            output: &mut Vec<CanonicalInputReference>) -> MemoryResult<()> {
            if depth > MAX_DERIVED_LINEAGE_DEPTH { return Err(invalid("input_depth")); }
            for input in &lineage.input_refs {
                let same_identity = |other: &&CanonicalInputReference| match (input, *other) {
                    (CanonicalInputReference::SessionMessage {session_id:a, message_id:b, ..},
                        CanonicalInputReference::SessionMessage {session_id:c, message_id:d, ..}) => a == c && b == d,
                    (CanonicalInputReference::Memory {source:a}, CanonicalInputReference::Memory {source:b}) => a.memory_id == b.memory_id,
                    _ => false,
                };
                if let Some(existing) = output.iter().find(same_identity) {
                    if existing != input { return Err(invalid("conflicting_input")); }
                } else { output.push(input.clone()); }
                if output.len() > MAX_DERIVED_MEMORY_INPUTS { return Err(invalid("input_count")); }
            }
            for source in &lineage.sources {
                if !path.insert(source.memory_id.clone()) { return Err(invalid("input_cycle")); }
                if let Some(nested) = &source.nested_lineage { visit(nested, depth + 1, path, output)?; }
                path.remove(&source.memory_id);
            }
            Ok(())
        }
        self.validate()?;
        let mut output = Vec::new();
        visit(self, 0, &mut BTreeSet::new(), &mut output)?;
        Ok(output)
    }

    fn validate_at_depth(&self, depth: usize) -> MemoryResult<()> {
        if depth > MAX_DERIVED_LINEAGE_DEPTH || self.schema_version != DERIVED_MEMORY_LINEAGE_SCHEMA_VERSION
            || self.sources.len() > MAX_DERIVED_MEMORY_SOURCES || self.input_refs.is_empty()
            || self.input_refs.len() > MAX_DERIVED_MEMORY_INPUTS
            || !optional_id(self.owner_subject.as_deref()) || !optional_id(self.owner_org_unit_id.as_deref()) {
            return Err(invalid("schema_or_bounds"));
        }
        if self.input_refs.iter().any(|input| matches!(input, CanonicalInputReference::SessionMessage { .. }))
            && self.owner_subject.is_none() {
            return Err(invalid("native_session_owner_missing"));
        }
        if self.owner_subject.is_none() && self.owner_org_unit_id.is_none()
            && (self.sources.is_empty() || self.sources.iter().any(|source| {
                source.target.owner_subject.is_some()
                    || (source.target.owner_org_unit_id.is_none() && !source.target.tenant_shared
                        && source.target.evidence != GovernedReadEvidence::SourceBinding)
            })) {
            return Err(invalid("shared_disposition_missing"));
        }
        let mut source_ids = BTreeSet::new();
        for source in &self.sources {
            source.validate_at_depth(depth + 1)?;
            if !source_ids.insert(source.memory_id.as_str()) { return Err(invalid("duplicate_source")); }
            if source.target.owner_subject.as_deref().is_some_and(|owner| Some(owner) != self.owner_subject.as_deref())
                || source.nested_lineage.as_ref().and_then(|l| l.owner_subject.as_deref())
                    .is_some_and(|owner| Some(owner) != self.owner_subject.as_deref()) {
                return Err(invalid("private_source_owner_dropped"));
            }
        }
        let mut inputs = BTreeSet::new();
        for input in &self.input_refs {
            let key = match input {
                CanonicalInputReference::SessionMessage { session_id, message_id, body_digest } => {
                    if !bounded_id(session_id) || !bounded_id(message_id) || !valid_digest(body_digest) {
                        return Err(invalid("message_reference"));
                    }
                    format!("message:{session_id}:{message_id}")
                }
                CanonicalInputReference::Memory { source } => {
                    if !self.sources.iter().any(|s| s.source_reference() == *source) { return Err(invalid("memory_reference")); }
                    format!("memory:{}", source.memory_id)
                }
            };
            if !inputs.insert(key) { return Err(invalid("duplicate_input")); }
        }
        canonical_lineage_digest(self)?;
        Ok(())
    }
}

pub fn metadata_with_derived_lineage(metadata: Option<Value>, lineage: &DerivedMemoryLineage) -> MemoryResult<Option<Value>> {
    lineage.validate()?;
    let mut object = match metadata { Some(Value::Object(object)) => object, None => Default::default(),
        Some(_) => return Err(invalid("metadata_not_object")) };
    object.insert(DERIVED_MEMORY_LINEAGE_METADATA_KEY.to_string(),
        serde_json::to_value(lineage).map_err(|_| invalid("not_serializable"))?);
    Ok(Some(Value::Object(object)))
}

pub fn derived_lineage_dedupe_digest(metadata: Option<&Value>) -> MemoryResult<String> {
    DerivedMemoryLineage::from_metadata(metadata)?.map(|lineage| lineage.digest()).transpose()
        .map(|digest| digest.unwrap_or_default())
}

pub fn reject_reserved_derived_metadata(metadata: Option<&Value>) -> MemoryResult<()> {
    if metadata.and_then(|value| value.get(DERIVED_MEMORY_LINEAGE_METADATA_KEY)).is_some() {
        Err(invalid("reserved_metadata"))
    } else { Ok(()) }
}

impl ResolvedDerivedLineage {
    pub fn contains(&self, lineage: &DerivedMemoryLineage) -> bool {
        lineage.digest().is_ok_and(|digest| self.digests.contains(&digest))
    }
}

impl MemoryAccessFilter {
    pub fn with_resolved_derived_lineage(mut self, proof: ResolvedDerivedLineage) -> Self {
        self.resolved_derived_lineages.digests.extend(proof.digests);
        self
    }

    pub fn decision_for_derived_lineage(&self, lineage: &DerivedMemoryLineage) -> GovernedReadDecision {
        let Ok(digest) = lineage.digest() else { return GovernedReadDecision::deny("derived_lineage_malformed"); };
        if !self.resolved_derived_lineages.digests.contains(&digest) {
            return GovernedReadDecision::deny("derived_lineage_unresolved");
        }
        if let Some(decision) = self.decision_for_owner_subject(lineage.owner_subject.as_deref()) { return decision; }
        if self.mode != crate::types::GovernedReadMode::LocalNoop {
            if let Some(unit) = &lineage.owner_org_unit_id {
                if !self.caller_org_units.as_ref().is_some_and(|units| units.contains(unit)) {
                    return GovernedReadDecision::deny("derived_lineage_department_denied");
                }
            }
        }
        for source in &lineage.sources {
            if source.demoted || source.expires_at_ms.is_some_and(|expires| expires <= self.now_ms) {
                return GovernedReadDecision::deny("derived_lineage_source_inactive");
            }
            if let Some(nested) = &source.nested_lineage {
                let decision = self.decision_for_derived_lineage(nested);
                if !decision.allowed { return decision; }
            }
            if let Some(policy) = &source.knowledge_scope {
                if let Some(reason) = policy.read_denial_reason(self.workflow_phase.as_deref(), self.now_ms) {
                    return GovernedReadDecision::deny(reason);
                }
            }
            let decision = self.decision_for_target(&source.target);
            if !decision.allowed { return decision; }
        }
        GovernedReadDecision::allow("derived_lineage_allowed")
    }

    pub(crate) fn decision_for_lineage_metadata(&self, metadata: Option<&Value>) -> Option<GovernedReadDecision> {
        match DerivedMemoryLineage::from_metadata(metadata) {
            Ok(Some(lineage)) => {
                let decision = self.decision_for_derived_lineage(&lineage);
                (!decision.allowed).then_some(decision)
            }
            Ok(None) => None,
            Err(_) => Some(GovernedReadDecision::deny("derived_lineage_malformed")),
        }
    }

    pub(crate) fn decision_for_owner_subject(&self, owner: Option<&str>) -> Option<GovernedReadDecision> {
        if self.mode == crate::types::GovernedReadMode::LocalNoop { return None; }
        let owner = owner?;
        if self.caller_subject.as_deref() != Some(owner) {
            Some(GovernedReadDecision::deny("subject_scope_mismatch"))
        } else { None }
    }

    pub(crate) fn decision_for_owner_metadata(&self, metadata: Option<&Value>) -> Option<GovernedReadDecision> {
        let value = metadata?.get("owner_subject")?;
        if value.is_null() { return None; }
        match value.as_str().filter(|owner| bounded_id(owner)) {
            Some(owner) => self.decision_for_owner_subject(Some(owner)),
            None => Some(GovernedReadDecision::deny("invalid_owner_subject")),
        }
    }
}

#[cfg(test)]
#[path = "derived_lineage_tests.rs"]
mod tests;
