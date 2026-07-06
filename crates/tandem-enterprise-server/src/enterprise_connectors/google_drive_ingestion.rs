// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use async_trait::async_trait;
use serde::Serialize;
use std::fmt;
use tandem_enterprise_contract::{
    ConnectorCredentialClass, ConnectorInstance, ConnectorLifecycleState, SourceBinding,
    SourceBindingState, TenantContext,
};

use super::google_drive::{
    GoogleDriveClient, GoogleDriveClientError, GoogleDriveFileMetadata, GoogleDriveListPage,
};
use super::secrets::{SecretResolver, SecretResolverError};

const GOOGLE_DRIVE_PROVIDER: &str = "google_drive";
const GOOGLE_DRIVE_SOURCE_TYPE: &str = "google_drive";

#[async_trait]
pub trait GoogleDriveReadClient: Send + Sync {
    async fn list_folder_children(
        &self,
        bearer_token: &str,
        folder_id: &str,
        page_token: Option<&str>,
    ) -> Result<GoogleDriveListPage, GoogleDriveClientError>;

    async fn download_file_bytes(
        &self,
        bearer_token: &str,
        file_id: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError>;

    async fn export_google_workspace_file(
        &self,
        bearer_token: &str,
        file_id: &str,
        mime_type: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError>;
}

#[async_trait]
impl GoogleDriveReadClient for GoogleDriveClient {
    async fn list_folder_children(
        &self,
        bearer_token: &str,
        folder_id: &str,
        page_token: Option<&str>,
    ) -> Result<GoogleDriveListPage, GoogleDriveClientError> {
        GoogleDriveClient::list_folder_children(self, bearer_token, folder_id, page_token).await
    }

    async fn download_file_bytes(
        &self,
        bearer_token: &str,
        file_id: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError> {
        GoogleDriveClient::download_file_bytes(self, bearer_token, file_id).await
    }

    async fn export_google_workspace_file(
        &self,
        bearer_token: &str,
        file_id: &str,
        mime_type: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError> {
        GoogleDriveClient::export_google_workspace_file(self, bearer_token, file_id, mime_type)
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoogleDriveBindingPreflight {
    pub binding_id: String,
    pub connector_id: String,
    pub folder_id: String,
    pub file_count: usize,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoogleDriveIngestedFile {
    pub drive_file_id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoogleDriveBindingIngestion {
    pub binding_id: String,
    pub connector_id: String,
    pub folder_id: String,
    pub files: Vec<GoogleDriveIngestedFile>,
    pub skipped_files: usize,
}

pub async fn preflight_google_drive_binding<R, C>(
    tenant_context: &TenantContext,
    connector: &ConnectorInstance,
    binding: &SourceBinding,
    secret_resolver: &R,
    drive_client: &C,
) -> Result<GoogleDriveBindingPreflight, GoogleDriveIngestionError>
where
    R: SecretResolver,
    C: GoogleDriveReadClient,
{
    validate_google_drive_binding(tenant_context, connector, binding)?;
    let credential_ref_id = binding
        .credential_ref_id
        .as_deref()
        .ok_or(GoogleDriveIngestionError::MissingCredentialRef)?;
    let credential_ref = connector
        .credential_refs
        .iter()
        .find(|credential| credential.credential_id == credential_ref_id)
        .ok_or(GoogleDriveIngestionError::CredentialRefNotFound)?;
    if credential_ref.credential_class != ConnectorCredentialClass::ReadOnly {
        return Err(GoogleDriveIngestionError::CredentialNotReadOnly);
    }
    if credential_ref.source_bound_resource.as_ref() != Some(&binding.resource_ref) {
        return Err(GoogleDriveIngestionError::CredentialResourceMismatch);
    }

    let token = secret_resolver
        .resolve_bearer_token(tenant_context, credential_ref)
        .await
        .map_err(GoogleDriveIngestionError::Secret)?;
    let page = drive_client
        .list_folder_children(token.expose_for_request(), &binding.native_source_id, None)
        .await
        .map_err(GoogleDriveIngestionError::Drive)?;

    Ok(GoogleDriveBindingPreflight {
        binding_id: binding.binding_id.clone(),
        connector_id: connector.connector_id.clone(),
        folder_id: binding.native_source_id.clone(),
        file_count: page.files.len(),
        next_page_token: page.next_page_token,
    })
}

pub async fn fetch_google_drive_binding_files<R, C>(
    tenant_context: &TenantContext,
    connector: &ConnectorInstance,
    binding: &SourceBinding,
    secret_resolver: &R,
    drive_client: &C,
) -> Result<GoogleDriveBindingIngestion, GoogleDriveIngestionError>
where
    R: SecretResolver,
    C: GoogleDriveReadClient,
{
    validate_google_drive_binding(tenant_context, connector, binding)?;
    let credential_ref_id = binding
        .credential_ref_id
        .as_deref()
        .ok_or(GoogleDriveIngestionError::MissingCredentialRef)?;
    let credential_ref = connector
        .credential_refs
        .iter()
        .find(|credential| credential.credential_id == credential_ref_id)
        .ok_or(GoogleDriveIngestionError::CredentialRefNotFound)?;
    if credential_ref.credential_class != ConnectorCredentialClass::ReadOnly {
        return Err(GoogleDriveIngestionError::CredentialNotReadOnly);
    }
    if credential_ref.source_bound_resource.as_ref() != Some(&binding.resource_ref) {
        return Err(GoogleDriveIngestionError::CredentialResourceMismatch);
    }

    let token = secret_resolver
        .resolve_bearer_token(tenant_context, credential_ref)
        .await
        .map_err(GoogleDriveIngestionError::Secret)?;
    let mut page_token = None;
    let mut files = Vec::new();
    let mut skipped_files = 0usize;
    loop {
        let page = drive_client
            .list_folder_children(
                token.expose_for_request(),
                &binding.native_source_id,
                page_token.as_deref(),
            )
            .await
            .map_err(GoogleDriveIngestionError::Drive)?;
        for file in page.files {
            match fetch_supported_file(token.expose_for_request(), drive_client, &file).await {
                Ok(Some(bytes)) => files.push(GoogleDriveIngestedFile {
                    drive_file_id: file.id,
                    name: file.name,
                    mime_type: file.mime_type,
                    bytes,
                }),
                Ok(None) => skipped_files = skipped_files.saturating_add(1),
                Err(error) => return Err(GoogleDriveIngestionError::Drive(error)),
            }
        }
        page_token = page
            .next_page_token
            .filter(|value| !value.trim().is_empty());
        if page_token.is_none() {
            break;
        }
    }

    Ok(GoogleDriveBindingIngestion {
        binding_id: binding.binding_id.clone(),
        connector_id: connector.connector_id.clone(),
        folder_id: binding.native_source_id.clone(),
        files,
        skipped_files,
    })
}

#[derive(Debug)]
pub enum GoogleDriveIngestionError {
    TenantMismatch,
    ConnectorMismatch,
    UnsupportedConnectorProvider,
    UnsupportedSourceType,
    ConnectorNotActive,
    BindingNotEnabled,
    MissingCredentialRef,
    CredentialRefNotFound,
    CredentialNotReadOnly,
    CredentialResourceMismatch,
    Secret(SecretResolverError),
    Drive(GoogleDriveClientError),
}

impl fmt::Display for GoogleDriveIngestionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TenantMismatch => formatter.write_str("Google Drive binding tenant mismatch"),
            Self::ConnectorMismatch => formatter.write_str("Google Drive connector mismatch"),
            Self::UnsupportedConnectorProvider => {
                formatter.write_str("unsupported Google Drive connector provider")
            }
            Self::UnsupportedSourceType => {
                formatter.write_str("unsupported Google Drive source type")
            }
            Self::ConnectorNotActive => formatter.write_str("Google Drive connector is not active"),
            Self::BindingNotEnabled => {
                formatter.write_str("Google Drive source binding is not enabled")
            }
            Self::MissingCredentialRef => {
                formatter.write_str("Google Drive source binding has no credential ref")
            }
            Self::CredentialRefNotFound => {
                formatter.write_str("Google Drive credential ref was not found")
            }
            Self::CredentialNotReadOnly => {
                formatter.write_str("Google Drive credential ref is not read-only")
            }
            Self::CredentialResourceMismatch => formatter
                .write_str("Google Drive credential ref is not bound to the source resource"),
            Self::Secret(error) => {
                write!(formatter, "Google Drive secret resolution failed: {error}")
            }
            Self::Drive(error) => write!(formatter, "Google Drive read failed: {error}"),
        }
    }
}

impl std::error::Error for GoogleDriveIngestionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Secret(error) => Some(error),
            Self::Drive(error) => Some(error),
            Self::TenantMismatch
            | Self::ConnectorMismatch
            | Self::UnsupportedConnectorProvider
            | Self::UnsupportedSourceType
            | Self::ConnectorNotActive
            | Self::BindingNotEnabled
            | Self::MissingCredentialRef
            | Self::CredentialRefNotFound
            | Self::CredentialNotReadOnly
            | Self::CredentialResourceMismatch => None,
        }
    }
}

fn validate_google_drive_binding(
    tenant_context: &TenantContext,
    connector: &ConnectorInstance,
    binding: &SourceBinding,
) -> Result<(), GoogleDriveIngestionError> {
    if !connector.tenant_matches(tenant_context) || !binding.tenant_matches(tenant_context) {
        return Err(GoogleDriveIngestionError::TenantMismatch);
    }
    if binding.connector_id != connector.connector_id {
        return Err(GoogleDriveIngestionError::ConnectorMismatch);
    }
    if connector.provider != GOOGLE_DRIVE_PROVIDER {
        return Err(GoogleDriveIngestionError::UnsupportedConnectorProvider);
    }
    if binding.source_type != GOOGLE_DRIVE_SOURCE_TYPE {
        return Err(GoogleDriveIngestionError::UnsupportedSourceType);
    }
    if connector.state != ConnectorLifecycleState::Active {
        return Err(GoogleDriveIngestionError::ConnectorNotActive);
    }
    if binding.state != SourceBindingState::Enabled {
        return Err(GoogleDriveIngestionError::BindingNotEnabled);
    }
    Ok(())
}

async fn fetch_supported_file<C>(
    bearer_token: &str,
    drive_client: &C,
    file: &GoogleDriveFileMetadata,
) -> Result<Option<Vec<u8>>, GoogleDriveClientError>
where
    C: GoogleDriveReadClient,
{
    if file.mime_type == "application/vnd.google-apps.folder" {
        return Ok(None);
    }
    if file.mime_type.starts_with("application/vnd.google-apps.") {
        return drive_client
            .export_google_workspace_file(bearer_token, &file.id, "text/plain")
            .await
            .map(Some);
    }
    if file.mime_type.starts_with("text/")
        || matches!(
            file.name
                .rsplit('.')
                .next()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("md" | "markdown" | "mdx" | "txt")
        )
    {
        return drive_client
            .download_file_bytes(bearer_token, &file.id)
            .await
            .map(Some);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::{
        ConnectorCredentialRef, DataClass, PrincipalRef, ResourceKind, ResourceRef, SecretRef,
    };

    #[tokio::test]
    async fn preflight_resolves_secret_and_lists_bound_drive_folder() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let binding = test_binding(&tenant);
        let connector = test_connector(&tenant, &binding);
        let resolver = TestResolver;
        let drive = TestDriveClient;

        let preflight =
            preflight_google_drive_binding(&tenant, &connector, &binding, &resolver, &drive)
                .await
                .expect("preflight");

        assert_eq!(preflight.binding_id, "finance-drive");
        assert_eq!(preflight.connector_id, "google_drive");
        assert_eq!(preflight.folder_id, "drive-folder-123");
        assert_eq!(preflight.file_count, 1);
        assert_eq!(preflight.next_page_token.as_deref(), Some("next-token"));
    }

    #[tokio::test]
    async fn preflight_rejects_missing_or_unbound_credentials_before_resolving() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let mut binding = test_binding(&tenant);
        let connector = test_connector(&tenant, &binding);
        binding.credential_ref_id = None;

        let error = preflight_google_drive_binding(
            &tenant,
            &connector,
            &binding,
            &TestResolver,
            &TestDriveClient,
        )
        .await
        .expect_err("missing credential ref");
        assert!(matches!(
            error,
            GoogleDriveIngestionError::MissingCredentialRef
        ));

        let mut binding = test_binding(&tenant);
        binding.resource_ref.resource_id = "other-drive".to_string();
        let error = preflight_google_drive_binding(
            &tenant,
            &connector,
            &binding,
            &TestResolver,
            &TestDriveClient,
        )
        .await
        .expect_err("resource mismatch");
        assert!(matches!(
            error,
            GoogleDriveIngestionError::CredentialResourceMismatch
        ));
    }

    #[tokio::test]
    async fn preflight_rejects_inactive_connector_and_disabled_binding() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let mut binding = test_binding(&tenant);
        let mut connector = test_connector(&tenant, &binding);
        connector.state = ConnectorLifecycleState::Paused;

        let error = preflight_google_drive_binding(
            &tenant,
            &connector,
            &binding,
            &TestResolver,
            &TestDriveClient,
        )
        .await
        .expect_err("inactive connector");
        assert!(matches!(
            error,
            GoogleDriveIngestionError::ConnectorNotActive
        ));

        connector.state = ConnectorLifecycleState::Active;
        binding.state = SourceBindingState::Disabled;
        let error = preflight_google_drive_binding(
            &tenant,
            &connector,
            &binding,
            &TestResolver,
            &TestDriveClient,
        )
        .await
        .expect_err("disabled binding");
        assert!(matches!(
            error,
            GoogleDriveIngestionError::BindingNotEnabled
        ));
    }

