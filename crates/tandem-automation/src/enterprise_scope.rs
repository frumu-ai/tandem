use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::{DataClass, PrincipalRef, ResourceScope, ToolRiskTier};

pub const AUTOMATION_ENTERPRISE_SCOPE_METADATA_KEY: &str = "enterprise_scope";
pub const AUTOMATION_RESOURCE_ACCESS_METADATA_KEY: &str = "resource_access";
pub const AUTOMATION_WEBHOOK_SCOPE_METADATA_KEY: &str = "automation_webhook";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AutomationEnterpriseScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_principal: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owning_org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_scope: Option<ResourceScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<ToolRiskTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegation_grant_ids: Vec<String>,
}

impl AutomationEnterpriseScope {
    pub fn is_empty(&self) -> bool {
        self.owner_principal.is_none()
            && self.owning_org_unit_id.is_none()
            && self.resource_scope.is_none()
            && self.data_classes.is_empty()
            && self.risk_tier.is_none()
            && self.policy_version_id.is_none()
            && self.delegation_grant_ids.is_empty()
    }

    pub fn from_metadata(metadata: Option<&Value>) -> Option<Self> {
        let metadata = metadata?.as_object()?;
        let mut scope = Self::default();
        scope.merge_value(metadata.get(AUTOMATION_RESOURCE_ACCESS_METADATA_KEY), false);
        scope.merge_value(metadata.get(AUTOMATION_WEBHOOK_SCOPE_METADATA_KEY), true);
        scope.merge_value(metadata.get(AUTOMATION_ENTERPRISE_SCOPE_METADATA_KEY), true);
        scope = scope.normalized();
        (!scope.is_empty()).then_some(scope)
    }

    pub fn normalized(mut self) -> Self {
        self.owning_org_unit_id = normalized_optional_string(self.owning_org_unit_id);
        self.policy_version_id = normalized_optional_string(self.policy_version_id);
        self.delegation_grant_ids =
            normalized_strings(std::mem::take(&mut self.delegation_grant_ids));
        self.data_classes = dedup_data_classes(std::mem::take(&mut self.data_classes));
        self
    }

    fn merge_value(&mut self, value: Option<&Value>, overwrite: bool) {
        let Some(value) = value else {
            return;
        };
        if overwrite || self.owner_principal.is_none() {
            self.owner_principal = json_field(value, "owner_principal").or_else(|| {
                json_field(value, "ownerPrincipal").or_else(|| self.owner_principal.clone())
            });
        }
        if overwrite || self.owning_org_unit_id.is_none() {
            self.owning_org_unit_id = json_string_field(value, "owning_org_unit_id")
                .or_else(|| json_string_field(value, "owningOrgUnitId"))
                .or_else(|| self.owning_org_unit_id.clone());
        }
        if overwrite || self.resource_scope.is_none() {
            self.resource_scope = json_field(value, "resource_scope")
                .or_else(|| json_field(value, "resourceScope"))
                .or_else(|| self.resource_scope.clone());
        }
        if overwrite || self.data_classes.is_empty() {
            self.data_classes = json_field(value, "data_classes")
                .or_else(|| json_field(value, "dataClasses"))
                .or_else(|| {
                    json_field::<DataClass>(value, "data_class")
                        .or_else(|| json_field::<DataClass>(value, "dataClass"))
                        .map(|data_class| vec![data_class])
                })
                .unwrap_or_else(|| self.data_classes.clone());
        }
        if overwrite || self.risk_tier.is_none() {
            self.risk_tier = json_field(value, "risk_tier")
                .or_else(|| json_field(value, "riskTier"))
                .or_else(|| self.risk_tier.clone());
        }
        if overwrite || self.policy_version_id.is_none() {
            self.policy_version_id = json_string_field(value, "policy_version_id")
                .or_else(|| json_string_field(value, "policyVersionId"))
                .or_else(|| json_string_field(value, "policy_version"))
                .or_else(|| json_string_field(value, "policyVersion"))
                .or_else(|| self.policy_version_id.clone());
        }
        if overwrite || self.delegation_grant_ids.is_empty() {
            self.delegation_grant_ids = json_field(value, "delegation_grant_ids")
                .or_else(|| json_field(value, "delegationGrantIds"))
                .unwrap_or_else(|| self.delegation_grant_ids.clone());
        }
    }
}

