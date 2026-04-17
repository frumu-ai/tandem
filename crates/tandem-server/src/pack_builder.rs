use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use tandem_tools::Tool;
use tandem_types::{ToolResult, ToolSchema};

use crate::pack_manager::PackInstallRequest;
use crate::{
    mcp_catalog, AppState, RoutineMisfirePolicy, RoutineSchedule, RoutineSpec, RoutineStatus,
};

include!("pack_builder_parts/part01.rs");
include!("pack_builder_parts/part02.rs");
