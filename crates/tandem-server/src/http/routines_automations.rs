// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use tandem_types::{RequestPrincipal, TenantContext, VerifiedTenantContext};

mod events_authority;
use events_authority::guard_automation_events;

include!("routines_automations_parts/part01.rs");
include!("routines_automations_parts/part07.rs");
include!("routines_automations_parts/part06.rs");
include!("routines_automations_parts/part05.rs");
include!("routines_automations_parts/part02.rs");
include!("routines_automations_parts/part04.rs");
include!("routines_automations_parts/part03.rs");
include!("routines_automations_parts/part08.rs");

#[cfg(test)]
#[path = "routines_automations/tests/events_authority_tests.rs"]
mod events_authority_tests;
