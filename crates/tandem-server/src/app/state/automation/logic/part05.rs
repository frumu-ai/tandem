
#[path = "../tasks.rs"]
pub mod tasks;

pub async fn run_automation_v2_executor(state: AppState) {
    tasks::run_automation_v2_executor(state).await
}
