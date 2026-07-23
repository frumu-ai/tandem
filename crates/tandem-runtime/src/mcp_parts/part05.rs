// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn migrate_server_headers(
    server_name: &str,
    server: &McpServer,
    current_tenant: &TenantContext,
) -> (
    HashMap<String, String>,
    HashMap<String, McpSecretRef>,
    HashMap<String, String>,
    bool,
) {
    let original_effective = effective_headers(server);
    let mut persisted_secret_headers = server.secret_headers.clone();
    let mut secret_header_values = if current_tenant.is_local_implicit() {
        resolve_secret_header_values(&persisted_secret_headers, current_tenant)
    } else {
        HashMap::new()
    };
    let mut persisted_headers = server.headers.clone();
    let mut migrated = false;

    let header_keys = persisted_headers.keys().cloned().collect::<Vec<_>>();
    for header_name in header_keys {
        let Some(value) = persisted_headers.get(&header_name).cloned() else {
            continue;
        };
        if persisted_secret_headers.contains_key(&header_name) {
            continue;
        }
        if let Some(secret_ref) = parse_secret_header_reference(value.trim()) {
            persisted_headers.remove(&header_name);
            let resolved =
                resolve_secret_ref_value(&secret_ref, current_tenant).unwrap_or_default();
            persisted_secret_headers.insert(header_name.clone(), secret_ref);
            if current_tenant.is_local_implicit() && !resolved.is_empty() {
                secret_header_values.insert(header_name.clone(), resolved);
            }
            migrated = true;
            continue;
        }
        if header_name_is_sensitive(&header_name) && !value.trim().is_empty() {
            let secret_id = mcp_header_secret_id(server_name, &header_name);
            if tandem_core::set_provider_auth_for_tenant(current_tenant, &secret_id, &value).is_ok()
            {
                persisted_headers.remove(&header_name);
                persisted_secret_headers.insert(
                    header_name.clone(),
                    McpSecretRef::Store {
                        secret_id: secret_id.clone(),
                        tenant_context: current_tenant.clone(),
                    },
                );
                if current_tenant.is_local_implicit() {
                    secret_header_values.insert(header_name.clone(), value);
                }
                migrated = true;
            }
        }
    }

    if !migrated {
        let effective = combine_headers(&persisted_headers, &secret_header_values);
        migrated = effective != original_effective;
    }

    (
        persisted_headers,
        persisted_secret_headers,
        secret_header_values,
        migrated,
    )
}

fn split_headers_for_storage(
    server_name: &str,
    headers: HashMap<String, String>,
    explicit_secret_headers: HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) -> (
    HashMap<String, String>,
    HashMap<String, McpSecretRef>,
    HashMap<String, String>,
) {
    let mut persisted_headers = HashMap::new();
    let mut persisted_secret_headers = HashMap::new();
    let mut secret_header_values = HashMap::new();

    for (header_name, raw_value) in headers {
        let value = raw_value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        if let Some(secret_ref) = parse_secret_header_reference(&value) {
            if current_tenant.is_local_implicit() {
                if let Some(resolved) = resolve_secret_ref_value(&secret_ref, current_tenant) {
                    secret_header_values.insert(header_name.clone(), resolved);
                }
            }
            persisted_secret_headers.insert(header_name, secret_ref);
            continue;
        }
        if header_name_is_sensitive(&header_name) {
            let secret_id =
                mcp_header_secret_id_for_tenant(server_name, &header_name, current_tenant);
            if tandem_core::set_provider_auth_for_tenant(current_tenant, &secret_id, &value).is_ok()
            {
                persisted_secret_headers.insert(
                    header_name.clone(),
                    McpSecretRef::Store {
                        secret_id: secret_id.clone(),
                        tenant_context: current_tenant.clone(),
                    },
                );
                if current_tenant.is_local_implicit() {
                    secret_header_values.insert(header_name, value);
                }
                continue;
            }
        }
        persisted_headers.insert(header_name, value);
    }

    for (header_name, secret_ref) in explicit_secret_headers {
        if current_tenant.is_local_implicit() {
            if let Some(resolved) = resolve_secret_ref_value(&secret_ref, current_tenant) {
                secret_header_values.insert(header_name.clone(), resolved);
            }
        }
        persisted_headers.remove(&header_name);
        persisted_secret_headers.insert(header_name, secret_ref);
    }

    (
        persisted_headers,
        persisted_secret_headers,
        secret_header_values,
    )
}

fn public_headers_for_identity(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(header_name, raw_value)| {
            let value = raw_value.trim();
            (!value.is_empty()
                && parse_secret_header_reference(value).is_none()
                && !header_name_is_sensitive(header_name))
            .then(|| (header_name.clone(), value.to_string()))
        })
        .collect()
}

fn input_secret_header_refs(
    headers: &HashMap<String, String>,
    explicit_secret_headers: &HashMap<String, McpSecretRef>,
) -> HashMap<String, McpSecretRef> {
    let mut refs = explicit_secret_headers.clone();
    for (header_name, raw_value) in headers {
        if let Some(secret_ref) = parse_secret_header_reference(raw_value) {
            refs.insert(header_name.clone(), secret_ref);
        }
    }
    refs
}

fn combine_headers(
    headers: &HashMap<String, String>,
    secret_header_values: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut combined = headers.clone();
    for (key, value) in secret_header_values {
        if !value.trim().is_empty() {
            combined.insert(key.clone(), value.clone());
        }
    }
    combined
}

fn effective_headers(server: &McpServer) -> HashMap<String, String> {
    combine_headers(&server.headers, &server.secret_header_values)
}

fn effective_headers_for_tenant(
    server: &McpServer,
    current_tenant: &TenantContext,
) -> HashMap<String, String> {
    if current_tenant.is_local_implicit() {
        return effective_headers(server);
    }
    combine_headers(
        &server.headers,
        &resolve_secret_header_values(&server.secret_headers, current_tenant),
    )
}

fn redacted_server_view(server: &McpServer) -> McpServer {
    let mut clone = server.clone();
    for (header_name, secret_ref) in &clone.secret_headers {
        clone.headers.insert(
            header_name.clone(),
            redacted_secret_header_value(secret_ref),
        );
    }
    clone.secret_header_values.clear();
    if let Some(oauth) = clone.oauth.as_mut() {
        oauth.client_secret_ref = None;
        oauth.client_secret_value = None;
    }
    clone
}

fn normalize_auth_kind(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "oauth" | "auto" | "bearer" | "x-api-key" | "custom" | "none" => {
            raw.trim().to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn redacted_secret_header_value(secret_ref: &McpSecretRef) -> String {
    match secret_ref {
        McpSecretRef::BearerEnv { .. } => "Bearer ".to_string(),
        McpSecretRef::Env { .. } | McpSecretRef::Store { .. } => MCP_SECRET_PLACEHOLDER.to_string(),
    }
}

fn resolve_secret_header_values(
    secret_headers: &HashMap<String, McpSecretRef>,
    current_tenant: &TenantContext,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (header_name, secret_ref) in secret_headers {
        if let Some(value) = resolve_secret_ref_value(secret_ref, current_tenant) {
            if !value.trim().is_empty() {
                out.insert(header_name.clone(), value);
            }
        }
    }
    out
}
