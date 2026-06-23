use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{
    bug_monitor_github, BugMonitorApprovalPolicy, BugMonitorConfig, BugMonitorDestinationConfig,
    BugMonitorDestinationReadiness, BugMonitorDraftRecord, BugMonitorIncidentRecord,
    BugMonitorRouteConfig, BugMonitorRoutePreviewMatch, BugMonitorRoutePreviewResponse,
    BugMonitorSubmission,
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

include!("bug_monitor_parts/part01.rs");
include!("bug_monitor_parts/part05.rs");
include!("bug_monitor_parts/part06.rs");
include!("bug_monitor_parts/part02.rs");
include!("bug_monitor_parts/part03.rs");
include!("bug_monitor_parts/part04.rs");