    #[tokio::test]
    async fn fetch_files_downloads_supported_drive_objects() {
        let tenant = TenantContext::explicit("acme", "finance", None);
        let binding = test_binding(&tenant);
        let connector = test_connector(&tenant, &binding);

        let ingestion = fetch_google_drive_binding_files(
            &tenant,
            &connector,
            &binding,
            &TestResolver,
            &TestFetchDriveClient,
        )
        .await
        .expect("ingestion fetch");

        assert_eq!(ingestion.binding_id, "finance-drive");
        assert_eq!(ingestion.files.len(), 1);
        assert_eq!(ingestion.files[0].drive_file_id, "file-1");
        assert_eq!(ingestion.files[0].bytes, b"downloaded drive note");
        assert_eq!(ingestion.skipped_files, 0);
    }

    struct TestResolver;

    #[async_trait]
    impl SecretResolver for TestResolver {
        async fn resolve_bearer_token(
            &self,
            _tenant_context: &TenantContext,
            _credential_ref: &ConnectorCredentialRef,
        ) -> Result<super::super::secrets::ResolvedBearerToken, SecretResolverError> {
            Ok(super::super::secrets::ResolvedBearerToken::from_test_token(
                "drive-token",
            ))
        }
    }

    struct TestDriveClient;

    #[async_trait]
    impl GoogleDriveReadClient for TestDriveClient {
        async fn list_folder_children(
            &self,
            bearer_token: &str,
            folder_id: &str,
            page_token: Option<&str>,
        ) -> Result<GoogleDriveListPage, GoogleDriveClientError> {
            assert_eq!(bearer_token, "drive-token");
            assert_eq!(folder_id, "drive-folder-123");
            assert_eq!(page_token, None);
            Ok(GoogleDriveListPage {
                files: vec![super::super::google_drive::GoogleDriveFileMetadata {
                    id: "file-1".to_string(),
                    name: "Finance Note".to_string(),
                    mime_type: "text/plain".to_string(),
                    modified_time: None,
                    md5_checksum: None,
                    size: None,
                    web_view_link: None,
                }],
                next_page_token: Some("next-token".to_string()),
            })
        }

