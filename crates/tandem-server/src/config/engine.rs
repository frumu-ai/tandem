// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeSet;
use std::path::PathBuf;

use base64::Engine;
use serde::Serialize;
use serde_json::json;
use tandem_types::RuntimeAuthMode;

const RUN_STALE_DEFAULT_MS: u64 = 120_000;
const RUN_STALE_MIN_MS: u64 = 30_000;
const RUN_STALE_MAX_MS: u64 = 600_000;
const EXECUTE_NODE_TIMEOUT_DEFAULT_MS: u64 = 1_800_000;
const EXECUTE_NODE_TIMEOUT_MIN_MS: u64 = 180_000;
const EXECUTE_NODE_TIMEOUT_MAX_MS: u64 = 3_600_000;
const CONTEXT_ASSERTION_FUTURE_SKEW_DEFAULT_MS: u64 = 10_000;
const CONTEXT_ASSERTION_FUTURE_SKEW_MIN_MS: u64 = 10_000;
const CONTEXT_ASSERTION_FUTURE_SKEW_MAX_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct EngineConfigOptions {
    pub cli_transport_token_configured: bool,
    pub unsafe_no_api_token: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineConfig {
    pub runtime_auth_mode: RuntimeAuthMode,
    pub run_stale_ms: u64,
    pub token_cost_per_1k_usd: f64,
    pub automation_strict_research_quality: bool,
    pub automation_quality_legacy_rollback: bool,
    pub automation_execute_node_timeout_ms: u64,
    pub scheduler_mode: &'static str,
    pub scheduler_max_concurrent_runs: usize,
    pub scheduler_shutdown_timeout_secs: u64,
    pub context_assertion_max_future_skew_ms: u64,
    pub context_assertion_verifier_configured: bool,
    pub hosted_control_plane_configured: bool,
    pub cross_tenant_grant_signing_key_configured: bool,
    pub audit_hmac_key_configured: bool,
    pub transport_token_configured: bool,
    pub unsafe_no_api_token: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineConfigReport {
    pub config: EngineConfig,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl EngineConfigReport {
    pub fn from_env(options: EngineConfigOptions) -> Self {
        let mut errors = Vec::new();
        let mut warnings = unknown_tandem_env_warnings();
        let runtime_auth_mode = parse_runtime_auth_mode(&mut errors);
        validate_context_assertion_public_keys(&mut errors);
        let unsafe_no_api_token = options.unsafe_no_api_token
            || parse_bool_env("TANDEM_UNSAFE_NO_API_TOKEN", false, &mut errors);
        let transport_token_configured = options.cli_transport_token_configured
            || env_value_present("TANDEM_API_TOKEN")
            || api_token_file_configured(&mut errors);

        let config = EngineConfig {
            runtime_auth_mode,
            run_stale_ms: parse_u64_env(
                "TANDEM_RUN_STALE_MS",
                RUN_STALE_DEFAULT_MS,
                Some((RUN_STALE_MIN_MS, RUN_STALE_MAX_MS)),
                &mut errors,
            ),
            token_cost_per_1k_usd: parse_non_negative_f64_env(
                "TANDEM_TOKEN_COST_PER_1K_USD",
                0.0,
                &mut errors,
            ),
            automation_strict_research_quality: parse_bool_env(
                "TANDEM_AUTOMATION_STRICT_RESEARCH_QUALITY",
                true,
                &mut errors,
            ),
            automation_quality_legacy_rollback: parse_bool_env(
                "TANDEM_AUTOMATION_QUALITY_LEGACY_ROLLBACK",
                false,
                &mut errors,
            ),
            automation_execute_node_timeout_ms: parse_u64_env(
                "TANDEM_AUTOMATION_EXECUTE_NODE_TIMEOUT_MS",
                EXECUTE_NODE_TIMEOUT_DEFAULT_MS,
                Some((EXECUTE_NODE_TIMEOUT_MIN_MS, EXECUTE_NODE_TIMEOUT_MAX_MS)),
                &mut errors,
            ),
            scheduler_mode: parse_scheduler_mode(&mut errors),
            scheduler_max_concurrent_runs: parse_usize_env(
                "TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS",
                8,
                &mut errors,
            ),
            scheduler_shutdown_timeout_secs: parse_u64_env(
                "TANDEM_SCHEDULER_SHUTDOWN_TIMEOUT_SECS",
                30,
                None,
                &mut errors,
            ),
            context_assertion_max_future_skew_ms: parse_u64_env(
                "TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS",
                CONTEXT_ASSERTION_FUTURE_SKEW_DEFAULT_MS,
                Some((
                    CONTEXT_ASSERTION_FUTURE_SKEW_MIN_MS,
                    CONTEXT_ASSERTION_FUTURE_SKEW_MAX_MS,
                )),
                &mut errors,
            ),
            context_assertion_verifier_configured:
                super::env::context_assertion_verifier_configured(),
            hosted_control_plane_configured: super::env::hosted_control_plane_configured(),
            cross_tenant_grant_signing_key_configured:
                super::env::cross_tenant_grant_signing_key_configured(),
            audit_hmac_key_configured: super::env::audit_hmac_key_configured(),
            transport_token_configured,
            unsafe_no_api_token,
        };

        validate_data_boundary_config(&mut errors);
        validate_storage_backend_config(&mut errors);
        validate_security_invariants(&config, &mut errors);
        warnings.sort();
        warnings.dedup();
        errors.sort();
        errors.dedup();
        Self {
            config,
            warnings,
            errors,
        }
    }

    pub fn ensure_valid(&self) -> anyhow::Result<()> {
        if self.errors.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "invalid Tandem engine configuration:\n{}",
            self.errors.join("\n")
        );
    }

    pub fn masked_json(&self) -> serde_json::Value {
        json!({
            "config": self.config,
            "warnings": self.warnings,
            "errors": self.errors,
        })
    }

    pub fn human_summary(&self) -> String {
        let mut lines = vec![
            "Tandem engine configuration".to_string(),
            format!(
                "- runtime_auth_mode: {}",
                self.config.runtime_auth_mode.as_str()
            ),
            format!(
                "- transport_token_configured: {}",
                self.config.transport_token_configured
            ),
            format!(
                "- context_assertion_verifier_configured: {}",
                self.config.context_assertion_verifier_configured
            ),
            format!(
                "- hosted_control_plane_configured: {}",
                self.config.hosted_control_plane_configured
            ),
            format!(
                "- cross_tenant_grant_signing_key_configured: {}",
                self.config.cross_tenant_grant_signing_key_configured
            ),
            format!(
                "- audit_hmac_key_configured: {}",
                self.config.audit_hmac_key_configured
            ),
            format!(
                "- automation_execute_node_timeout_ms: {}",
                self.config.automation_execute_node_timeout_ms
            ),
            format!("- scheduler_mode: {}", self.config.scheduler_mode),
        ];
        if !self.warnings.is_empty() {
            lines.push("Warnings:".to_string());
            lines.extend(self.warnings.iter().map(|warning| format!("- {warning}")));
        }
        if !self.errors.is_empty() {
            lines.push("Errors:".to_string());
            lines.extend(self.errors.iter().map(|error| format!("- {error}")));
        } else {
            lines.push("OK: configuration is valid.".to_string());
        }
        lines.join("\n")
    }
}

pub fn config_reference_markdown() -> String {
    let mut out = String::from(
        "# Tandem Engine Configuration Reference\n\nThis page is generated from the engine config registry used by `tandem-engine config check`.\n\n| Variable | Default | Notes |\n| --- | --- | --- |\n",
    );
    for var in CONFIG_VARS {
        out.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            var.name, var.default, var.notes
        ));
    }
    out.push_str(
        "\n`tandem-engine config check` validates these startup invariants before the server binds:\n\n\
- Hosted or enterprise auth mode requires a context assertion verifier keyring.\n\
- Hosted or enterprise auth mode requires an explicit transport token from `TANDEM_API_TOKEN`, `TANDEM_API_TOKEN_FILE`, or `--api-token`.\n\
- Hosted or enterprise auth mode rejects `TANDEM_UNSAFE_NO_API_TOKEN`.\n\
- Malformed verifier key material, invalid booleans, invalid modes, and out-of-range numeric settings fail fast.\n\
- Unknown `TANDEM_*` variables are reported as warnings to catch typos without blocking local startup.\n\n\
Predicate-governed decisions and enterprise-authored exact-action approvals additionally fail closed at decision time in hosted/enterprise mode unless `TANDEM_AUDIT_HMAC_KEY` or `TANDEM_AUDIT_HMAC_KEY_FILE` is configured.\n",
    );
    out
}

