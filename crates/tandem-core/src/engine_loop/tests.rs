use super::*;
use super::loop_guards::{parse_budget_override, HARD_TOOL_CALL_CEILING};
use crate::{EventBus, Storage};
use std::sync::{Mutex, OnceLock};
use tandem_types::{HostOs, PathStyle, PrewriteCoverageMode, PrewriteRequirements, ShellFamily, Session};
use uuid::Uuid;

fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env test lock")
}

mod suite_a;
mod suite_b;
