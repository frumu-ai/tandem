# Tandem Core HTTP Contracts

This document establishes the expected payloads and canonical behavior for the core HTTP API surface across all SDKs. Every SDK implemented must parse these exact shapes correctly.

## 1. Global Health (`/global/health`)

- **Method:** `GET`
- **Wire Response:** `{"ready": true, "phase": "startup"}`
- **SDK Normalized Response:** `SystemHealth`

## 2. Session List (`/session`)

- **Method:** `GET`
- **Wire Response:** `{"sessions": [{"id": "s_123", "title": "Example", "createdAtMs": 1700000000, "workspaceRoot": "/app"}], "count": 1}`
- **SDK Normalized Response:** `SessionListResponse` containing `[SessionRecord]`

## 3. Session Run Trigger (`/session/:id/prompt_async`)

- **Method:** `POST`
- **Input:** `{"parts": [{"type": "text", "text": "Prompt"}]}`
- **Wire Response:** `{"runID": "r_123"}`
- **Conflict Response (409):** `{"activeRun": {"runId": "r_123"}}`
- **SDK Normalized Response:** Parses canonical `runId` explicitly.

## 4. Key-Value Resources (`/resource`)

- **Method:** `GET`
- **Wire Response:** `{"items": [{"key": "status", "value": "active", "updatedAtMs": 1700000000}], "count": 1}`
- **SDK Normalized Response:** Canonical fields (`key`, `value`, `updatedAtMs` (TS) / `updated_at_ms` (Py)).

## 5. Global Memory (`/memory/*`)

- **Method:** `POST /memory/put`
- **Input:** `{"run_id":"r_123","user_id":"u_123","content":"Use sqlite WAL mode","source_type":"assistant_final","visibility":"private","project_tag":"repo-a","channel_tag":"web"}`
- **Wire Response:** `{"stored":true,"memoryID":"m_1","deduped":false}`
- **SDK Normalized Response:** `MemoryPutResponse` with compatibility for `memoryID`/`memory_id`.

- **Method:** `POST /memory/search`
- **Input:** `{"query":"sqlite wal","run_id":"r_123","user_id":"u_123","limit":5}`
- **Wire Response:** `{"results":[{"id":"m_1","content":"Use sqlite WAL mode","sourceType":"assistant_final","runId":"r_120","score":0.92}],"count":1}`
- **SDK Normalized Response:** Canonical memory records (`content` primary; `text` alias accepted) plus source/run aliases.

- **Method:** `POST /memory/import`
- **Input:** `{"source":{"kind":"path","path":"/srv/tandem/imports/company-docs"},"format":"directory","tier":"project","project_id":"company-brain-demo","session_id":null,"sync_deletes":true}`
- **Wire Response:** `{"ok":true,"source":{"kind":"path","path":"/srv/tandem/imports/company-docs"},"format":"directory","tier":"project","project_id":"company-brain-demo","session_id":null,"sync_deletes":true,"discovered_files":42,"files_processed":42,"indexed_files":39,"skipped_files":3,"deleted_files":0,"chunks_created":312,"errors":0}`
- **Validation:** `source.kind` supports `path` only. `source.path` must be non-empty, exist, and be readable. `tier=project` requires `project_id`; `tier=session` requires `session_id`. Invalid requests return `400`; importer/runtime failures return `500`.
- **Formats:** `directory` imports markdown/text files from a path. `openclaw` imports OpenClaw-style memory roots such as `MEMORY.md` and `memory/**/*.md`. This route reuses the engine memory importer and does not shell out to `tandem-engine`.
- **SDK Normalized Response:** `MemoryImportResponse`; TypeScript exposes `client.memory.importPath(...)`, Python exposes `client.memory.import_path(...)`.

- **Method:** `GET /memory?user_id=u_123&query=sqlite&limit=20&offset=0`
- **Wire Response:** `{"items":[{"id":"m_1","content":"Use sqlite WAL mode","user_id":"u_123","visibility":"private"}],"count":1}`
- **SDK Normalized Response:** `MemoryListResponse` with global-user filtering.

- **Method:** `POST /memory/promote`
- **Input:** `{"run_id":"r_123","source_memory_id":"m_1","to_tier":"shared"}`
- **Wire Response:** `{"promoted":true,"newMemoryID":"m_2"}`
- **SDK Normalized Response:** `MemoryPromoteResponse` with compatibility for `newMemoryID`/`new_memory_id`.

- **Method:** `POST /memory/demote`
- **Input:** `{"id":"m_2","run_id":"r_123"}`
- **Wire Response:** `{"ok":true,"id":"m_2","visibility":"private","demoted":true}`
- **SDK Normalized Response:** `MemoryDemoteResponse`.

- **Method:** `DELETE /memory/{id}`
- **Wire Response:** `{"ok":true}`

## 6. Definitions (`/routines`)

- **Method:** `GET`
- **Wire Response:** `{"routines": [{"id": "rt_1", "status": "enabled", "requiresApproval": true}]}`
- **SDK Normalized Response:** Canonical fields (`requiresApproval` (TS) / `requires_approval` (Py)).