fn validate_security_invariants(config: &EngineConfig, errors: &mut Vec<String>) {
    let hosted_or_enterprise = config.runtime_auth_mode != RuntimeAuthMode::LocalSingleTenant
        || config.hosted_control_plane_configured;
    if hosted_or_enterprise && !config.context_assertion_verifier_configured {
        errors.push(
            "TANDEM_RUNTIME_AUTH_MODE hosted/enterprise requires TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS or *_FILE"
                .to_string(),
        );
    }
    if hosted_or_enterprise && !config.transport_token_configured {
        errors.push(
            "hosted/enterprise runtime requires TANDEM_API_TOKEN, TANDEM_API_TOKEN_FILE, or --api-token"
                .to_string(),
        );
    }
    if hosted_or_enterprise && config.unsafe_no_api_token {
        errors.push(
            "hosted/enterprise runtime cannot use TANDEM_UNSAFE_NO_API_TOKEN or --unsafe-no-api-token"
                .to_string(),
        );
    }
}

fn parse_runtime_auth_mode(errors: &mut Vec<String>) -> RuntimeAuthMode {
    match std::env::var("TANDEM_RUNTIME_AUTH_MODE") {
        Ok(value) if !value.trim().is_empty() => match RuntimeAuthMode::parse(&value) {
            Ok(mode) => mode,
            Err(_) => {
                errors.push(format!(
                    "TANDEM_RUNTIME_AUTH_MODE has invalid value `{}`; expected local_single_tenant, hosted_single_tenant, or enterprise_required",
                    value.trim()
                ));
                RuntimeAuthMode::LocalSingleTenant
            }
        },
        _ => RuntimeAuthMode::LocalSingleTenant,
    }
}

