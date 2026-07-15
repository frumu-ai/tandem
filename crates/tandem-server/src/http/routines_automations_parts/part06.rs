// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn routine_for_tenant(
    state: &AppState,
    routine_id: &str,
    tenant_context: &TenantContext,
) -> Option<RoutineSpec> {
    state
        .get_routine_for_tenant(routine_id, tenant_context)
        .await
}

async fn routine_run_for_tenant(
    state: &AppState,
    run_id: &str,
    tenant_context: &TenantContext,
) -> Option<RoutineRunRecord> {
    state
        .get_routine_run_for_tenant(run_id, tenant_context)
        .await
}

pub(super) fn objective_from_args(args: &Value, routine_id: &str, entrypoint: &str) -> String {
    args.get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!("Execute automation '{routine_id}' with entrypoint '{entrypoint}'.")
        })
}

pub(super) fn success_criteria_from_args(args: &Value) -> Vec<String> {
    args.get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn mode_from_args(args: &Value) -> String {
    args.get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("standalone")
        .to_string()
}

pub(super) fn normalize_automation_mode(raw: Option<&str>) -> Result<String, String> {
    let value = raw.unwrap_or("standalone").trim();
    if value.is_empty() {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("standalone") {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("orchestrated") {
        return Ok("orchestrated".to_string());
    }
    Err("mode must be one of standalone|orchestrated".to_string())
}

pub(super) fn validate_model_spec_object(value: &Value, path: &str) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("{path} must be an object"))?;
    let provider_id = obj
        .get("provider_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.provider_id is required"))?;
    let model_id = obj
        .get("model_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.model_id is required"))?;
    if provider_id.is_empty() || model_id.is_empty() {
        return Err(format!(
            "{path}.provider_id and {path}.model_id are required"
        ));
    }
    Ok(())
}

pub(crate) fn validate_model_policy(value: &Value) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "model_policy must be an object".to_string())?;
    if let Some(default_model) = obj.get("default_model") {
        validate_model_spec_object(default_model, "model_policy.default_model")?;
    }
    if let Some(role_models) = obj.get("role_models") {
        let role_obj = role_models
            .as_object()
            .ok_or_else(|| "model_policy.role_models must be an object".to_string())?;
        for (role, spec) in role_obj {
            validate_model_spec_object(spec, &format!("model_policy.role_models.{role}"))?;
        }
    }
    Ok(())
}
