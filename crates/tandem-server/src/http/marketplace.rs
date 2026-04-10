use super::*;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::{json, Value};
use std::path::{Path as StdPath, PathBuf};
use zip::ZipArchive;

use crate::http::global::sanitize_relative_subpath;

#[derive(Debug, Deserialize)]
pub(super) struct MarketplacePackPath {
    pub pack_id: String,
    pub path: String,
}

pub(super) async fn marketplace_catalog(
    State(_state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    match load_marketplace_catalog().await {
        Ok((value, _catalog_path)) => Ok(Json(value)),
        Err(StatusCode::NOT_FOUND) => Ok(Json(json!({
            "schema_version": "1",
            "generated_at": null,
            "packs": [],
        }))),
        Err(status) => Err(status),
    }
}

pub(super) async fn marketplace_pack_file_get(
    State(_state): State<AppState>,
    Path(MarketplacePackPath { pack_id, path }): Path<MarketplacePackPath>,
) -> Result<Response, StatusCode> {
    let rel = sanitize_relative_subpath(Some(&path))?;
    let (catalog, catalog_path) = load_marketplace_catalog().await.map_err(|err| {
        tracing::warn!("marketplace catalog load failed: {}", err);
        err
    })?;
    let pack = find_marketplace_pack(&catalog, &pack_id).ok_or(StatusCode::NOT_FOUND)?;
    let zip_path =
        resolve_marketplace_zip_path(&pack, &catalog_path).ok_or(StatusCode::NOT_FOUND)?;
    if !zip_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    let body = read_zip_entry(&zip_path, &rel).map_err(|status| {
        if status == StatusCode::NOT_FOUND {
            status
        } else {
            tracing::warn!(
                "marketplace file read failed for {}: {:?}",
                zip_path.display(),
                status
            );
            status
        }
    })?;
    let mime = content_type_for_path(&rel);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

async fn load_marketplace_catalog() -> Result<(Value, PathBuf), StatusCode> {
    let catalog_path = resolve_marketplace_catalog_path();
    if !catalog_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    let raw = tokio::fs::read_to_string(&catalog_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut catalog: Value =
        serde_json::from_str(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    augment_catalog(&mut catalog, &catalog_path);
    Ok((catalog, catalog_path))
}

fn resolve_marketplace_catalog_path() -> PathBuf {
    if let Ok(path) = std::env::var("TANDEM_MARKETPLACE_CATALOG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    std::env::current_dir()
        .map(|cwd| cwd.join("docs/internal/marketplace/dist/catalog.json"))
        .unwrap_or_else(|_| PathBuf::from("docs/internal/marketplace/dist/catalog.json"))
}

fn augment_catalog(catalog: &mut Value, catalog_path: &StdPath) {
    let Some(dir) = catalog_path.parent() else {
        return;
    };
    let Some(packs) = catalog.get_mut("packs").and_then(|v| v.as_array_mut()) else {
        return;
    };
    for pack in packs {
        let Some(dist) = pack.get_mut("distribution").and_then(|v| v.as_object_mut()) else {
            continue;
        };
        if let Some(download_url) = dist
            .get("download_url")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let zip_path = dir.join(download_url);
            dist.insert(
                "zip_path".to_string(),
                Value::String(zip_path.to_string_lossy().to_string()),
            );
        }
    }
}

fn find_marketplace_pack<'a>(catalog: &'a Value, pack_id: &str) -> Option<&'a Value> {
    catalog
        .get("packs")
        .and_then(|v| v.as_array())
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("pack_id")
                    .and_then(|v| v.as_str())
                    .map(|value| value == pack_id)
                    .unwrap_or(false)
            })
        })
}

fn resolve_marketplace_zip_path(pack: &Value, catalog_path: &StdPath) -> Option<PathBuf> {
    let dist = pack.get("distribution")?.as_object()?;
    if let Some(path) = dist.get("zip_path").and_then(|v| v.as_str()) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let download_url = dist.get("download_url")?.as_str()?.trim();
    if download_url.is_empty() {
        return None;
    }
    catalog_path.parent().map(|dir| dir.join(download_url))
}

fn read_zip_entry(zip_path: &StdPath, rel: &StdPath) -> Result<Vec<u8>, StatusCode> {
    let file = std::fs::File::open(zip_path).map_err(|_| StatusCode::NOT_FOUND)?;
    let mut archive = ZipArchive::new(file).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let name = rel.to_string_lossy().replace('\\', "/");
    let mut entry = archive.by_name(&name).map_err(|_| StatusCode::NOT_FOUND)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(bytes)
}

fn content_type_for_path(path: &StdPath) -> &'static str {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" => "application/json; charset=utf-8",
        "yaml" | "yml" => "text/yaml; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
