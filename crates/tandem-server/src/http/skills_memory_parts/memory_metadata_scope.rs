// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// Stamp the collector's active department (`owner_org_unit_id`) into a record's
/// metadata so it flows into the first-class column via `put_global_memory_record`
/// (TAN-645/646). A department already present in the metadata — client-supplied
/// and membership-validated upstream — is preserved; otherwise the verified
/// context's active department is written. No-op when there is no active
/// department (unattributable data / local single-tenant mode).
fn memory_metadata_with_owner_org_unit(
    metadata: Option<Value>,
    owner_org_unit_id: Option<&str>,
) -> Option<Value> {
    let Some(owner_org_unit_id) = owner_org_unit_id else {
        return metadata;
    };
    // Explicit department ownership always wins. An explicitly tenant-shared
    // record without a department keeps its independent private-owner boundary.
    if tandem_memory::types::owner_org_unit_id_from_metadata(metadata.as_ref()).is_some()
        || tandem_memory::types::tenant_shared_from_metadata(metadata.as_ref())
    {
        return metadata;
    }
    let mut metadata = metadata.unwrap_or_else(|| json!({}));
    if !metadata.is_object() {
        metadata = json!({ "value": metadata });
    }
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert(
            tandem_memory::types::OWNER_ORG_UNIT_METADATA_KEY.to_string(),
            json!(owner_org_unit_id),
        );
    }
    Some(metadata)
}