fn parse_scheduler_mode(errors: &mut Vec<String>) -> &'static str {
    match std::env::var("TANDEM_SCHEDULER_MODE") {
        Ok(value) if !value.trim().is_empty() => match value.trim().to_ascii_lowercase().as_str() {
            "single" => "single",
            "multi" => "multi",
            _ => {
                errors.push(format!(
                    "TANDEM_SCHEDULER_MODE has invalid value `{}`; expected single or multi",
                    value.trim()
                ));
                "multi"
            }
        },
        _ => "multi",
    }
}

/// TAN-714: the stateful store backend selection is fail-closed. A typo'd
/// backend name or a postgres selection without a URL (or without the
/// compiled backend) must stop startup instead of silently running SQLite.
fn validate_storage_backend_config(errors: &mut Vec<String>) {
    match crate::stateful_runtime::backend::StorageBackendConfig::from_env() {
        Err(error) => errors.push(error.to_string()),
        Ok(crate::stateful_runtime::backend::StorageBackendConfig::Sqlite) => {
            if !cfg!(feature = "storage-sqlite") {
                errors.push(
                    "storage backend `sqlite` requested but this build omits the storage-sqlite \
                     feature; set TANDEM_STORAGE_BACKEND=postgres or rebuild with SQLite support"
                        .to_string(),
                );
            }
        }
        Ok(crate::stateful_runtime::backend::StorageBackendConfig::Postgres { .. }) => {
            if !cfg!(feature = "storage-postgres") {
                errors.push(
                    "storage backend `postgres` requested but this build omits the \
                     storage-postgres feature"
                        .to_string(),
                );
            }
        }
    }
}

/// TAN-389: `TANDEM_DATA_BOUNDARY_*` values are parsed leniently at the
/// engine-loop call site (tandem-core reads env directly per the tunables
/// convention), so bad values must be rejected here at startup — otherwise a
/// typo'd `enforce` or class name would silently weaken a security policy.
fn validate_data_boundary_config(errors: &mut Vec<String>) {
    if let Ok(value) = std::env::var("TANDEM_DATA_BOUNDARY_MODE") {
        if !value.trim().is_empty()
            && tandem_data_boundary::DataBoundaryMode::parse(&value).is_none()
        {
            errors.push(format!(
                "TANDEM_DATA_BOUNDARY_MODE has invalid value `{}`; expected off, audit, or enforce",
                value.trim()
            ));
        }
    }
    if let Ok(value) = std::env::var("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY") {
        let normalized = value.trim().to_ascii_lowercase();
        if !normalized.is_empty()
            && !matches!(
                normalized.as_str(),
                "allow"
                    | "audit"
                    | "redact"
                    | "approval"
                    | "require_local"
                    | "required_local"
                    | "block"
            )
        {
            errors.push(format!(
                "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY has invalid value `{}`; expected allow, audit, redact, approval, require_local, or block",
                value.trim()
            ));
        }
    }
    for var in [
        "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES",
        "TANDEM_DATA_BOUNDARY_REDACT_CLASSES",
        "TANDEM_DATA_BOUNDARY_BLOCK_CLASSES",
    ] {
        let Ok(value) = std::env::var(var) else {
            continue;
        };
        for entry in value.split(',') {
            let entry = entry.trim();
            if !entry.is_empty() && tandem_data_boundary::SensitiveDataClass::parse(entry).is_none()
            {
                errors.push(format!(
                    "{var} contains unknown sensitive data class `{entry}`"
                ));
            }
        }
    }
    if let Ok(value) = std::env::var("TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES") {
        for entry in value.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let valid = entry.split_once('=').is_some_and(|(id, class)| {
                !id.trim().is_empty()
                    && tandem_data_boundary::ProviderBoundaryClass::parse(class).is_some()
            });
            if !valid {
                errors.push(format!(
                    "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES entry `{entry}` is invalid; expected provider_id=local|customer_hosted|approved_external|unapproved_external|prohibited|unknown"
                ));
            }
        }
    }
    if let Ok(value) = std::env::var("TANDEM_DATA_BOUNDARY_STRICT") {
        let normalized = value.trim().to_ascii_lowercase();
        if !normalized.is_empty()
            && !matches!(
                normalized.as_str(),
                "1" | "true" | "yes" | "on" | "0" | "false" | "no" | "off"
            )
        {
            errors.push(format!(
                "TANDEM_DATA_BOUNDARY_STRICT has invalid value `{}`; expected a boolean",
                value.trim()
            ));
        }
    }
    if let Ok(value) = std::env::var("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES") {
        if !value.trim().is_empty()
            && value
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|bytes| *bytes > 0)
                .is_none()
        {
            errors.push(format!(
                "TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES has invalid value `{}`; expected a positive integer",
                value.trim()
            ));
        }
    }
}