        async fn download_file_bytes(
            &self,
            _bearer_token: &str,
            _file_id: &str,
        ) -> Result<Vec<u8>, GoogleDriveClientError> {
            panic!("preflight fixture should not download file bytes")
        }

        async fn export_google_workspace_file(
            &self,
            _bearer_token: &str,
            _file_id: &str,
            _mime_type: &str,
        ) -> Result<Vec<u8>, GoogleDriveClientError> {
            panic!("text/plain fixture should download, not export")
        }
    }

    struct TestFetchDriveClient;

    #[async_trait]
    impl GoogleDriveReadClient for TestFetchDriveClient {
        async fn list_folder_children(
            &self,
            bearer_token: &str,
            folder_id: &str,
            page_token: Option<&str>,
        ) -> Result<GoogleDriveListPage, GoogleDriveClientError> {
            assert_eq!(bearer_token, "drive-token");
            assert_eq!(folder_id, "drive-folder-123");
            assert_eq!(page_token, None);
            Ok(GoogleDriveListPage {
                files: vec![super::super::google_drive::GoogleDriveFileMetadata {
                    id: "file-1".to_string(),
                    name: "Finance Note.txt".to_string(),
                    mime_type: "text/plain".to_string(),
                    modified_time: None,
                    md5_checksum: None,
                    size: None,
                    web_view_link: None,
                }],
                next_page_token: None,
            })
        }

