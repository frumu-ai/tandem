// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::host::{PlannerLlmInvocation, PlannerLlmInvoker};
use crate::planner_types::PlannerInvocationFailure;
use crate::workflow_plan::truncate_text;

pub async fn invoke_planner_json<T, H>(
    host: &H,
    invocation: PlannerLlmInvocation,
) -> Result<T, PlannerInvocationFailure>
where
    T: DeserializeOwned,
    H: PlannerLlmInvoker,
{
    let payload = host.invoke_planner_llm(invocation).await?;
    parse_planner_json(payload)
}

pub fn parse_planner_json<T>(payload: Value) -> Result<T, PlannerInvocationFailure>
where
    T: DeserializeOwned,
{
    serde_json::from_value::<T>(payload).map_err(|error| PlannerInvocationFailure {
        reason: "invalid_json".to_string(),
        detail: Some(truncate_text(&error.to_string(), 500)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize)]
    struct Payload {
        action: String,
    }

    #[test]
    fn parse_planner_json_decodes_valid_payloads() {
        let parsed: Payload = parse_planner_json(json!({ "action": "keep" })).expect("parses");
        assert_eq!(parsed.action, "keep");
    }

    #[test]
    fn parse_planner_json_reports_invalid_json_with_bounded_detail() {
        let error = parse_planner_json::<Payload>(json!({ "wrong_key": true }))
            .expect_err("missing field fails");
        assert_eq!(error.reason, "invalid_json");
        let detail = error.detail.expect("detail present");
        assert!(detail.contains("action"), "detail names the missing field");
        assert!(detail.len() <= 504, "detail is truncated to ~500 chars");
    }
}