fn parse_bool_env(name: &str, default: bool, errors: &mut Vec<String>) -> bool {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => {
                errors.push(format!(
                    "{name} has invalid boolean value `{}`; expected true/false, yes/no, on/off, or 1/0",
                    value.trim()
                ));
                default
            }
        },
        _ => default,
    }
}

fn parse_u64_env(
    name: &str,
    default: u64,
    range: Option<(u64, u64)>,
    errors: &mut Vec<String>,
) -> u64 {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => match value.trim().parse::<u64>() {
            Ok(parsed) if parsed > 0 => {
                if let Some((min, max)) = range {
                    if parsed < min || parsed > max {
                        errors.push(format!(
                            "{name}={parsed} is outside the allowed range {min}..={max}"
                        ));
                    }
                }
                parsed
            }
            _ => {
                errors.push(format!("{name} must be a positive integer"));
                default
            }
        },
        _ => default,
    }
}

fn parse_usize_env(name: &str, default: usize, errors: &mut Vec<String>) -> usize {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => match value.trim().parse::<usize>() {
            Ok(parsed) if parsed > 0 => parsed,
            _ => {
                errors.push(format!("{name} must be a positive integer"));
                default
            }
        },
        _ => default,
    }
}

fn parse_non_negative_f64_env(name: &str, default: f64, errors: &mut Vec<String>) -> f64 {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => match value.trim().parse::<f64>() {
            Ok(parsed) if parsed.is_finite() && parsed >= 0.0 => parsed,
            _ => {
                errors.push(format!("{name} must be a non-negative finite number"));
                default
            }
        },
        _ => default,
    }
}

