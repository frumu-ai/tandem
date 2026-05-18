use crate::http::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use tandem_types::{RequestPrincipal, TenantContext};

include!("config_providers_parts/part01.rs");
include!("config_providers_parts/part02.rs");
