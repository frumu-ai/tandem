use serde_json::Value;

pub fn shared_context_pack_ids_from_metadata(metadata: Option<&Value>) -> Vec<String> {
    let Some(metadata) = metadata.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut append_binding_ids = |value: &Value| {
        if let Some(entries) = value.as_array() {
            for entry in entries {
                if let Some(text) = entry.as_str() {
                    let text = text.trim();
                    if !text.is_empty() {
                        rows.push(text.to_string());
                    }
                    continue;
                }
                if let Some(obj) = entry.as_object() {
                    let id = obj
                        .get("pack_id")
                        .or_else(|| obj.get("packId"))
                        .or_else(|| obj.get("context_pack_id"))
                        .or_else(|| obj.get("contextPackId"))
                        .or_else(|| obj.get("id"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string);
                    if let Some(id) = id {
                        rows.push(id);
                    }
                }
            }
        }
    };

    if let Some(value) = metadata.get("shared_context_bindings") {
        append_binding_ids(value);
    }
    if let Some(value) = metadata.get("sharedContextBindings") {
        append_binding_ids(value);
    }
    if let Some(value) = metadata.get("shared_context_pack_ids") {
        append_binding_ids(value);
    }
    if let Some(value) = metadata.get("sharedContextPackIds") {
        append_binding_ids(value);
    }
    if let Some(value) = metadata
        .get("plan_package")
        .or_else(|| metadata.get("planPackage"))
    {
        if let Some(obj) = value.as_object() {
            if let Some(bindings) = obj.get("shared_context_bindings") {
                append_binding_ids(bindings);
            }
            if let Some(bindings) = obj.get("sharedContextBindings") {
                append_binding_ids(bindings);
            }
            if let Some(bindings) = obj.get("shared_context_pack_ids") {
                append_binding_ids(bindings);
            }
            if let Some(bindings) = obj.get("sharedContextPackIds") {
                append_binding_ids(bindings);
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    rows.retain(|value| seen.insert(value.clone()));
    rows
}