        async fn download_file_bytes(
            &self,
            bearer_token: &str,
            file_id: &str,
        ) -> Result<Vec<u8>, GoogleDriveClientError> {
            assert_eq!(bearer_token, "drive-token");
            assert_eq!(file_id, "file-1");
            Ok(b"downloaded drive note".to_vec())
        }

        async fn export_google_workspace_file(
            &self,
            _bearer_token: &str,
            _file_id: &str,
            _mime_type: &str,
        ) -> Result<Vec<u8>, GoogleDriveClientError> {
            panic!("text/plain fixture should download, not export")
        }
    }

    fn test_connector(tenant: &TenantContext, binding: &SourceBinding) -> ConnectorInstance {
        ConnectorInstance::active(
            "google_drive",
            tenant.clone(),
            "google_drive",
            PrincipalRef::human_user("finance-admin"),
            1_000,
        )
        .with_credential_refs(vec![ConnectorCredentialRef {
            org_id: tenant.org_id.clone(),
            workspace_id: tenant.workspace_id.clone(),
            connector_id: "google_drive".to_string(),
            credential_id: "drive-readonly".to_string(),
            credential_class: ConnectorCredentialClass::ReadOnly,
            secret_ref: SecretRef {
                org_id: tenant.org_id.clone(),
                workspace_id: tenant.workspace_id.clone(),
                provider: "env".to_string(),
                secret_id: "env://TANDEM_TEST_GOOGLE_DRIVE_TOKEN".to_string(),
                name: "Local Drive token".to_string(),
            },
            source_bound_resource: Some(binding.resource_ref.clone()),
            created_at_ms: 1_000,
            rotated_at_ms: None,
            expires_at_ms: None,
        }])
    }

    fn test_binding(tenant: &TenantContext) -> SourceBinding {
        SourceBinding::enabled(
            "finance-drive",
            tenant.clone(),
            "google_drive",
            "google_drive",
            "drive-folder-123",
            ResourceRef {
                organization_id: tenant.org_id.clone(),
                workspace_id: tenant.workspace_id.clone(),
                project_id: None,
                resource_kind: ResourceKind::DocumentCollection,
                resource_id: "finance-drive".to_string(),
                parent_path: Vec::new(),
                branch_id: None,
                path_prefix: None,
            },
            DataClass::FinancialRecord,
            PrincipalRef::human_user("finance-admin"),
            1_000,
        )
        .with_credential_ref_id("drive-readonly")
    }
}
