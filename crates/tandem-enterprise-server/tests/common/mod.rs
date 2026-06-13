#![allow(dead_code)]
// EAA-10 (TAN-35): shared request-body builders for the enterprise HTTP tests.
use serde_json::json;

pub fn source_binding_body(binding_id: &str, org_id: &str, workspace_id: &str) -> String {
    json!({
        "binding_id": binding_id,
        "connector_id": "google_drive",
        "source_type": "google_drive",
        "native_source_id": "drive-root-123",
        "source_root_label": "Finance Drive",
        "resource_ref": {
            "organization_id": org_id,
            "workspace_id": workspace_id,
            "resource_kind": "document_collection",
            "resource_id": binding_id
        },
        "data_class": "financial_record",
        "ingestion_policy": {
            "allow_indexing": true,
            "allow_prompt_context": true,
            "require_review": false
        }
    })
    .to_string()
}

pub fn connector_body(connector_id: &str, provider: &str) -> String {
    json!({
        "connector_id": connector_id,
        "provider": provider,
        "display_name": "Finance Drive Connector"
    })
    .to_string()
}

pub fn connector_credential_ref_body(credential_id: &str, secret_id: &str) -> String {
    json!({
        "credential_id": credential_id,
        "credential_class": "read_only",
        "secret_ref": {
            "org_id": "acme",
            "workspace_id": "finance",
            "provider": "google_kms",
            "secret_id": secret_id,
            "name": "Finance Drive read-only secret"
        },
        "source_bound_resource": {
            "organization_id": "acme",
            "workspace_id": "finance",
            "resource_kind": "document_collection",
            "resource_id": "finance-drive"
        }
    })
    .to_string()
}

