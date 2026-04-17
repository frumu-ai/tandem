use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use tokio::sync::{Mutex, RwLock};
use tokio::task;
use uuid::Uuid;

use tandem_types::{Message, MessagePart, MessageRole, Session};

use crate::message_part_reducer::reduce_message_parts;
use crate::{
    derive_session_title_from_prompt, normalize_workspace_path, title_needs_repair,
    workspace_project_id,
};

include!("storage_parts/part01.rs");
include!("storage_parts/part02.rs");
