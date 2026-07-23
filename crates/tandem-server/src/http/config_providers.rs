// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::http::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use std::net::IpAddr;
use tandem_types::{RequestPrincipal, TenantContext};

include!("config_providers_parts/part01.rs");
include!("config_providers_parts/part02.rs");
include!("config_providers_parts/part03.rs");
include!("config_providers_parts/part04.rs");
include!("config_providers_parts/part05.rs");
include!("config_providers_parts/part06.rs");
