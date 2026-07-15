// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::Deserialize;
use std::fmt;

const DEFAULT_GOOGLE_API_BASE_URL: &str = "https://www.googleapis.com";
const DRIVE_FILES_PATH: &str = "/drive/v3/files";
const DRIVE_LIST_FIELDS: &str =
    "nextPageToken, files(id,name,mimeType,modifiedTime,md5Checksum,size,webViewLink)";

#[derive(Debug, Clone)]
pub struct GoogleDriveClient {
    http: reqwest::Client,
    api_base_url: String,
}

impl Default for GoogleDriveClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleDriveClient {
    pub fn new() -> Self {
        Self::new_with_base_url(DEFAULT_GOOGLE_API_BASE_URL)
    }

    pub fn new_with_base_url(api_base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_base_url: api_base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn new_from_env() -> Self {
        std::env::var("TANDEM_GOOGLE_DRIVE_API_BASE_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(Self::new_with_base_url)
            .unwrap_or_else(Self::new)
    }

    pub async fn list_folder_children(
        &self,
        bearer_token: &str,
        folder_id: &str,
        page_token: Option<&str>,
    ) -> Result<GoogleDriveListPage, GoogleDriveClientError> {
        let bearer_token = checked_bearer_token(bearer_token)?;
        let folder_id = checked_drive_id("folder_id", folder_id)?;
        let query = format!(
            "'{}' in parents and trashed = false",
            escape_drive_query_value(folder_id)
        );
        let mut request = self
            .http
            .get(format!("{}{}", self.api_base_url, DRIVE_FILES_PATH))
            .header(AUTHORIZATION, format!("Bearer {bearer_token}"))
            .header(ACCEPT, "application/json")
            .query(&[
                ("q", query.as_str()),
                ("fields", DRIVE_LIST_FIELDS),
                ("pageSize", "100"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ]);
        if let Some(page_token) = page_token.filter(|value| !value.trim().is_empty()) {
            request = request.query(&[("pageToken", page_token.trim())]);
        }
        let response = request
            .send()
            .await
            .map_err(GoogleDriveClientError::Transport)?;
        parse_json_response(response).await
    }

    pub async fn download_file_bytes(
        &self,
        bearer_token: &str,
        file_id: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError> {
        let bearer_token = checked_bearer_token(bearer_token)?;
        let file_id = checked_drive_id("file_id", file_id)?;
        let response = self
            .http
            .get(format!(
                "{}{}/{}",
                self.api_base_url,
                DRIVE_FILES_PATH,
                urlencoding::encode(file_id)
            ))
            .header(AUTHORIZATION, format!("Bearer {bearer_token}"))
            .query(&[("alt", "media")])
            .send()
            .await
            .map_err(GoogleDriveClientError::Transport)?;
        parse_bytes_response(response).await
    }

    pub async fn export_google_workspace_file(
        &self,
        bearer_token: &str,
        file_id: &str,
        mime_type: &str,
    ) -> Result<Vec<u8>, GoogleDriveClientError> {
        let bearer_token = checked_bearer_token(bearer_token)?;
        let file_id = checked_drive_id("file_id", file_id)?;
        let mime_type = checked_mime_type(mime_type)?;
        let response = self
            .http
            .get(format!(
                "{}{}/{}/export",
                self.api_base_url,
                DRIVE_FILES_PATH,
                urlencoding::encode(file_id)
            ))
            .header(AUTHORIZATION, format!("Bearer {bearer_token}"))
            .query(&[("mimeType", mime_type)])
            .send()
            .await
            .map_err(GoogleDriveClientError::Transport)?;
        parse_bytes_response(response).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveListPage {
    #[serde(default)]
    pub files: Vec<GoogleDriveFileMetadata>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveFileMetadata {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    #[serde(default)]
    pub modified_time: Option<String>,
    #[serde(default)]
    pub md5_checksum: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub web_view_link: Option<String>,
}

#[derive(Debug)]
pub enum GoogleDriveClientError {
    InvalidInput(&'static str),
    Transport(reqwest::Error),
    Http { status: reqwest::StatusCode },
    Decode(reqwest::Error),
}

impl fmt::Display for GoogleDriveClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(field) => write!(formatter, "invalid Google Drive {field}"),
            Self::Transport(_) => write!(formatter, "Google Drive request failed"),
            Self::Http { status } => write!(formatter, "Google Drive returned HTTP {status}"),
            Self::Decode(_) => write!(formatter, "Google Drive response decode failed"),
        }
    }
}

impl std::error::Error for GoogleDriveClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) | Self::Decode(error) => Some(error),
            Self::InvalidInput(_) | Self::Http { .. } => None,
        }
    }
}

async fn parse_json_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, GoogleDriveClientError> {
    let status = response.status();
    if !status.is_success() {
        return Err(GoogleDriveClientError::Http { status });
    }
    response
        .json::<T>()
        .await
        .map_err(GoogleDriveClientError::Decode)
}

async fn parse_bytes_response(
    response: reqwest::Response,
) -> Result<Vec<u8>, GoogleDriveClientError> {
    let status = response.status();
    if !status.is_success() {
        return Err(GoogleDriveClientError::Http { status });
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(GoogleDriveClientError::Decode)
}

fn checked_bearer_token(value: &str) -> Result<&str, GoogleDriveClientError> {
    let value = value.trim();
    if value.is_empty() {
        Err(GoogleDriveClientError::InvalidInput("bearer_token"))
    } else {
        Ok(value)
    }
}

fn checked_drive_id<'a>(
    field: &'static str,
    value: &'a str,
) -> Result<&'a str, GoogleDriveClientError> {
    let value = value.trim();
    if value.is_empty() || value.contains('/') || value.contains('\\') {
        Err(GoogleDriveClientError::InvalidInput(field))
    } else {
        Ok(value)
    }
}

