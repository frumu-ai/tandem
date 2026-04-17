use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};

include!("routines_automations_parts/part01.rs");
include!("routines_automations_parts/part02.rs");
include!("routines_automations_parts/part03.rs");