fn api_token_file_configured(errors: &mut Vec<String>) -> bool {
    let Ok(path) = std::env::var("TANDEM_API_TOKEN_FILE") else {
        return false;
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    match std::fs::read_to_string(PathBuf::from(trimmed)) {
        Ok(value) if !value.trim().is_empty() => true,
        Ok(_) => {
            errors.push(format!(
                "TANDEM_API_TOKEN_FILE points to an empty file: {trimmed}"
            ));
            false
        }
        Err(error) => {
            errors.push(format!(
                "TANDEM_API_TOKEN_FILE could not be read ({trimmed}): {error}"
            ));
            false
        }
    }
}

fn validate_context_assertion_public_keys(errors: &mut Vec<String>) {
    if let Some(raw_keys) = read_env_or_file(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE",
        errors,
    ) {
        validate_keyring(&raw_keys, "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", errors);
    }
    if let Some(raw_key) = read_env_or_file(
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE",
        errors,
    ) {
        validate_public_key(&raw_key, "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY", errors);
    }
}

fn read_env_or_file(
    env_name: &str,
    file_env_name: &str,
    errors: &mut Vec<String>,
) -> Option<String> {
    if let Ok(value) = std::env::var(env_name) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let Ok(path) = std::env::var(file_env_name) else {
        return None;
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    match std::fs::read_to_string(PathBuf::from(trimmed)) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        Ok(_) => {
            errors.push(format!(
                "{file_env_name} points to an empty file: {trimmed}"
            ));
            None
        }
        Err(error) => {
            errors.push(format!(
                "{file_env_name} could not be read ({trimmed}): {error}"
            ));
            None
        }
    }
}

fn validate_keyring(raw: &str, source: &str, errors: &mut Vec<String>) {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(trimmed) {
            Ok(entries) if entries.is_empty() => errors.push(format!("{source} must not be empty")),
            Ok(entries) => {
                for (kid, value) in entries {
                    if kid.trim().is_empty() {
                        errors.push(format!("{source} contains an empty key id"));
                        continue;
                    }
                    let raw_key = match value {
                        serde_json::Value::String(value) => Some(value),
                        serde_json::Value::Object(mut object) => object
                            .remove("public_key")
                            .or_else(|| object.remove("publicKey"))
                            .and_then(|value| value.as_str().map(ToString::to_string)),
                        _ => None,
                    };
                    match raw_key {
                        Some(raw_key) => validate_public_key(&raw_key, source, errors),
                        None => {
                            errors.push(format!("{source} entry `{kid}` is missing public_key"))
                        }
                    }
                }
            }
            Err(error) => errors.push(format!("{source} is not valid JSON keyring: {error}")),
        }
        return;
    }

    let mut saw_entry = false;
    for entry in trimmed.split([',', '\n', ';']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        saw_entry = true;
        let Some((kid, raw_key)) = entry.split_once('=').or_else(|| entry.split_once(':')) else {
            errors.push(format!(
                "{source} entry `{entry}` must be kid=base64_public_key"
            ));
            continue;
        };
        if kid.trim().is_empty() {
            errors.push(format!("{source} contains an empty key id"));
        }
        validate_public_key(raw_key, source, errors);
    }
    if !saw_entry {
        errors.push(format!("{source} must not be empty"));
    }
}

fn validate_public_key(raw: &str, source: &str, errors: &mut Vec<String>) {
    if decode_public_key(raw).is_none() {
        errors.push(format!(
            "{source} must contain base64/base64url Ed25519 public keys"
        ));
    }
}

fn decode_public_key(raw: &str) -> Option<[u8; 32]> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw.trim()))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw.trim()))
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
}

fn unknown_tandem_env_warnings() -> Vec<String> {
    let known: BTreeSet<&'static str> = CONFIG_VARS.iter().map(|var| var.name).collect();
    let mut warnings = Vec::new();
    for (name, _) in std::env::vars() {
        if !name.starts_with("TANDEM_") {
            continue;
        }
        if known.contains(name.as_str())
            || KNOWN_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
        {
            continue;
        }
        warnings.push(format!("unknown Tandem environment variable `{name}`"));
    }
    warnings
}

