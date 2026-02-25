use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::ToolSchema;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TeamCreateInput {
    pub team_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SendMessageType {
    Message,
    Broadcast,
    ShutdownRequest,
    ShutdownResponse,
    PlanApprovalResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SendMessageInput {
    #[serde(rename = "type")]
    pub message_type: SendMessageType,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub approve: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TaskCreateInput {
    pub subject: String,
    pub description: String,
    #[serde(rename = "activeForm", default, alias = "active_form")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TaskUpdateInput {
    #[serde(rename = "taskId", alias = "task_id")]
    pub task_id: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "activeForm", default, alias = "active_form")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "addBlocks", default, alias = "add_blocks")]
    pub add_blocks: Option<Vec<String>>,
    #[serde(rename = "addBlockedBy", default, alias = "add_blocked_by")]
    pub add_blocked_by: Option<Vec<String>>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TaskListInput {}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TaskInput {
    pub description: String,
    pub prompt: String,
    pub subagent_type: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub resume: Option<String>,
    #[serde(default)]
    pub run_in_background: Option<bool>,
    #[serde(default)]
    pub max_turns: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub team_name: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

pub fn compat_tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "TeamCreate".to_string(),
            description: "Create a coordinated team and shared task context.".to_string(),
            input_schema: team_create_schema(),
        },
        ToolSchema {
            name: "SendMessage".to_string(),
            description: "Send teammate messages and coordination protocol responses.".to_string(),
            input_schema: send_message_schema(),
        },
        ToolSchema {
            name: "TaskCreate".to_string(),
            description: "Create a task in the shared team task list.".to_string(),
            input_schema: task_create_schema(),
        },
        ToolSchema {
            name: "TaskUpdate".to_string(),
            description: "Update ownership/state/dependencies of a shared task.".to_string(),
            input_schema: task_update_schema(),
        },
        ToolSchema {
            name: "TaskList".to_string(),
            description: "List tasks from the shared task list.".to_string(),
            input_schema: task_list_schema(),
        },
        ToolSchema {
            name: "Task".to_string(),
            description: "Spawn a teammate task, optionally scoped to a team_name.".to_string(),
            input_schema: task_schema(),
        },
    ]
}

pub fn team_create_schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "properties":{
            "team_name":{
                "description":"Name for the new team to create.",
                "type":"string"
            },
            "description":{
                "description":"Team description/purpose.",
                "type":"string"
            },
            "agent_type":{
                "description":"Type/role of the team lead (e.g., \"researcher\", \"test-runner\"). Used for team file and inter-agent coordination.",
                "type":"string"
            }
        },
        "required":["team_name"],
        "additionalProperties":false
    })
}

pub fn send_message_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "type":{
                "type":"string",
                "enum":["message","broadcast","shutdown_request","shutdown_response","plan_approval_response"],
                "description":"Message type: \"message\" for DMs, \"broadcast\" to all teammates, \"shutdown_request\" to request shutdown, \"shutdown_response\" to respond to shutdown, \"plan_approval_response\" to approve/reject plans"
            },
            "recipient":{
                "type":"string",
                "description":"Agent name of the recipient (required for message, shutdown_request, plan_approval_response)"
            },
            "content":{
                "type":"string",
                "description":"Message text, reason, or feedback"
            },
            "summary":{
                "type":"string",
                "description":"A 5-10 word summary of the message, shown as a preview in the UI (required for message, broadcast)"
            },
            "request_id":{
                "type":"string",
                "description":"Request ID to respond to (required for shutdown_response, plan_approval_response)"
            },
            "approve":{
                "type":"boolean",
                "description":"Whether to approve the request (required for shutdown_response, plan_approval_response)"
            }
        },
        "required":["type"],
        "additionalProperties":false
    })
}

pub fn task_create_schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "properties":{
            "subject":{"type":"string","description":"A brief title for the task"},
            "description":{"type":"string","description":"A detailed description of what needs to be done"},
            "activeForm":{"type":"string","description":"Present continuous form shown in spinner when in_progress (e.g., \"Running tests\")"},
            "metadata":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}
        },
        "required":["subject","description"],
        "additionalProperties":false
    })
}

pub fn task_update_schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "properties":{
            "taskId":{"type":"string","description":"The ID of the task to update"},
            "subject":{"type":"string","description":"New subject for the task"},
            "description":{"type":"string","description":"New description for the task"},
            "activeForm":{"type":"string","description":"Present continuous form shown in spinner when in_progress (e.g., \"Running tests\")"},
            "status":{"anyOf":[{"type":"string","enum":["pending","in_progress","completed"]},{"type":"string","const":"deleted"}]},
            "addBlocks":{"type":"array","items":{"type":"string"}},
            "addBlockedBy":{"type":"array","items":{"type":"string"}},
            "owner":{"type":"string"},
            "metadata":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}
        },
        "required":["taskId"],
        "additionalProperties":false
    })
}

pub fn task_list_schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "properties":{},
        "additionalProperties":false
    })
}

pub fn task_schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "type":"object",
        "properties":{
            "description":{"type":"string","description":"A short (3-5 word) description of the task"},
            "prompt":{"type":"string","description":"The task for the agent to perform"},
            "subagent_type":{"type":"string","description":"The type of specialized agent to use for this task"},
            "model":{"type":"string","enum":["sonnet","opus","haiku"]},
            "resume":{"type":"string"},
            "run_in_background":{"type":"boolean"},
            "max_turns":{"type":"integer","exclusiveMinimum":0},
            "name":{"type":"string"},
            "team_name":{"type":"string","description":"Team name for spawning. Uses current team context if omitted."},
            "mode":{"type":"string","enum":["acceptEdits","bypassPermissions","default","delegate","dontAsk","plan"]}
        },
        "required":["description","prompt","subagent_type"],
        "additionalProperties":false
    })
}
