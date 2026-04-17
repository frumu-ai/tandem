use crate::http::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};

include!("config_providers_parts/part01.rs");
include!("config_providers_parts/part02.rs");