fn checked_mime_type(value: &str) -> Result<&str, GoogleDriveClientError> {
    let value = value.trim();
    if value.is_empty() || !value.contains('/') {
        Err(GoogleDriveClientError::InvalidInput("mime_type"))
    } else {
        Ok(value)
    }
}

fn escape_drive_query_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::extract::{Path, Query};
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::get;
    use axum::{Json, Router};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn list_folder_children_uses_read_only_drive_query() {
        let base_url = spawn_drive_fixture().await;
        let client = GoogleDriveClient::new_with_base_url(base_url);

        let page = client
            .list_folder_children("token-123", "folder-123", Some("next-page"))
            .await
            .expect("list page");

        assert_eq!(page.next_page_token.as_deref(), Some("next-token"));
        assert_eq!(page.files.len(), 1);
        assert_eq!(page.files[0].id, "file-1");
        assert_eq!(page.files[0].mime_type, "text/plain");
    }

    #[tokio::test]
    async fn download_and_export_use_expected_drive_endpoints() {
        let base_url = spawn_drive_fixture().await;
        let client = GoogleDriveClient::new_with_base_url(base_url);

        let bytes = client
            .download_file_bytes("token-123", "file-1")
            .await
            .expect("download bytes");
        assert_eq!(bytes, b"plain file bytes");

        let exported = client
            .export_google_workspace_file("token-123", "doc-1", "text/plain")
            .await
            .expect("export bytes");
        assert_eq!(exported, b"exported workspace doc");
    }

    #[tokio::test]
    async fn rejects_empty_tokens_and_path_like_ids() {
        let client = GoogleDriveClient::new_with_base_url("http://127.0.0.1:9");

        assert!(matches!(
            client.list_folder_children(" ", "folder-123", None).await,
            Err(GoogleDriveClientError::InvalidInput("bearer_token"))
        ));
        assert!(matches!(
            client.download_file_bytes("token-123", "../secret").await,
            Err(GoogleDriveClientError::InvalidInput("file_id"))
        ));
    }

    async fn spawn_drive_fixture() -> String {
        let app = Router::new()
            .route("/drive/v3/files", get(list_files))
            .route("/drive/v3/files/{file_id}", get(download_file))
            .route("/drive/v3/files/{file_id}/export", get(export_file));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let addr = listener.local_addr().expect("fixture addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("fixture server");
        });
        format!("http://{addr}")
    }

    async fn list_files(
        headers: HeaderMap,
        Query(query): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer token-123")
        );
        assert_eq!(
            query.get("q").map(String::as_str),
            Some("'folder-123' in parents and trashed = false")
        );
        assert_eq!(
            query.get("pageToken").map(String::as_str),
            Some("next-page")
        );
        assert!(query
            .get("fields")
            .is_some_and(|fields| fields.contains("files(id,name,mimeType")));
        Json(json!({
            "nextPageToken": "next-token",
            "files": [{
                "id": "file-1",
                "name": "Note.txt",
                "mimeType": "text/plain",
                "modifiedTime": "2026-05-22T00:00:00Z",
                "md5Checksum": "abc123",
                "size": "16",
                "webViewLink": "https://drive.google.com/file/d/file-1/view"
            }]
        }))
    }

    async fn download_file(
        headers: HeaderMap,
        Path(file_id): Path<String>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Result<Bytes, StatusCode> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer token-123")
        );
        assert_eq!(file_id, "file-1");
        assert_eq!(query.get("alt").map(String::as_str), Some("media"));
        Ok(Bytes::from_static(b"plain file bytes"))
    }

    async fn export_file(
        headers: HeaderMap,
        Path(file_id): Path<String>,
        Query(query): Query<HashMap<String, String>>,
    ) -> Result<Bytes, StatusCode> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer token-123")
        );
        assert_eq!(file_id, "doc-1");
        assert_eq!(
            query.get("mimeType").map(String::as_str),
            Some("text/plain")
        );
        Ok(Bytes::from_static(b"exported workspace doc"))
    }
}
