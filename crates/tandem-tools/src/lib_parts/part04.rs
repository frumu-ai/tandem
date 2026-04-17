fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn uuid_like(seed: u64) -> String {
    format!("{:x}", seed)
}

struct MemorySearchTool;
#[async_trait]
impl Tool for MemorySearchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_search",
            "Search tandem memory across session/project/global tiers. If scope fields are omitted, the tool defaults to the current session/project context and may include global memory when policy allows it.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "limit":{"type":"integer","minimum":1,"maximum":20},
                    "allow_global":{"type":"boolean"}
                },
                "required":["query"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .or_else(|| args.get("q"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if query.is_empty() {
            return Ok(ToolResult {
                output: "memory_search requires a non-empty query".to_string(),
                metadata: json!({"ok": false, "reason": "missing_query"}),
            });
        }

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
        let allow_global = global_memory_enabled(&args);
        if session_id.is_none() && project_id.is_none() && !allow_global {
            return Ok(ToolResult {
                output: "memory_search requires a current session/project context or global memory enabled by policy"
                    .to_string(),
                metadata: json!({"ok": false, "reason": "missing_scope"}),
            });
        }

        let tier = match args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(t) if t == "session" => Some(MemoryTier::Session),
            Some(t) if t == "project" => Some(MemoryTier::Project),
            Some(t) if t == "global" => Some(MemoryTier::Global),
            Some(_) => {
                return Ok(ToolResult {
                    output: "memory_search tier must be one of: session, project, global"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
            None => None,
        };
        if matches!(tier, Some(MemoryTier::Session)) && session_id.is_none() {
            return Ok(ToolResult {
                output: "tier=session requires session_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_session_scope"}),
            });
        }
        if matches!(tier, Some(MemoryTier::Project)) && project_id.is_none() {
            return Ok(ToolResult {
                output: "tier=project requires project_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_project_scope"}),
            });
        }
        if matches!(tier, Some(MemoryTier::Global)) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(5)
            .clamp(1, 20);

        let db_path = resolve_memory_db_path(&args);
        let db_exists = db_path.exists();
        if !db_exists {
            return Ok(ToolResult {
                output: "memory database not found".to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "memory_db_missing",
                    "db_path": db_path,
                }),
            });
        }

        let manager = MemoryManager::new(&db_path).await?;
        let health = manager.embedding_health().await;
        if health.status != "ok" {
            return Ok(ToolResult {
                output: "memory embeddings unavailable; semantic search is disabled".to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "embeddings_unavailable",
                    "embedding_status": health.status,
                    "embedding_reason": health.reason,
                }),
            });
        }

        let mut results: Vec<MemorySearchResult> = Vec::new();
        match tier {
            Some(MemoryTier::Session) => {
                results.extend(
                    manager
                        .search(
                            query,
                            Some(MemoryTier::Session),
                            project_id.as_deref(),
                            session_id.as_deref(),
                            Some(limit),
                        )
                        .await?,
                );
            }
            Some(MemoryTier::Project) => {
                results.extend(
                    manager
                        .search(
                            query,
                            Some(MemoryTier::Project),
                            project_id.as_deref(),
                            session_id.as_deref(),
                            Some(limit),
                        )
                        .await?,
                );
            }
            Some(MemoryTier::Global) => {
                results.extend(
                    manager
                        .search(query, Some(MemoryTier::Global), None, None, Some(limit))
                        .await?,
                );
            }
            _ => {
                if session_id.is_some() {
                    results.extend(
                        manager
                            .search(
                                query,
                                Some(MemoryTier::Session),
                                project_id.as_deref(),
                                session_id.as_deref(),
                                Some(limit),
                            )
                            .await?,
                    );
                }
                if project_id.is_some() {
                    results.extend(
                        manager
                            .search(
                                query,
                                Some(MemoryTier::Project),
                                project_id.as_deref(),
                                session_id.as_deref(),
                                Some(limit),
                            )
                            .await?,
                    );
                }
                if allow_global {
                    results.extend(
                        manager
                            .search(query, Some(MemoryTier::Global), None, None, Some(limit))
                            .await?,
                    );
                }
            }
        }

        let mut dedup: HashMap<String, MemorySearchResult> = HashMap::new();
        for result in results {
            match dedup.get(&result.chunk.id) {
                Some(existing) if existing.similarity >= result.similarity => {}
                _ => {
                    dedup.insert(result.chunk.id.clone(), result);
                }
            }
        }
        let mut merged = dedup.into_values().collect::<Vec<_>>();
        merged.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        merged.truncate(limit as usize);

        let output_rows = merged
            .iter()
            .map(|item| {
                json!({
                    "chunk_id": item.chunk.id,
                    "tier": item.chunk.tier.to_string(),
                    "session_id": item.chunk.session_id,
                    "project_id": item.chunk.project_id,
                    "source": item.chunk.source,
                    "similarity": item.similarity,
                    "content": item.chunk.content,
                    "created_at": item.chunk.created_at,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output_rows).unwrap_or_default(),
            metadata: json!({
                "ok": true,
                "count": output_rows.len(),
                "limit": limit,
                "query": query,
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "embedding_status": health.status,
                "embedding_reason": health.reason,
                "strict_scope": !allow_global,
            }),
        })
    }
}

