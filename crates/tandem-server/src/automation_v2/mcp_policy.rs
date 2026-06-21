use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationMcpRunAs {
    CurrentActor,
    ServicePrincipal { principal_id: String },
    AutomationPrincipal { automation_id: String },
    SharedConnection { grant_id: String },
}

impl AutomationMcpRunAs {
    fn normalized(self) -> Self {
        match self {
            Self::CurrentActor => Self::CurrentActor,
            Self::ServicePrincipal { principal_id } => Self::ServicePrincipal {
                principal_id: principal_id.trim().to_string(),
            },
            Self::AutomationPrincipal { automation_id } => Self::AutomationPrincipal {
                automation_id: automation_id.trim().to_string(),
            },
            Self::SharedConnection { grant_id } => Self::SharedConnection {
                grant_id: grant_id.trim().to_string(),
            },
        }
    }

    fn sort_key(&self) -> String {
        match self {
            Self::CurrentActor => "current_actor".to_string(),
            Self::ServicePrincipal { principal_id } => format!("service:{principal_id}"),
            Self::AutomationPrincipal { automation_id } => format!("automation:{automation_id}"),
            Self::SharedConnection { grant_id } => format!("shared:{grant_id}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationMcpConnectionGrant {
    pub server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_as: Option<AutomationMcpRunAs>,
}

impl AutomationMcpConnectionGrant {
    fn normalized(self) -> Option<Self> {
        let server = self.server.trim().to_string();
        if server.is_empty() {
            return None;
        }
        Some(Self {
            server,
            connection_id: normalize_optional_string(self.connection_id),
            run_as: self.run_as.map(AutomationMcpRunAs::normalized),
        })
    }

    fn sort_key(&self) -> (String, String, String) {
        (
            self.server.to_ascii_lowercase(),
            self.connection_id.clone().unwrap_or_default(),
            self.run_as
                .as_ref()
                .map(AutomationMcpRunAs::sort_key)
                .unwrap_or_default(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationAgentMcpPolicy {
    #[serde(default)]
    pub allowed_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_connections: Vec<AutomationMcpConnectionGrant>,
}

impl AutomationAgentMcpPolicy {
    pub fn normalize(&mut self) {
        self.allowed_servers = normalize_strings(&self.allowed_servers);
        if let Some(allowed_tools) = self.allowed_tools.as_mut() {
            *allowed_tools = normalize_strings(allowed_tools);
        }
        self.allowed_connections =
            normalize_connection_grants(std::mem::take(&mut self.allowed_connections));
    }

    pub fn effective_allowed_servers(&self) -> Vec<String> {
        let mut servers = self.allowed_servers.clone();
        servers.extend(
            self.allowed_connections
                .iter()
                .map(|grant| grant.server.clone()),
        );
        normalize_strings(&servers)
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_strings(values: &[String]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn normalize_connection_grants(
    grants: Vec<AutomationMcpConnectionGrant>,
) -> Vec<AutomationMcpConnectionGrant> {
    let mut grants = grants
        .into_iter()
        .filter_map(AutomationMcpConnectionGrant::normalized)
        .collect::<Vec<_>>();
    grants.sort_by_key(AutomationMcpConnectionGrant::sort_key);
    grants.dedup_by_key(|grant| grant.sort_key());
    grants
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mcp_policy_defaults_allowed_connections_for_legacy_specs() {
        let policy: AutomationAgentMcpPolicy =
            serde_json::from_value(json!({ "allowed_servers": ["github"] }))
                .expect("legacy mcp policy");

        assert_eq!(policy.allowed_servers, vec!["github".to_string()]);
        assert!(policy.allowed_connections.is_empty());
        assert_eq!(
            policy.effective_allowed_servers(),
            vec!["github".to_string()]
        );
    }

    #[test]
    fn mcp_policy_includes_connection_grant_servers_in_effective_scope() {
        let mut policy = AutomationAgentMcpPolicy {
            allowed_servers: vec!["github".to_string(), " github ".to_string()],
            allowed_tools: None,
            allowed_connections: vec![AutomationMcpConnectionGrant {
                server: " linear ".to_string(),
                connection_id: Some(" conn-1 ".to_string()),
                run_as: Some(AutomationMcpRunAs::SharedConnection {
                    grant_id: " shared-1 ".to_string(),
                }),
            }],
        };

        policy.normalize();

        assert_eq!(
            policy.effective_allowed_servers(),
            vec!["github".to_string(), "linear".to_string()]
        );
        assert_eq!(
            policy.allowed_connections[0].connection_id.as_deref(),
            Some("conn-1")
        );
    }
}
