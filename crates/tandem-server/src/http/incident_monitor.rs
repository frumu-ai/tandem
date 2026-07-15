// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{
    incident_monitor_github, IncidentMonitorApprovalPolicy, IncidentMonitorConfig,
    IncidentMonitorDestinationConfig, IncidentMonitorDestinationKind,
    IncidentMonitorDestinationReadiness, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord, IncidentMonitorRouteConfig, IncidentMonitorRoutePreviewMatch,
    IncidentMonitorRoutePreviewResponse, IncidentMonitorSourceKind, IncidentMonitorSourceReadiness,
    IncidentMonitorSubmission,
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

include!("incident_monitor_parts/part01.rs");
include!("incident_monitor_parts/part05.rs");
include!("incident_monitor_parts/part06.rs");
include!("incident_monitor_parts/part08.rs");
include!("incident_monitor_parts/part07.rs");
include!("incident_monitor_parts/part02.rs");
include!("incident_monitor_parts/part03.rs");
include!("incident_monitor_parts/part04.rs");
include!("incident_monitor_parts/part09.rs");
include!("incident_monitor_parts/part10.rs");
include!("incident_monitor_parts/part11.rs");
include!("incident_monitor_parts/part12.rs");
include!("incident_monitor_parts/part13.rs");
include!("incident_monitor_parts/part14.rs");
include!("incident_monitor_parts/part15.rs");
include!("incident_monitor_parts/part16.rs");
