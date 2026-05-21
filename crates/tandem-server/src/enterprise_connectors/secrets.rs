use async_trait::async_trait;
use std::fmt;
use tandem_enterprise_contract::{ConnectorCredentialRef, SecretRef, TenantContext};

const ENV_SECRET_PREFIX: &str = "env://";

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve_bearer_token(
        &self,
        tenant_context: &TenantContext,
        credential_ref: &ConnectorCredentialRef,
    ) -> Result<ResolvedBearerToken, SecretResolverError>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedBearerToken(String);

impl ResolvedBearerToken {
    pub fn expose_for_request(&self) -> &str {
        &self.0
    }
}

impl ResolvedBearerToken {
    #[cfg(test)]
    pub(crate) fn from_test_token(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Debug for ResolvedBearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResolvedBearerToken(<redacted>)")
    }
}

#[derive(Debug, Default, Clone)]
pub struct EnvSecretResolver;

#[async_trait]
impl SecretResolver for EnvSecretResolver {
    async fn resolve_bearer_token(
        &self,
        tenant_context: &TenantContext,
        credential_ref: &ConnectorCredentialRef,
    ) -> Result<ResolvedBearerToken, SecretResolverError> {
        validate_credential_tenant(tenant_context, credential_ref)?;
        let env_var = env_var_name(&credential_ref.secret_ref)?;
        let value = std::env::var(env_var)
            .map_err(|_| SecretResolverError::SecretUnavailable { provider: "env" })?;
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(SecretResolverError::SecretUnavailable { provider: "env" });
        }
        Ok(ResolvedBearerToken(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretResolverError {
    TenantMismatch,
    UnsupportedProvider { provider: String },
    InvalidSecretId { provider: String },
    SecretUnavailable { provider: &'static str },
}

impl fmt::Display for SecretResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TenantMismatch => formatter.write_str("secret ref tenant mismatch"),
            Self::UnsupportedProvider { provider } => {
                write!(formatter, "unsupported secret provider `{provider}`")
            }
            Self::InvalidSecretId { provider } => {
                write!(formatter, "invalid secret id for provider `{provider}`")
            }
            Self::SecretUnavailable { provider } => {
                write!(formatter, "secret unavailable from provider `{provider}`")
            }
        }
    }
}

impl std::error::Error for SecretResolverError {}

fn validate_credential_tenant(
    tenant_context: &TenantContext,
    credential_ref: &ConnectorCredentialRef,
) -> Result<(), SecretResolverError> {
    if credential_ref.org_id != tenant_context.org_id
        || credential_ref.workspace_id != tenant_context.workspace_id
        || credential_ref.secret_ref.org_id != tenant_context.org_id
        || credential_ref.secret_ref.workspace_id != tenant_context.workspace_id
    {
        return Err(SecretResolverError::TenantMismatch);
    }
    Ok(())
}

fn env_var_name(secret_ref: &SecretRef) -> Result<&str, SecretResolverError> {
    if secret_ref.provider != "env" {
        return Err(SecretResolverError::UnsupportedProvider {
            provider: secret_ref.provider.clone(),
        });
    }
    let Some(raw) = secret_ref.secret_id.strip_prefix(ENV_SECRET_PREFIX) else {
        return Err(SecretResolverError::InvalidSecretId {
            provider: secret_ref.provider.clone(),
        });
    };
    if raw.is_empty()
        || raw.contains('/')
        || raw.contains('\\')
        || raw.contains('=')
        || !raw
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(SecretResolverError::InvalidSecretId {
            provider: secret_ref.provider.clone(),
        });
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tandem_enterprise_contract::{ConnectorCredentialClass, ResourceKind, ResourceRef};

    #[tokio::test]
    #[serial]
    async fn env_resolver_returns_runtime_only_bearer_token() {
        std::env::set_var("TANDEM_TEST_GOOGLE_DRIVE_TOKEN", "test-drive-token");
        let tenant = TenantContext::explicit("acme", "finance", None);
        let credential = test_credential("env", "env://TANDEM_TEST_GOOGLE_DRIVE_TOKEN");

        let token = EnvSecretResolver
            .resolve_bearer_token(&tenant, &credential)
            .await
            .expect("resolved token");

        assert_eq!(token.expose_for_request(), "test-drive-token");
        assert_eq!(format!("{token:?}"), "ResolvedBearerToken(<redacted>)");
        std::env::remove_var("TANDEM_TEST_GOOGLE_DRIVE_TOKEN");
    }

    #[tokio::test]
    #[serial]
    async fn env_resolver_rejects_unsupported_provider_and_invalid_ids() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let unsupported = test_credential("google_kms", "kms://finance/token");
        let invalid_env = test_credential("env", "env://../TOKEN");

        assert!(matches!(
            EnvSecretResolver
                .resolve_bearer_token(&tenant, &unsupported)
                .await,
            Err(SecretResolverError::UnsupportedProvider { provider }) if provider == "google_kms"
        ));
        assert!(matches!(
            EnvSecretResolver
                .resolve_bearer_token(&tenant, &invalid_env)
                .await,
            Err(SecretResolverError::InvalidSecretId { provider }) if provider == "env"
        ));
    }

    #[tokio::test]
    #[serial]
    async fn env_resolver_does_not_leak_missing_secret_names_in_display() {
        std::env::remove_var("TANDEM_TEST_MISSING_DRIVE_TOKEN");
        let tenant = TenantContext::explicit("acme", "finance", None);
        let credential = test_credential("env", "env://TANDEM_TEST_MISSING_DRIVE_TOKEN");

        let error = EnvSecretResolver
            .resolve_bearer_token(&tenant, &credential)
            .await
            .expect_err("missing secret should fail");

        assert_eq!(error.to_string(), "secret unavailable from provider `env`");
        assert!(!error
            .to_string()
            .contains("TANDEM_TEST_MISSING_DRIVE_TOKEN"));
    }

    #[tokio::test]
    async fn env_resolver_rejects_cross_tenant_credentials() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let mut credential = test_credential("env", "env://TANDEM_TEST_GOOGLE_DRIVE_TOKEN");
        credential.workspace_id = "hr".to_string();

        assert!(matches!(
            EnvSecretResolver
                .resolve_bearer_token(&tenant, &credential)
                .await,
            Err(SecretResolverError::TenantMismatch)
        ));
    }

    fn test_credential(provider: &str, secret_id: &str) -> ConnectorCredentialRef {
        ConnectorCredentialRef {
            org_id: "acme".to_string(),
            workspace_id: "finance".to_string(),
            connector_id: "google_drive".to_string(),
            credential_id: "drive-readonly".to_string(),
            credential_class: ConnectorCredentialClass::ReadOnly,
            secret_ref: SecretRef {
                org_id: "acme".to_string(),
                workspace_id: "finance".to_string(),
                provider: provider.to_string(),
                secret_id: secret_id.to_string(),
                name: "Local Google Drive token".to_string(),
            },
            source_bound_resource: Some(ResourceRef {
                organization_id: "acme".to_string(),
                workspace_id: "finance".to_string(),
                project_id: None,
                resource_kind: ResourceKind::DocumentCollection,
                resource_id: "finance-drive".to_string(),
                parent_path: Vec::new(),
                branch_id: None,
                path_prefix: None,
            }),
            created_at_ms: 1_000,
            rotated_at_ms: None,
            expires_at_ms: None,
        }
    }
}
