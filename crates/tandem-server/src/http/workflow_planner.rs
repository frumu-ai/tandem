#[path = "workflow_planner_parts/session_background.rs"]
mod session_background;
use session_background::{
    workflow_planner_session_message_background, workflow_planner_session_start_background,
};

include!("workflow_planner_parts/part01.rs");
include!("workflow_planner_parts/part03.rs");
include!("workflow_planner_parts/part02.rs");
