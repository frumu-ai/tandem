use axum::body::Body;
use axum::http::header;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

static ADMIN_HTML: &str = include_str!("admin.html");

const CSP_HEADER: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

pub fn web_ui_router<S>(prefix: &str) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let base = normalize_prefix(prefix);
    let wildcard = format!("{}/{{*path}}", base);
    Router::new()
        .route(&base, get(serve_index))
        .route(&format!("{}/", base), get(serve_index))
        .route(&wildcard, get(serve_index))
}

async fn serve_index() -> impl IntoResponse {
    let mut response = Response::new(Body::from(ADMIN_HTML));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CSP_HEADER),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn normalize_prefix(prefix: &str) -> String {
    let raw = prefix.trim();
    if raw.is_empty() || raw == "/" {
        return "/admin".to_string();
    }
    let with_leading = if raw.starts_with('/') {
        raw.to_string()
    } else {
        format!("/{raw}")
    };
    with_leading.trim_end_matches('/').to_string()
}