pub fn stamp_enterprise_scope_metadata(metadata: Option<Value>) -> Option<Value> {
    let Some(scope) = AutomationEnterpriseScope::from_metadata(metadata.as_ref()) else {
        return metadata;
    };
    upsert_enterprise_scope_metadata(metadata, &scope)
}

pub fn upsert_enterprise_scope_metadata(
    metadata: Option<Value>,
    scope: &AutomationEnterpriseScope,
) -> Option<Value> {
    let scope = scope.clone().normalized();
    if scope.is_empty() {
        return metadata;
    }
    let mut object = metadata
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let value = serde_json::to_value(scope).unwrap_or(Value::Null);
    object.insert(AUTOMATION_ENTERPRISE_SCOPE_METADATA_KEY.to_string(), value);
    Some(Value::Object(object))
}

fn json_field<T: for<'de> Deserialize<'de>>(value: &Value, key: &str) -> Option<T> {
    value
        .get(key)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn json_string_field(value: &Value, key: &str) -> Option<String> {
    let value = value.get(key)?;
    if let Some(text) = value.as_str() {
        return normalized_optional_string(Some(text.to_string()));
    }
    value.as_u64().map(|number| number.to_string())
}

fn normalized_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalized_strings(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .filter_map(|value| normalized_optional_string(Some(value)))
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn dedup_data_classes(values: Vec<DataClass>) -> Vec<DataClass> {
    let mut keyed = values
        .into_iter()
        .filter_map(|value| serde_json::to_string(&value).ok().map(|key| (key, value)))
        .collect::<Vec<_>>();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);
    keyed.into_iter().map(|(_, value)| value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_types::{PrincipalKind, PrincipalRef, ResourceKind, ResourceRef};

    #[test]
    fn enterprise_scope_merges_resource_access_and_explicit_scope() {
        let metadata = json!({
            "resource_access": {
                "owner_principal": { "kind": "human_user", "id": "owner-a" },
                "owning_org_unit_id": "finance"
            },
            "enterprise_scope": {
                "owner_principal": { "kind": "automation", "id": "automation-a" },
                "policy_version": 42,
                "delegation_grant_ids": [" grant-b ", "grant-a", "grant-a"]
            }
        });

        let scope = AutomationEnterpriseScope::from_metadata(Some(&metadata)).expect("scope");

        assert_eq!(
            scope.owner_principal,
            Some(PrincipalRef::new(PrincipalKind::Automation, "automation-a"))
        );
        assert_eq!(scope.owning_org_unit_id.as_deref(), Some("finance"));
        assert_eq!(scope.policy_version_id.as_deref(), Some("42"));
        assert_eq!(
            scope.delegation_grant_ids,
            vec!["grant-a".to_string(), "grant-b".to_string()]
        );
    }

    #[test]
    fn webhook_scope_stamps_canonical_enterprise_metadata() {
        let metadata = json!({
            "automation_webhook": {
                "owning_org_unit_id": "platform",
                "resource_scope": {
                    "root": {
                        "organization_id": "org-a",
                        "workspace_id": "workspace-a",
                        "resource_kind": "source_binding",
                        "resource_id": "github"
                    }
                },
                "data_class": "source_code"
            }
        });

        let stamped = stamp_enterprise_scope_metadata(Some(metadata)).expect("metadata");
        let stamped_scope = stamped
            .get(AUTOMATION_ENTERPRISE_SCOPE_METADATA_KEY)
            .expect("enterprise scope");

        assert_eq!(
            stamped_scope
                .get("owning_org_unit_id")
                .and_then(Value::as_str),
            Some("platform")
        );
        assert_eq!(
            json_field::<ResourceScope>(stamped_scope, "resource_scope")
                .expect("resource scope")
                .root,
            ResourceRef::new(
                "org-a",
                "workspace-a",
                ResourceKind::SourceBinding,
                "github"
            )
        );
    }
}
