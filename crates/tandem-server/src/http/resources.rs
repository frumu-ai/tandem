use super::*;

#[derive(Debug, Deserialize, Default)]
pub(super) struct ResourceListQuery {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ResourceEventsQuery {
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourceWriteInput {
    pub value: Value,
    pub if_match_rev: Option<u64>,
    pub updated_by: Option<String>,
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResourceDeleteInput {
    pub if_match_rev: Option<u64>,
    pub updated_by: Option<String>,
}

pub(super) fn resource_error_response(error: ResourceStoreError) -> (StatusCode, Json<Value>) {
    match error {
        ResourceStoreError::InvalidKey { key } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid resource key namespace",
                "code": "INVALID_RESOURCE_KEY",
                "key": key,
            })),
        ),
        ResourceStoreError::RevisionConflict(conflict) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Resource revision conflict",
                "code": "RESOURCE_REVISION_CONFLICT",
                "key": conflict.key,
                "expected_rev": conflict.expected_rev,
                "current_rev": conflict.current_rev,
            })),
        ),
        ResourceStoreError::PersistFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Resource persistence failed",
                "code": "RESOURCE_PERSIST_FAILED",
                "detail": message,
            })),
        ),
    }
}

pub(super) fn normalize_resource_key(raw: String) -> String {
    raw.trim_start_matches('/').trim().to_string()
}

fn resource_event_payload(mut payload: Value, tenant_context: &TenantContext) -> Value {
    if !tenant_context.is_local_implicit() {
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "tenantContext".to_string(),
                serde_json::to_value(tenant_context).unwrap_or(Value::Null),
            );
        }
    }
    payload
}

fn resource_event_tenant_context(event: &EngineEvent) -> TenantContext {
    event
        .properties
        .get("tenantContext")
        .cloned()
        .and_then(|value| serde_json::from_value::<TenantContext>(value).ok())
        .unwrap_or_else(TenantContext::local_implicit)
}

pub(super) fn resource_event_visible_to_tenant(
    event: &EngineEvent,
    tenant_context: &TenantContext,
) -> bool {
    super::tenant_matches(&resource_event_tenant_context(event), tenant_context)
}

pub(super) async fn resource_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<ResourceListQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = state
        .list_shared_resources_for_tenant(query.prefix.as_deref(), limit, &tenant_context)
        .await;
    Json(json!({
        "resources": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn resource_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(key): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let resource = state
        .get_shared_resource_for_tenant(&key, &tenant_context)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Resource not found",
                    "code": "RESOURCE_NOT_FOUND",
                    "key": key,
                })),
            )
        })?;

    Ok(Json(json!({
        "resource": resource,
    })))
}

pub(super) async fn resource_put(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(key): Path<String>,
    Json(input): Json<ResourceWriteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let record = state
        .put_shared_resource_for_tenant(
            key.clone(),
            input.value,
            input.if_match_rev,
            updated_by.clone(),
            input.ttl_ms,
            &tenant_context,
        )
        .await
        .map_err(resource_error_response)?;

    state.event_bus.publish(EngineEvent::new(
        "resource.updated",
        resource_event_payload(
            json!({
                "key": record.key,
                "rev": record.rev,
                "updatedBy": updated_by,
                "updatedAtMs": record.updated_at_ms,
            }),
            &tenant_context,
        ),
    ));

    Ok(Json(json!({
        "resource": record
    })))
}

pub(super) async fn resource_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(key): Path<String>,
    Json(input): Json<ResourceWriteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let existing = state
        .get_shared_resource_for_tenant(&key, &tenant_context)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Resource not found",
                    "code": "RESOURCE_NOT_FOUND",
                    "key": key,
                })),
            )
        })?;

    let merged_value = if existing.value.is_object() && input.value.is_object() {
        let mut map = existing.value.as_object().cloned().unwrap_or_default();
        for (k, v) in input.value.as_object().cloned().unwrap_or_default() {
            map.insert(k, v);
        }
        Value::Object(map)
    } else {
        input.value
    };

    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let record = state
        .put_shared_resource_for_tenant(
            key.clone(),
            merged_value,
            input.if_match_rev,
            updated_by.clone(),
            input.ttl_ms.or(existing.ttl_ms),
            &tenant_context,
        )
        .await
        .map_err(resource_error_response)?;

    state.event_bus.publish(EngineEvent::new(
        "resource.updated",
        resource_event_payload(
            json!({
                "key": record.key,
                "rev": record.rev,
                "updatedBy": updated_by,
                "updatedAtMs": record.updated_at_ms,
            }),
            &tenant_context,
        ),
    ));

    Ok(Json(json!({
        "resource": record
    })))
}

pub(super) async fn resource_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(key): Path<String>,
    Json(input): Json<ResourceDeleteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let deleted = state
        .delete_shared_resource_for_tenant(&key, input.if_match_rev, &tenant_context)
        .await
        .map_err(resource_error_response)?;

    if let Some(record) = deleted {
        state.event_bus.publish(EngineEvent::new(
            "resource.deleted",
            resource_event_payload(
                json!({
                    "key": record.key,
                    "rev": record.rev,
                    "updatedBy": updated_by,
                    "updatedAtMs": crate::now_ms(),
                }),
                &tenant_context,
            ),
        ));
        Ok(Json(json!({
            "deleted": true,
            "key": key,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Resource not found",
                "code": "RESOURCE_NOT_FOUND",
                "key": key,
            })),
        ))
    }
}

pub(super) fn resource_sse_stream(
    state: AppState,
    prefix: Option<String>,
    tenant_context: TenantContext,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "resource",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if event.event_type != "resource.updated" && event.event_type != "resource.deleted" {
                return None;
            }
            if !resource_event_visible_to_tenant(&event, &tenant_context) {
                return None;
            }
            if let Some(prefix) = prefix.as_deref() {
                let key = event
                    .properties
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !key.starts_with(prefix) {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn resource_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<ResourceEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(resource_sse_stream(state, query.prefix, tenant_context))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
