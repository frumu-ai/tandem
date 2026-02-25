pub mod compat;
pub mod paths;

pub use compat::{
    compat_tool_schemas, SendMessageInput, SendMessageType, TaskCreateInput, TaskInput,
    TaskListInput, TaskUpdateInput, TeamCreateInput,
};
pub use paths::AgentTeamPaths;
