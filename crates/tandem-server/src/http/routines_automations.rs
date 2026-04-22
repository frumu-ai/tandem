use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use tandem_types::{RequestPrincipal, TenantContext};

include!("routines_automations_parts/part01.rs");
include!("routines_automations_parts/part02.rs");
include!("routines_automations_parts/part03.rs");
