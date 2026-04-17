use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{bug_monitor_github, BugMonitorConfig, BugMonitorDraftRecord, BugMonitorSubmission};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

include!("bug_monitor_parts/part01.rs");
include!("bug_monitor_parts/part02.rs");