struct MemoryStoreTool;
#[async_trait]
impl Tool for MemoryStoreTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_store",
            "Store memory chunks in session/project/global tiers. If scope is omitted, the tool defaults to the current project, then session, and only uses global memory when policy allows it.",
            json!({
                "type":"object",
                "properties":{
                    "content":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "source":{"type":"string"},
                    "metadata":{"type":"object"},
                    "allow_global":{"type":"boolean"}
                },
                "required":["content"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if content.is_empty() {
            return Ok(ToolResult {
                output: "memory_store requires non-empty content".to_string(),
                metadata: json!({"ok": false, "reason": "missing_content"}),
            });
        }

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
        let allow_global = global_memory_enabled(&args);

        let tier = match args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(t) if t == "session" => MemoryTier::Session,
            Some(t) if t == "project" => MemoryTier::Project,
            Some(t) if t == "global" => MemoryTier::Global,
            Some(_) => {
                return Ok(ToolResult {
                    output: "memory_store tier must be one of: session, project, global"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
            None => {
                if project_id.is_some() {
                    MemoryTier::Project
                } else if session_id.is_some() {
                    MemoryTier::Session
                } else if allow_global {
                    MemoryTier::Global
                } else {
                    return Ok(ToolResult {
                        output: "memory_store requires a current session/project context or global memory enabled by policy"
                            .to_string(),
                        metadata: json!({"ok": false, "reason": "missing_scope"}),
                    });
                }
            }
        };

        if matches!(tier, MemoryTier::Session) && session_id.is_none() {
            return Ok(ToolResult {
                output: "tier=session requires session_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_session_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Project) && project_id.is_none() {
            return Ok(ToolResult {
                output: "tier=project requires project_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_project_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Global) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let db_path = resolve_memory_db_path(&args);
        let manager = MemoryManager::new(&db_path).await?;
        let health = manager.embedding_health().await;
        if health.status != "ok" {
            return Ok(ToolResult {
                output: "memory embeddings unavailable; semantic memory store is disabled"
                    .to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "embeddings_unavailable",
                    "embedding_status": health.status,
                    "embedding_reason": health.reason,
                }),
            });
        }

        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("agent_note")
            .to_string();
        let metadata = args.get("metadata").cloned();

        let request = tandem_memory::types::StoreMessageRequest {
            content: content.to_string(),
            tier,
            session_id: session_id.clone(),
            project_id: project_id.clone(),
            source,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            metadata,
        };
        let chunk_ids = manager.store_message(request).await?;

        Ok(ToolResult {
            output: format!("stored {} chunk(s) in {} memory", chunk_ids.len(), tier),
            metadata: json!({
                "ok": true,
                "chunk_ids": chunk_ids,
                "count": chunk_ids.len(),
                "tier": tier.to_string(),
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "embedding_status": health.status,
                "embedding_reason": health.reason,
                "db_path": db_path,
            }),
        })
    }
}

struct MemoryListTool;
#[async_trait]
impl Tool for MemoryListTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_list",
            "List stored memory chunks for auditing and knowledge-base browsing.",
            json!({
                "type":"object",
                "properties":{
                    "tier":{"type":"string","enum":["session","project","global","all"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "limit":{"type":"integer","minimum":1,"maximum":200},
                    "allow_global":{"type":"boolean"}
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
        let allow_global = global_memory_enabled(&args);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(50)
            .clamp(1, 200) as usize;

        let tier = args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "all".to_string());
        if tier == "global" && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }
        if session_id.is_none() && project_id.is_none() && tier != "global" && !allow_global {
            return Ok(ToolResult {
                output: "memory_list requires a current session/project context or global memory enabled by policy".to_string(),
                metadata: json!({"ok": false, "reason": "missing_scope"}),
            });
        }

        let db_path = resolve_memory_db_path(&args);
        let manager = MemoryManager::new(&db_path).await?;

        let mut chunks: Vec<tandem_memory::types::MemoryChunk> = Vec::new();
        match tier.as_str() {
            "session" => {
                let Some(sid) = session_id.as_deref() else {
                    return Ok(ToolResult {
                        output: "tier=session requires session_id".to_string(),
                        metadata: json!({"ok": false, "reason": "missing_session_scope"}),
                    });
                };
                chunks.extend(manager.db().get_session_chunks(sid).await?);
            }
            "project" => {
                let Some(pid) = project_id.as_deref() else {
                    return Ok(ToolResult {
                        output: "tier=project requires project_id".to_string(),
                        metadata: json!({"ok": false, "reason": "missing_project_scope"}),
                    });
                };
                chunks.extend(manager.db().get_project_chunks(pid).await?);
            }
            "global" => {
                chunks.extend(manager.db().get_global_chunks(limit as i64).await?);
            }
            "all" => {
                if let Some(sid) = session_id.as_deref() {
                    chunks.extend(manager.db().get_session_chunks(sid).await?);
                }
                if let Some(pid) = project_id.as_deref() {
                    chunks.extend(manager.db().get_project_chunks(pid).await?);
                }
                if allow_global {
                    chunks.extend(manager.db().get_global_chunks(limit as i64).await?);
                }
            }
            _ => {
                return Ok(ToolResult {
                    output: "memory_list tier must be one of: session, project, global, all"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
        }

        chunks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        chunks.truncate(limit);
        let rows = chunks
            .iter()
            .map(|chunk| {
                json!({
                    "chunk_id": chunk.id,
                    "tier": chunk.tier.to_string(),
                    "session_id": chunk.session_id,
                    "project_id": chunk.project_id,
                    "source": chunk.source,
                    "content": chunk.content,
                    "created_at": chunk.created_at,
                    "metadata": chunk.metadata,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&rows).unwrap_or_default(),
            metadata: json!({
                "ok": true,
                "count": rows.len(),
                "limit": limit,
                "tier": tier,
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "db_path": db_path,
            }),
        })
    }
}

struct MemoryDeleteTool;
#[async_trait]
impl Tool for MemoryDeleteTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_delete",
            "Delete a stored memory chunk from session/project/global memory within the current allowed scope.",
            json!({
                "type":"object",
                "properties":{
                    "chunk_id":{"type":"string"},
                    "id":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "allow_global":{"type":"boolean"}
                },
                "required":["chunk_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let chunk_id = args
            .get("chunk_id")
            .or_else(|| args.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if chunk_id.is_empty() {
            return Ok(ToolResult {
                output: "memory_delete requires chunk_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_chunk_id"}),
            });
        }

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
        let allow_global = global_memory_enabled(&args);

        let tier = match args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(t) if t == "session" => MemoryTier::Session,
            Some(t) if t == "project" => MemoryTier::Project,
            Some(t) if t == "global" => MemoryTier::Global,
            Some(_) => {
                return Ok(ToolResult {
                    output: "memory_delete tier must be one of: session, project, global"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
            None => {
                if project_id.is_some() {
                    MemoryTier::Project
                } else if session_id.is_some() {
                    MemoryTier::Session
                } else if allow_global {
                    MemoryTier::Global
                } else {
                    return Ok(ToolResult {
                        output: "memory_delete requires a current session/project context or global memory enabled by policy".to_string(),
                        metadata: json!({"ok": false, "reason": "missing_scope"}),
                    });
                }
            }
        };

        if matches!(tier, MemoryTier::Session) && session_id.is_none() {
            return Ok(ToolResult {
                output: "tier=session requires session_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_session_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Project) && project_id.is_none() {
            return Ok(ToolResult {
                output: "tier=project requires project_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_project_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Global) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let db_path = resolve_memory_db_path(&args);
        let manager = MemoryManager::new(&db_path).await?;
        let deleted = manager
            .db()
            .delete_chunk(tier, chunk_id, project_id.as_deref(), session_id.as_deref())
            .await?;

        if deleted == 0 {
            return Ok(ToolResult {
                output: format!("memory chunk `{chunk_id}` not found in {tier} memory"),
                metadata: json!({
                    "ok": false,
                    "reason": "not_found",
                    "chunk_id": chunk_id,
                    "tier": tier.to_string(),
                    "session_id": session_id,
                    "project_id": project_id,
                    "allow_global": allow_global,
                    "db_path": db_path,
                }),
            });
        }

        Ok(ToolResult {
            output: format!("deleted memory chunk `{chunk_id}` from {tier} memory"),
            metadata: json!({
                "ok": true,
                "deleted": true,
                "chunk_id": chunk_id,
                "count": deleted,
                "tier": tier.to_string(),
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "db_path": db_path,
            }),
        })
    }
}

fn resolve_memory_db_path(args: &Value) -> PathBuf {
    if let Some(path) = args
        .get("__memory_db_path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("TANDEM_MEMORY_DB_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(state_dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = state_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("memory.sqlite");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("tandem").join("memory.sqlite");
    }
    PathBuf::from("memory.sqlite")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MemoryVisibleScope {
    Session,
    Project,
    Global,
}

fn parse_memory_visible_scope(raw: &str) -> Option<MemoryVisibleScope> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "session" => Some(MemoryVisibleScope::Session),
        "project" | "workspace" => Some(MemoryVisibleScope::Project),
        "global" => Some(MemoryVisibleScope::Global),
        _ => None,
    }
}

fn memory_visible_scope(args: &Value) -> MemoryVisibleScope {
    if let Some(scope) = args
        .get("__memory_max_visible_scope")
        .and_then(|v| v.as_str())
        .and_then(parse_memory_visible_scope)
    {
        return scope;
    }
    if let Ok(raw) = std::env::var("TANDEM_MEMORY_MAX_VISIBLE_SCOPE") {
        if let Some(scope) = parse_memory_visible_scope(&raw) {
            return scope;
        }
    }
    MemoryVisibleScope::Global
}

fn memory_session_id(args: &Value) -> Option<String> {
    args.get("session_id")
        .or_else(|| args.get("__session_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn memory_project_id(args: &Value) -> Option<String> {
    args.get("project_id")
        .or_else(|| args.get("__project_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn global_memory_enabled(args: &Value) -> bool {
    if memory_visible_scope(args) != MemoryVisibleScope::Global {
        return false;
    }
    if let Some(explicit) = args.get("allow_global").and_then(|v| v.as_bool()) {
        return explicit;
    }
    match std::env::var("TANDEM_ENABLE_GLOBAL_MEMORY") {
        Ok(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

struct SkillTool;
#[async_trait]
impl Tool for SkillTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "skill",
            "List or load installed Tandem skills. Call without name to list available skills; provide name to load full SKILL.md content.",
            json!({"type":"object","properties":{"name":{"type":"string"}}}),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workspace_root = std::env::current_dir().ok();
        let service = SkillService::for_workspace(workspace_root);
        let requested = args["name"].as_str().map(str::trim).unwrap_or("");
        let allowed_skills = parse_allowed_skills(&args);

        if requested.is_empty() {
            let mut skills = service.list_skills().unwrap_or_default();
            if let Some(allowed) = &allowed_skills {
                skills.retain(|s| allowed.contains(&s.name));
            }
            if skills.is_empty() {
                return Ok(ToolResult {
                    output: "No skills available.".to_string(),
                    metadata: json!({"count": 0, "skills": []}),
                });
            }
            let mut lines = vec![
                "Available Tandem skills:".to_string(),
                "<available_skills>".to_string(),
            ];
            for skill in &skills {
                lines.push("  <skill>".to_string());
                lines.push(format!("    <name>{}</name>", skill.name));
                lines.push(format!(
                    "    <description>{}</description>",
                    escape_xml_text(&skill.description)
                ));
                lines.push(format!("    <location>{}</location>", skill.path));
                lines.push("  </skill>".to_string());
            }
            lines.push("</available_skills>".to_string());
            return Ok(ToolResult {
                output: lines.join("\n"),
                metadata: json!({"count": skills.len(), "skills": skills}),
            });
        }

        if let Some(allowed) = &allowed_skills {
            if !allowed.contains(requested) {
                let mut allowed_list = allowed.iter().cloned().collect::<Vec<_>>();
                allowed_list.sort();
                return Ok(ToolResult {
                    output: format!(
                        "Skill \"{}\" is not enabled for this agent. Enabled skills: {}",
                        requested,
                        allowed_list.join(", ")
                    ),
                    metadata: json!({"name": requested, "enabled": allowed_list}),
                });
            }
        }

        let loaded = service.load_skill(requested).map_err(anyhow::Error::msg)?;
        let Some(skill) = loaded else {
            let available = service
                .list_skills()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>();
            return Ok(ToolResult {
                output: format!(
                    "Skill \"{}\" not found. Available skills: {}",
                    requested,
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
                metadata: json!({"name": requested, "matches": [], "available": available}),
            });
        };

        let files = skill
            .files
            .iter()
            .map(|f| format!("<file>{}</file>", f))
            .collect::<Vec<_>>()
            .join("\n");
        let output = [
            format!("<skill_content name=\"{}\">", skill.info.name),
            format!("# Skill: {}", skill.info.name),
            String::new(),
            skill.content.trim().to_string(),
            String::new(),
            format!("Base directory for this skill: {}", skill.base_dir),
            "Relative paths in this skill are resolved from this base directory.".to_string(),
            "Note: file list is sampled.".to_string(),
            String::new(),
            "<skill_files>".to_string(),
            files,
            "</skill_files>".to_string(),
            "</skill_content>".to_string(),
        ]
        .join("\n");
        Ok(ToolResult {
            output,
            metadata: json!({
                "name": skill.info.name,
                "dir": skill.base_dir,
                "path": skill.info.path
            }),
        })
    }
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_allowed_skills(args: &Value) -> Option<HashSet<String>> {
    let values = args
        .get("allowed_skills")
        .or_else(|| args.get("allowedSkills"))
        .and_then(|v| v.as_array())?;
    let out = values
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    Some(out)
}

struct ApplyPatchTool;
#[async_trait]
impl Tool for ApplyPatchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "apply_patch",
            "Apply a Codex-style patch in a git workspace, or validate patch text when git patching is unavailable",
            json!({"type":"object","properties":{"patchText":{"type":"string"}}}),
            apply_patch_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let patch = args["patchText"].as_str().unwrap_or("");
        let has_begin = patch.contains("*** Begin Patch");
        let has_end = patch.contains("*** End Patch");
        let patch_paths = extract_apply_patch_paths(patch);
        let file_ops = patch_paths.len();
        let valid = has_begin && has_end && file_ops > 0;
        if !valid {
            return Ok(ToolResult {
                output: "Invalid patch format. Expected Begin/End markers and at least one file operation."
                    .to_string(),
                metadata: json!({"valid": false, "fileOps": file_ops}),
            });
        }
        let workspace_root =
            workspace_root_from_args(&args).unwrap_or_else(|| effective_cwd_from_args(&args));
        let git_root = resolve_git_root_for_dir(&workspace_root).await;
        if let Some(git_root) = git_root {
            let denied_paths = patch_paths
                .iter()
                .filter_map(|rel| {
                    let resolved = git_root.join(rel);
                    if is_within_workspace_root(&resolved, &workspace_root) {
                        None
                    } else {
                        Some(rel.clone())
                    }
                })
                .collect::<Vec<_>>();
            if !denied_paths.is_empty() {
                return Ok(ToolResult {
                    output: format!(
                        "patch denied by workspace policy for paths: {}",
                        denied_paths.join(", ")
                    ),
                    metadata: json!({
                        "valid": true,
                        "applied": false,
                        "reason": "path_outside_workspace",
                        "paths": patch_paths
                    }),
                });
            }
            let tmp_name = format!(
                "tandem-apply-patch-{}-{}.patch",
                std::process::id(),
                now_millis()
            );
            let patch_path = std::env::temp_dir().join(tmp_name);
            fs::write(&patch_path, patch).await?;
            let output = Command::new("git")
                .current_dir(&git_root)
                .arg("apply")
                .arg("--3way")
                .arg("--recount")
                .arg("--whitespace=nowarn")
                .arg(&patch_path)
                .output()
                .await?;
            let _ = fs::remove_file(&patch_path).await;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let ok = output.status.success();
            return Ok(ToolResult {
                output: if ok {
                    if stdout.is_empty() {
                        "ok".to_string()
                    } else {
                        stdout.clone()
                    }
                } else if stderr.is_empty() {
                    "git apply failed".to_string()
                } else {
                    stderr.clone()
                },
                metadata: json!({
                    "valid": true,
                    "applied": ok,
                    "paths": patch_paths,
                    "git_root": git_root.to_string_lossy(),
                    "stdout": stdout,
                    "stderr": stderr
                }),
            });
        }
        Ok(ToolResult {
            output: "Patch format validated, but no git workspace was detected. Use `edit` for existing files or `write` for new files in this workspace."
                .to_string(),
            metadata: json!({
                "valid": true,
                "applied": false,
                "reason": "git_workspace_unavailable",
                "paths": patch_paths
            }),
        })
    }
}