fn env_value_present(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

struct ConfigVar {
    name: &'static str,
    default: &'static str,
    notes: &'static str,
}

const KNOWN_PREFIXES: &[&str] = &[
    "TANDEM_AGENT_AUTOMATION_",
    "TANDEM_APPROVAL_",
    "TANDEM_AUDIT_",
    "TANDEM_AUTO_",
    "TANDEM_BASE_",
    "TANDEM_BASH_",
    "TANDEM_BENCHMARK_",
    "TANDEM_BIN",
    "TANDEM_BOT_",
    "TANDEM_BRAVE_",
    "TANDEM_BROWSER_",
    "TANDEM_INCIDENT_MONITOR_",
    "TANDEM_BUILD_",
    "TANDEM_BUILTIN_",
    "TANDEM_CHANNEL_",
    "TANDEM_CODER_",
    "TANDEM_CONNECTOR_",
    "TANDEM_CONTEXT_ASSERTION",
    "TANDEM_CONTROL_PANEL_",
    "TANDEM_CORS_",
    "TANDEM_CROSS_TENANT_",
    "TANDEM_CURSOR",
    "TANDEM_DEFAULT_",
    "TANDEM_DISABLE_",
    "TANDEM_DISCORD_",
    "TANDEM_DOCKER_",
    "TANDEM_DOCS_",
    "TANDEM_ENABLE_",
    "TANDEM_ENGINE_",
    "TANDEM_ENTERPRISE_",
    "TANDEM_EXA_",
    "TANDEM_FAILURE_REPORTER_",
    "TANDEM_FORCE_",
    "TANDEM_FULL_CONTEXT_",
    "TANDEM_GLOBAL_",
    "TANDEM_GOOGLE_",
    "TANDEM_HOME",
    "TANDEM_HOSTED_",
    "TANDEM_MARKET",
    "TANDEM_MCP",
    "TANDEM_MEMORY_",
    "TANDEM_MISSION_",
    "TANDEM_OBSERVABILITY_",
    "TANDEM_OPENCODE_",
    "TANDEM_ORCH_",
    "TANDEM_OS_",
    "TANDEM_PACK_",
    "TANDEM_PERMISSION_",
    "TANDEM_PERSONA",
    "TANDEM_PREWRITE_",
    "TANDEM_PROMPT_",
    "TANDEM_PROTOCOL_",
    "TANDEM_PROVIDER_",
    "TANDEM_RELAXATION_",
    "TANDEM_RESPONSE_",
    "TANDEM_RUN_",
    "TANDEM_RUNTIME_",
    "TANDEM_SCHEDULER_",
    "TANDEM_SCHEME",
    "TANDEM_SEARCH_",
    "TANDEM_SEARXNG_",
    "TANDEM_SEMANTIC_",
    "TANDEM_SERVER_",
    "TANDEM_SHARED_",
    "TANDEM_SKILL_",
    "TANDEM_SLACK_",
    "TANDEM_STALE_",
    "TANDEM_STATE_",
    "TANDEM_STORAGE_",
    "TANDEM_STRICT_",
    "TANDEM_TELEGRAM_",
    "TANDEM_TEST_",
    "TANDEM_TOKEN",
    "TANDEM_TOOL_",
    "TANDEM_TUI_",
    "TANDEM_UNSAFE_",
    "TANDEM_WEB_",
    "TANDEM_WEBSEARCH_",
    "TANDEM_WORKFLOW_",
];

const CONFIG_VARS: &[ConfigVar] = &[
    ConfigVar { name: "TANDEM_RUNTIME_AUTH_MODE", default: "local_single_tenant", notes: "Runtime trust mode: local_single_tenant, hosted_single_tenant, or enterprise_required." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_MODE", default: "off", notes: "Data boundary evaluation at every production LLM-provider dispatch: off, audit, or enforce. Enforce can block, transform, or require approval." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", default: "block", notes: "Treatment of raw sensitive data headed to unapproved external providers: allow, audit, redact, approval, require_local, or block." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES", default: "unset", notes: "Optional provider-payload byte cap; blocks oversized dispatches in enforce mode." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES", default: "unset", notes: "Comma-separated sensitive data classes requiring approval (e.g. credential,customer_data)." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_REDACT_CLASSES", default: "unset", notes: "Comma-separated sensitive data classes to redact before external dispatch." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_BLOCK_CLASSES", default: "unset", notes: "Comma-separated sensitive data classes that must never leave for a provider." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES", default: "unset", notes: "Comma-separated provider_id=boundary_class mappings (e.g. openai=approved_external, ollama=local). All unmapped providers classify as unknown - builtin loopback ids get no id-based trust because their base URLs can be reconfigured to remote endpoints." },
    ConfigVar { name: "TANDEM_DATA_BOUNDARY_STRICT", default: "false", notes: "Strict enterprise posture: enforce mode fails closed on missing tenant/run/session authority or unknown provider classification." },
    ConfigVar { name: "TANDEM_API_TOKEN", default: "unset", notes: "Explicit HTTP transport bearer token. Secret value is never printed by config check." },
    ConfigVar { name: "TANDEM_API_TOKEN_FILE", default: "unset", notes: "File containing the HTTP transport bearer token. Required in hosted/enterprise mode unless --api-token is supplied." },
    ConfigVar { name: "TANDEM_UNSAFE_NO_API_TOKEN", default: "false", notes: "Local loopback development only; rejected in hosted/enterprise mode." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", default: "unset", notes: "JSON or kid=base64 Ed25519 context assertion verifier keyring. Required in hosted/enterprise mode." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE", default: "unset", notes: "File containing the context assertion verifier keyring." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY", default: "unset", notes: "Legacy single Ed25519 verifier public key." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE", default: "unset", notes: "File containing the legacy single verifier public key." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_ISSUER", default: "tandem-web", notes: "Expected context assertion issuer." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_AUDIENCE", default: "tandem-runtime", notes: "Expected context assertion audience." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_REPLAY_MODE", default: "audit", notes: "Replay handling mode for verified context assertions." },
    ConfigVar { name: "TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS", default: "10000", notes: "Allowed future clock skew for assertions; valid range 10000..=60000." },
    ConfigVar { name: "TANDEM_HOSTED_CONTROL_PLANE_URL", default: "unset", notes: "Hosted control-plane URL; enables enterprise-scoped memory policy." },
    ConfigVar { name: "TANDEM_ENTERPRISE_CONTROL_PLANE_URL", default: "unset", notes: "Enterprise control-plane URL alias." },
    ConfigVar { name: "TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY", default: "unset", notes: "Secret signing key for cross-tenant grants." },
    ConfigVar { name: "TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_FILE", default: "unset", notes: "File containing the cross-tenant grant signing key." },
    ConfigVar { name: "TANDEM_AUDIT_HMAC_KEY", default: "unset", notes: "Deployment secret used for privacy-preserving predicate evidence and exact-action approval bindings. Required when those authored policies execute in hosted/enterprise mode." },
    ConfigVar { name: "TANDEM_AUDIT_HMAC_KEY_FILE", default: "unset", notes: "File containing the deployment policy-evidence audit HMAC key." },
    ConfigVar { name: "TANDEM_RUN_STALE_MS", default: "120000", notes: "Run staleness threshold; valid range 30000..=600000." },
    ConfigVar { name: "TANDEM_TOKEN_COST_PER_1K_USD", default: "0.0", notes: "Non-negative token cost used for estimates." },
    ConfigVar { name: "TANDEM_AUTOMATION_STRICT_RESEARCH_QUALITY", default: "true", notes: "Enable strict automation research quality checks." },
    ConfigVar { name: "TANDEM_AUTOMATION_QUALITY_LEGACY_ROLLBACK", default: "false", notes: "Enable legacy rollback behavior for automation quality checks." },
    ConfigVar { name: "TANDEM_AUTOMATION_EXECUTE_NODE_TIMEOUT_MS", default: "1800000", notes: "Automation node timeout; valid range 180000..=3600000." },
    ConfigVar { name: "TANDEM_AUTOMATION_WEBHOOK_ALLOW_UNSIGNED_DEV_MODE", default: "false", notes: "Dev/test only opt-in for unsigned automation webhook triggers." },
    ConfigVar { name: "TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED", default: "false", notes: "Enable the authenticated `/metrics` Prometheus endpoint." },
    ConfigVar { name: "TANDEM_SCHEDULER_MODE", default: "multi", notes: "Scheduler mode: single or multi." },
    ConfigVar { name: "TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS", default: "8", notes: "Positive maximum concurrent scheduler runs." },
    ConfigVar { name: "TANDEM_SCHEDULER_SHUTDOWN_TIMEOUT_SECS", default: "30", notes: "Positive scheduler shutdown timeout." },
    ConfigVar { name: "TANDEM_STATE_DIR", default: "shared path", notes: "Engine state directory." },
    ConfigVar { name: "TANDEM_STORAGE_DIR", default: "state dir", notes: "Storage directory override." },
    ConfigVar { name: "TANDEM_STORAGE_BACKEND", default: "sqlite", notes: "Stateful store backend: sqlite or postgres. Fail-closed on invalid values." },
    ConfigVar { name: "TANDEM_STORAGE_POSTGRES_URL", default: "unset", notes: "PostgreSQL connection URL; required when TANDEM_STORAGE_BACKEND=postgres." },
    ConfigVar { name: "TANDEM_ENGINE_HOST", default: "127.0.0.1", notes: "Default engine bind host for CLI commands." },
    ConfigVar { name: "TANDEM_ENGINE_PORT", default: "39731", notes: "Default engine bind port for CLI commands." },
    ConfigVar { name: "TANDEM_DISABLE_EMBEDDINGS", default: "false", notes: "Disable semantic memory embeddings." },
    ConfigVar { name: "TANDEM_WEB_UI", default: "false", notes: "Enable embedded web admin UI." },
    ConfigVar { name: "TANDEM_WEB_UI_PREFIX", default: "/admin", notes: "Embedded web admin UI path prefix." },
];

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    const VALID_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    #[serial]
    fn hosted_mode_without_assertion_keys_fails() {
        with_env(
            &[
                ("TANDEM_RUNTIME_AUTH_MODE", Some("hosted_single_tenant")),
                ("TANDEM_API_TOKEN", Some("secret")),
                ("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS", None),
                ("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY", None),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("requires TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS")));
            },
        );
    }

    #[test]
    #[serial]
    fn hosted_mode_without_transport_token_fails() {
        with_env(
            &[
                ("TANDEM_RUNTIME_AUTH_MODE", Some("enterprise_required")),
                (
                    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
                    Some("main=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                ),
                ("TANDEM_API_TOKEN", None),
                ("TANDEM_API_TOKEN_FILE", None),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("requires TANDEM_API_TOKEN")));
            },
        );
    }

    #[test]
    #[serial]
    fn hosted_mode_with_key_and_token_passes() {
        with_env(
            &[
                ("TANDEM_RUNTIME_AUTH_MODE", Some("enterprise_required")),
                (
                    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
                    Some("main=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                ),
                ("TANDEM_API_TOKEN", Some("secret")),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report.errors.is_empty(), "{:?}", report.errors);
            },
        );
    }

    #[test]
    #[serial]
    fn hosted_mode_rejects_unsafe_no_api_token_from_env() {
        with_env(
            &[
                ("TANDEM_RUNTIME_AUTH_MODE", Some("enterprise_required")),
                (
                    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
                    Some("main=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                ),
                ("TANDEM_API_TOKEN", Some("secret")),
                ("TANDEM_UNSAFE_NO_API_TOKEN", Some("1")),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("cannot use TANDEM_UNSAFE_NO_API_TOKEN")));
            },
        );
    }

    #[test]
    #[serial]
    fn hosted_mode_rejects_missing_api_token_file() {
        with_env(
            &[
                ("TANDEM_RUNTIME_AUTH_MODE", Some("enterprise_required")),
                (
                    "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
                    Some("main=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                ),
                ("TANDEM_API_TOKEN", None),
                (
                    "TANDEM_API_TOKEN_FILE",
                    Some("target/does-not-exist/tandem-token"),
                ),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("TANDEM_API_TOKEN_FILE could not be read")));
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("requires TANDEM_API_TOKEN")));
            },
        );
    }

    #[test]
    #[serial]
    fn malformed_context_key_fails_validation() {
        with_env(
            &[("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY", Some("not-a-key"))],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(report
                    .errors
                    .iter()
                    .any(|error| error.contains("Ed25519 public keys")));
            },
        );
    }

    #[test]
    #[serial]
    fn invalid_data_boundary_config_fails_validation() {
        with_data_boundary_env(
            &[
                ("TANDEM_DATA_BOUNDARY_MODE", Some("enforced")),
                ("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", Some("maybe")),
                (
                    "TANDEM_DATA_BOUNDARY_REDACT_CLASSES",
                    Some("credential,super_secret_typo"),
                ),
                ("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES", Some("lots")),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                for needle in [
                    "TANDEM_DATA_BOUNDARY_MODE",
                    "TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY",
                    "super_secret_typo",
                    "TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES",
                ] {
                    assert!(
                        report.errors.iter().any(|error| error.contains(needle)),
                        "expected error mentioning {needle}: {:?}",
                        report.errors
                    );
                }
            },
        );
    }

    #[test]
    #[serial]
    fn valid_data_boundary_config_passes_validation() {
        with_data_boundary_env(
            &[
                ("TANDEM_DATA_BOUNDARY_MODE", Some("audit")),
                ("TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY", Some("redact")),
                (
                    "TANDEM_DATA_BOUNDARY_REDACT_CLASSES",
                    Some("credential, customer_data"),
                ),
                ("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES", Some("1048576")),
            ],
            || {
                let report = EngineConfigReport::from_env(EngineConfigOptions::default());
                assert!(
                    !report
                        .errors
                        .iter()
                        .any(|error| error.contains("TANDEM_DATA_BOUNDARY")),
                    "unexpected data boundary errors: {:?}",
                    report.errors
                );
                assert!(
                    !report
                        .warnings
                        .iter()
                        .any(|warning| warning.contains("TANDEM_DATA_BOUNDARY")),
                    "data boundary vars must be registered known vars: {:?}",
                    report.warnings
                );
            },
        );
    }

    #[test]
    #[serial]
    fn unknown_tandem_env_vars_warn() {
        with_env(&[("TANDEM_TYPOED_SETTING", Some("1"))], || {
            let report = EngineConfigReport::from_env(EngineConfigOptions::default());
            assert!(report
                .warnings
                .iter()
                .any(|warning| warning.contains("TANDEM_TYPOED_SETTING")));
        });
    }

    /// Like `with_env`, but scoped strictly to TANDEM_DATA_BOUNDARY_* vars so
    /// these tests (which hold the `data_boundary_env` lock rather than the
    /// default config lock) can never clobber vars owned by other config tests.
    fn with_data_boundary_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        assert!(pairs
            .iter()
            .all(|(name, _)| name.starts_with("TANDEM_DATA_BOUNDARY_")));
        let saved = pairs
            .iter()
            .map(|(name, _)| (*name, std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for (name, value) in pairs {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        f();
        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    fn with_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        let saved = pairs
            .iter()
            .map(|(name, _)| (*name, std::env::var(name).ok()))
            .collect::<Vec<_>>();
        for (name, value) in pairs {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        f();
        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        std::env::remove_var("TANDEM_TYPOED_SETTING");
        std::env::remove_var("TANDEM_API_TOKEN_FILE");
        std::env::remove_var("TANDEM_UNSAFE_NO_API_TOKEN");
        std::env::remove_var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE");
        std::env::remove_var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE");
    }
}
