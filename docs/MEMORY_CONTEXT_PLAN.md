# Memory Context Implementation Plan for Tandem

## Executive Summary

This document outlines a comprehensive plan for adding **Semantic Memory** to Tandem.
The solution involves extracting memory logic into a shared crate (`crates/tandem-memory`) to be used by both the Tauri App and the TUI/CLI, ensuring a unified "Brain" for the agent.

> **Status Update (2026-02-15):** A memory system already exists in `src-tauri/src/memory/`. This plan has been updated to reflect the current implementation and identify remaining gaps.

## Current Implementation Status

### Already Implemented (`src-tauri/src/memory/`)

| Component | File | Status |
|-----------|------|--------|
| Database Layer | `db.rs` | ✅ Complete - SQLite + sqlite-vec |
| Embedding Service | `embeddings.rs` | ⚠️ Placeholder (deterministic pseudo-embeddings) |
| Memory Manager | `manager.rs` | ✅ Complete - store, retrieve, cleanup |
| Text Chunking | `chunking.rs` | ✅ Complete - semantic chunking with overlap |
| File Indexer | `indexer.rs` | ✅ Complete - workspace file indexing |
| Type Definitions | `types.rs` | ✅ Complete - MemoryTier, MemoryChunk, etc. |

### Current Schema (Already in Production)

```sql
-- Session memory (ephemeral)
CREATE TABLE session_memory_chunks (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    session_id TEXT NOT NULL,
    project_id TEXT,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    token_count INTEGER NOT NULL DEFAULT 0,
    metadata TEXT
);

CREATE VIRTUAL TABLE session_memory_vectors USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding float[384]
);

-- Project memory (persistent)
CREATE TABLE project_memory_chunks (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    project_id TEXT NOT NULL,
    session_id TEXT,
    source TEXT NOT NULL,
    source_path TEXT,        -- File tracking
    source_mtime INTEGER,
    source_size INTEGER,
    source_hash TEXT,
    created_at TEXT NOT NULL,
    token_count INTEGER NOT NULL DEFAULT 0,
    metadata TEXT
);

CREATE VIRTUAL TABLE project_memory_vectors USING vec0(
    chunk_id TEXT PRIMARY KEY,
    embedding float[384]
);

-- Global memory (cross-project)
CREATE TABLE global_memory_chunks (...);
CREATE VIRTUAL TABLE global_memory_vectors USING vec0(...);

-- Configuration
CREATE TABLE memory_config (
    project_id TEXT PRIMARY KEY,
    max_chunks INTEGER NOT NULL DEFAULT 10000,
    chunk_size INTEGER NOT NULL DEFAULT 512,
    retrieval_k INTEGER NOT NULL DEFAULT 5,
    ...
);

-- File indexing state
CREATE TABLE project_file_index (...);
CREATE TABLE project_index_status (...);
```

## Core Vision Alignment

- **Local-First & Private:** All data stored in a local SQLite database (`.tandem/memory.sqlite`).
- **Unified Engine:** Logic resides in `tandem-memory`, accessible to `tandem-core` and all interfaces (GUI/TUI).
- **Session-Linear:** Memory operations are atomic and follow the linear session history.

---

## Architecture Design

### 1. New Crate: `crates/tandem-memory`

We will move the memory logic out of `src-tauri` into a dedicated workspace crate.

**Rationale:**
- TUI (`tandem-tui`) currently has no access to memory features
- `tandem-core` cannot use memory for context injection
- Code duplication risk between GUI and CLI interfaces

```toml
[package]
name = "tandem-memory"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"

[dependencies]
rusqlite = { version = "0.32", features = ["bundled", "uuid", "blob"] }
sqlite-vec = "0.1"
fastembed = { version = "4", default-features = false, features = ["ort-download-binaries"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["sync"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
tokenizers = "0.19"  # For chunking

[dev-dependencies]
tempfile = "3"
```

### 2. Migration Strategy

**Phase A: Extract and Refactor**

1. Create `crates/tandem-memory/` with the existing code
2. Move files:
   - `src-tauri/src/memory/db.rs` → `crates/tandem-memory/src/db.rs`
   - `src-tauri/src/memory/embeddings.rs` → `crates/tandem-memory/src/embeddings.rs`
   - `src-tauri/src/memory/manager.rs` → `crates/tandem-memory/src/manager.rs`
   - `src-tauri/src/memory/chunking.rs` → `crates/tandem-memory/src/chunking.rs`
   - `src-tauri/src/memory/indexer.rs` → `crates/tandem-memory/src/indexer.rs`
   - `src-tauri/src/memory/types.rs` → `crates/tandem-memory/src/types.rs`

3. Remove Tauri-specific dependencies (use generic paths)
4. Add `tandem-memory` to workspace `Cargo.toml`

**Phase B: Update Consumers**

1. `src-tauri/Cargo.toml` → add `tandem-memory` dependency
2. `crates/tandem-core/Cargo.toml` → add `tandem-memory` dependency
3. `crates/tandem-tui/Cargo.toml` → add `tandem-memory` dependency
4. Update imports in all consumers

### 3. Embeddings: `fastembed-rs` (ONNX)

**Current State:** The `EmbeddingService` uses `generate_deterministic_embedding()` which creates pseudo-embeddings from text hashes. This provides no semantic search capability.

**Target State:** Use `fastembed` to run `all-MiniLM-L6-v2` locally.

```rust
// embeddings.rs - Target implementation
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct EmbeddingService {
    model: TextEmbedding,
    dimension: usize,
}

impl EmbeddingService {
    pub fn new() -> MemoryResult<Self> {
        let model = TextEmbedding::try_new(InitOptions {
            model_name: EmbeddingModel::AllMiniLML6V2,
            ..Default::default()
        })?;
        
        Ok(Self {
            model,
            dimension: 384,
        })
    }
    
    pub async fn embed(&self, text: &str) -> MemoryResult<Vec<f32>> {
        let embeddings = self.model.embed(vec![text.to_string()], None)?;
        Ok(embeddings.into_iter().next().unwrap_or_default())
    }
    
    pub async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Vec<f32>>> {
        let embeddings = self.model.embed(texts.to_vec(), None)?;
        Ok(embeddings)
    }
}
```

### 3.1 Embedding Model Download Mechanism

**Question:** Where and when does the embedding model download happen? Is it compiled in the engine or downloaded?

**Answer:** The model is **downloaded at runtime on first use**, NOT compiled into the engine binary.

#### Download Mechanism Breakdown

| Component | When | Where | Size |
|-----------|------|-------|------|
| **ONNX Runtime binaries** | Compile time (via `ort-download-binaries` feature) | Embedded in binary | ~15-30MB |
| **Embedding model weights** | Runtime (first `TextEmbedding::try_new()` call) | HuggingFace Hub cache | ~30MB |

#### How fastembed Works

1. **Compile Time:**
   - The `fastembed` crate with `ort-download-binaries` feature downloads ONNX Runtime native libraries during build
   - These are linked into the final binary (no separate DLLs/dylibs needed)
   - This ensures the inference engine is available immediately

2. **Runtime (First Use):**
   - When `TextEmbedding::try_new(EmbeddingModel::AllMiniLML6V2)` is called:
   - fastembed checks local HuggingFace cache for the model
   - If not found, downloads from `https://huggingface.co/{model_repo}`
   - Model is cached for future use

3. **Cache Locations:**
   - **Windows:** `C:\Users\{user}\.cache\huggingface\hub\models--BAAI--bge-small-en-v1.5\`
   - **macOS/Linux:** `~/.cache/huggingface/hub/models--BAAI--bge-small-en-v1.5\`
   - **Override:** Set `HF_HOME` or `HUGGINGFACE_HUB_CACHE` env var

#### Model Download UX Options

| Option | Pros | Cons | Recommendation |
|--------|------|------|----------------|
| **Download on first use** | Smaller binary, always latest model | Requires internet on first run, ~30s delay | Default |
| **Bundle model in binary** | Works offline, instant startup | +30MB binary size, model can become stale | Optional for air-gapped envs |
| **Download on install** | No delay on first use | Complex install script | Not recommended |

#### Recommended Implementation

```rust
// In EmbeddingService::new()
pub fn new() -> MemoryResult<Self> {
    // Try to load model, with progress feedback
    let model = match TextEmbedding::try_new(InitOptions {
        model_name: EmbeddingModel::AllMiniLML6V2,
        ..Default::default()
    }) {
        Ok(m) => m,
        Err(e) => {
            // Log warning and fall back to deterministic embeddings
            tracing::warn!("Failed to load embedding model, falling back to deterministic: {}", e);
            return Ok(Self::fallback_deterministic());
        }
    };
    
    Ok(Self {
        model,
        dimension: 384,
    })
}

// Progress callback for UI
pub fn new_with_progress<F: Fn(f32)>(on_progress: F) -> MemoryResult<Self> {
    // fastembed doesn't expose download progress directly
    // We can check cache first and report 100% if cached
    if is_model_cached() {
        on_progress(1.0);
        return Self::new();
    }
    
    // Otherwise, spawn a background task and poll
    // (Implementation depends on fastembed API support)
}
```

#### Offline/Air-Gapped Support

For users without internet access or who want to bundle the model:

1. **Pre-download the model:**
   ```bash
   # Run once on a machine with internet
   cargo run -p tandem-memory --example download-model
   ```

2. **Copy cache to target machine:**
   - Copy `~/.cache/huggingface/hub/models--BAAI--bge-small-en-v1.5/` to the same location

3. **Or set cache directory:**
   ```bash
   export HF_HOME=/path/to/bundled/models
   ```

**Considerations:**
- Model download: ~30MB on first use (requires internet)
- CPU inference: ~10-50ms per chunk
- Memory overhead: ~100MB for model
- Offline support: Pre-cache model or bundle with installer

---

## Features & Workflows

### Feature 1: The "Context Loop" (Automatic Injection)

Inspired by Agent Zero's `_50_recall_memories.py`, we need a **Proactive Recall** system.

**Current State:** Memory retrieval exists but is not automatically injected into prompts.

**Target Workflow:**
1. **User sends message:** "Fix the bug in that file we talked about."
2. **Engine Intercept:**
   - Generate a search query (via cheap LLM call or heuristics).
   - Query `tandem-memory`.
3. **Context Injection:**
   - Top 5 relevant chunks are PREPENDED to the `system` prompt or `user` message.
   - Format:
     ```xml
     <memory_context>
       <fact>User prefers Rust over Python.</fact>
       <code_snippet source="main.rs">fn main() { ... }</code_snippet>
     </memory_context>
     ```
4. **Agent Action:** Agent sees the context and knows "that file" refers to `main.rs`.

**Implementation Location:** `crates/tandem-core/src/engine_loop.rs`

```rust
// Pseudo-code for context injection
pub async fn run_prompt_async_with_context(
    &self,
    session_id: String,
    req: SendMessageRequest,
    correlation_id: Option<String>,
) -> anyhow::Result<()> {
    // NEW: Memory retrieval before prompt
    if let Some(memory) = &self.memory_store {
        let query = extract_search_query(&req);
        let context = memory.retrieve_context(&query, &session_id).await?;
        req = inject_memory_context(req, context);
    }
    
    // ... existing prompt logic
}
```

### Feature 2: Explicit Memory Tool

The agent should also have the ability to *intentionally* search memory if the automatic injection wasn't enough.

**Tool Definition (`tandem-tools`):**
```rust
// Add to ToolRegistry in tandem-tools/src/lib.rs
map.insert("memory_search".to_string(), Arc::new(MemorySearchTool));

// Tool implementation
pub struct MemorySearchTool;

#[async_trait]
impl Tool for MemorySearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_search".to_string(),
            description: "Search your long-term memory for facts, code snippets, or past decisions.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The semantic search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
        }
    }
    
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        // Requires memory store to be accessible
        // This is a gap - tools don't currently have access to memory
    }
}
```

**Gap Identified:** Tools in `tandem-tools` don't have access to the memory store. Need to either:
1. Pass memory store reference to tools
2. Create a separate memory tool in `tandem-core`
3. Use a trait-based dependency injection

### Feature 3: Session Summarization

To prevent context window overflow, we periodically compress the session history.

**Current State:** Not implemented.

**Target Logic:**
- Every 10 turns, the engine takes the oldest messages.
- Calls an LLM to "Summarize the key decisions and facts from this conversation segment."
- Stores the summary in `session_summaries` and embeds it into `chunks`.
- Drops the raw messages from the prompt tokens.

**New Table Required:**
```sql
CREATE TABLE session_summaries (
    session_id TEXT NOT NULL,
    turn_start INTEGER NOT NULL,
    turn_end INTEGER NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_id, turn_start)
);
```

---

## Implementation Phases

### Phase 1: Refactor to `tandem-memory` Crate (P0)

- [ ] Create `crates/tandem-memory/` with `Cargo.toml`
- [ ] Move `src-tauri/src/memory/*.rs` → `crates/tandem-memory/src/`
- [ ] Remove Tauri-specific imports, use generic `std::path::PathBuf`
- [ ] Add `tandem-memory` to workspace members
- [ ] Update `src-tauri` to use `tandem-memory`
- [ ] Run tests to verify migration
- [ ] Update `tandem-core` to use `tandem-memory`
- [ ] Update `tandem-tui` to use `tandem-memory`

### Phase 2: Real Embeddings with fastembed (P0)

- [ ] Add `fastembed` dependency to `tandem-memory`
- [ ] Implement `EmbeddingService::new()` with model loading
- [ ] Implement `embed()` and `embed_batch()` with real inference
- [ ] Add fallback to deterministic embeddings if model fails to load
- [ ] Add configuration for model selection
- [ ] Test embedding quality with semantic search queries

### Phase 3: Engine Integration (P1)

- [ ] Add `MemoryStore` to `EngineLoop` struct
- [ ] Initialize `MemoryStore` in `EngineLoop::new()`
- [ ] Implement context injection in `run_prompt_async_with_context()`
- [ ] Add memory retrieval telemetry events
- [ ] Wire up `memory_store_message` on message send

### Phase 4: Memory Search Tool (P1)

- [ ] Design tool-memory integration pattern
- [ ] Implement `memory_search` tool in `tandem-tools` or `tandem-core`
- [ ] Add tool to default tool registry
- [ ] Test tool invocation from agent

### Phase 5: Session Summarization (P2)

- [ ] Add `session_summaries` table to schema
- [ ] Implement turn counting in session storage
- [ ] Implement summarization trigger logic
- [ ] Implement LLM-based summarization
- [ ] Store summaries and update embeddings
- [ ] Test context window reduction

### Phase 6: Frontend Control (P2)

- [ ] Add "Memory" toggle in the UI (to enable/disable context injection)
- [ ] Show "Retrieved Context" in the message log for debugging
- [ ] Add memory stats display in Settings
- [ ] Add per-project memory configuration UI

---

## Gaps and Improvements Identified

### Critical Gaps

| Gap | Description | Priority |
|-----|-------------|----------|
| **Pseudo-embeddings** | Current implementation uses hash-based embeddings with no semantic meaning | P0 |
| **No TUI memory access** | TUI cannot use memory features at all | P0 |
| **No context injection** | Memory is stored but not automatically retrieved for prompts | P1 |
| **No memory tool** | Agent cannot explicitly search memory | P1 |
| **No session summarization** | Context window can overflow on long sessions | P2 |

### Schema Improvements

| Improvement | Description |
|-------------|-------------|
| Add `role` column | Track message role (user/assistant/system/tool) for better context |
| Add `session_summaries` table | Store compressed conversation segments |
| Add `embedding_model` column | Track which model generated embeddings (for migration) |

### API Improvements

| Improvement | Description |
|-------------|-------------|
| `MemoryStore` trait | Abstract interface for different memory backends |
| Async-first API | All operations should be async for TUI compatibility |
| Batch operations | Optimize for bulk insert/retrieve |
| Streaming retrieval | Return results as they're found |

### Configuration Improvements

| Setting | Description | Default |
|---------|-------------|---------|
| `embedding_model` | Which model to use for embeddings | `all-MiniLM-L6-v2` |
| `auto_inject` | Whether to automatically inject context | `true` |
| `injection_token_budget` | Max tokens for injected context | `2000` |
| `summarization_threshold` | Turns before summarization | `10` |

---

## Testing Strategy

### Unit Tests

- [ ] Embedding generation (deterministic vs semantic)
- [ ] Chunking logic (overlap, token counting)
- [ ] Database operations (CRUD, search)
- [ ] Context formatting for injection

### Integration Tests

- [ ] End-to-end memory lifecycle (store → retrieve → inject)
- [ ] Multi-project isolation
- [ ] Session summarization flow
- [ ] TUI memory access

### Performance Tests

- [ ] Embedding latency (target: <50ms per chunk)
- [ ] Search latency (target: <100ms for 10k chunks)
- [ ] Memory usage (target: <200MB for model + database)

---

## Dependencies

### New Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `fastembed` | 4.x | ONNX-based embedding inference |
| `tokenizers` | 0.19.x | HuggingFace tokenizers for chunking |

### Existing Dependencies (to preserve)

| Crate | Purpose |
|-------|---------|
| `rusqlite` | SQLite database |
| `sqlite-vec` | Vector similarity search |
| `tokio` | Async runtime |
| `serde` | Serialization |
| `chrono` | Timestamps |
| `uuid` | ID generation |

---

## Embedding Model Download Mechanism

### Current State: No Model Download

The current implementation in [`src-tauri/src/memory/embeddings.rs`](../src-tauri/src/memory/embeddings.rs) uses **pseudo-embeddings**:

```rust
/// Note: This is a simplified implementation that generates deterministic
/// pseudo-embeddings based on text content. In a production environment,
/// this should be replaced with actual fastembed or onnxruntime-based
/// embedding generation.
pub async fn embed(&self, text: &str) -> MemoryResult<Vec<f32>> {
    Ok(self.generate_deterministic_embedding(text))
}
```

**Key points:**
- No model is downloaded or loaded
- Uses hash-based deterministic vectors (same text = same vector)
- Provides NO semantic search capability
- Vectors are normalized but have no meaning

### Target State: fastembed with Runtime Download

When we implement real embeddings via `fastembed`, the model download happens as follows:

#### 1. ONNX Runtime Binaries (Compile-time)

The `Cargo.toml` includes:
```toml
fastembed = { version = "4", default-features = false, features = ["ort-download-binaries"] }
```

The `ort-download-binaries` feature downloads **ONNX Runtime native libraries** at **compile time**:
- These are platform-specific shared libraries (.dll/.so/.dylib)
- Downloaded during `cargo build` via build.rs
- Bundled into the final binary/app
- Size: ~10-20MB per platform

#### 2. Embedding Model Weights (Runtime)

The embedding model (`all-MiniLM-L6-v2`) is downloaded **at runtime on first use**:

```rust
let model = TextEmbedding::try_new(InitOptions {
    model_name: EmbeddingModel::AllMiniLML6V2,
    ..Default::default()
})?;
```

**Download behavior:**
- **When:** First call to `EmbeddingService::new()` or first embedding generation
- **Where:** Model is cached in user's local cache directory:
  - Windows: `C:\Users\<user>\.cache\huggingface\hub\models--BAAI--bge-small-en-v1.5\`
  - macOS: `~/.cache/huggingface/hub/models--BAAI--bge-small-en-v1.5/`
  - Linux: `~/.cache/huggingface/hub/models--BAAI--bge-small-en-v1.5/`
- **Size:** ~30-50MB for `all-MiniLM-L6-v2`
- **Network:** Requires internet access on first run
- **Subsequent runs:** Uses cached model (no download)

#### 3. UX Considerations

| Scenario | Behavior |
|----------|----------|
| First launch with internet | Model downloads automatically (30-50MB, ~10-30 seconds) |
| First launch offline | Error: model not found, fallback to pseudo-embeddings |
| Subsequent launches | Instant startup (uses cached model) |
| App update | No re-download (model cached separately) |

**Recommended UX:**
1. Show a "Downloading embedding model..." progress indicator on first use
2. Allow offline fallback to pseudo-embeddings with a warning
3. Add a "Pre-download embedding model" option in Settings for offline prep

#### 4. Alternative: Bundle Model with App

To avoid runtime downloads entirely:

```toml
# In Cargo.toml
fastembed = { version = "4", default-features = false, features = ["ort-download-binaries"] }

# In build.rs or via include_dir
# Bundle model files from ~/.cache/huggingface into the app
```

**Pros:**
- Works offline from first launch
- No network dependency

**Cons:**
- Increases app size by ~30-50MB
- Model updates require app updates

**Recommendation:** Start with runtime download, add bundled model option for enterprise/offline users.

---

### 3.2 Modular Embedding Models (Language Support)

To support non-English languages and different performance profiles, the embedding model must be **configurable** by the developer or user.

#### Supported Models (via fastembed)

| Model | Language | Size | Dimensions | Notes |
|-------|----------|------|------------|-------|
| `BGE-Small-EN-v1.5` | **English** | 34MB | 384 | **Recommended Default.** Good balance of speed/accuracy. |
| `BGE-Small-ZH-v1.5` | Chinese | 34MB | 512 | Optimized for Chinese. |
| `Multilingual-E5-Small` | **90+ Langs** | 120MB | 384 | Heavier, but supports Spanish, French, German, etc. |
| `All-MiniLM-L6-v2` | English | 23MB | 384 | Fastest, slightly less accurate. |

#### Configuration Design

We will add an `embedding_model` field to the global/project config.

**Config (`config.json`):**
```json
{
  "memory": {
    "embedding_model": "bge-small-en-v1.5" 
  }
}
```

**Implementation:**
The `EmbeddingService` will accept a configuration string and map it to `fastembed::EmbeddingModel`.

```rust
pub enum SupportedModel {
    BGESmallEN,
    MultilingualE5,
    MiniLM,
    // ...
}

// In EmbeddingService
pub fn from_config(model_id: &str) -> Self {
    let model = match model_id {
        "multilingual-e5" => EmbeddingModel::MultilingualE5Small,
        _ => EmbeddingModel::BGESmallENV15, // Default
    };
    // ...
}
```

This allows developers to swap the "brain" of the memory system just by changing a config string, without recompiling.

#### BYO Model (Bring Your Own Model)

For true modularity, we will also support loading a custom ONNX model from a local file path. This enables:
- **Air-gapped usage:** Point to a model file on a USB drive.
- **Custom models:** Use fine-tuned models critical for enterprise domains (medical, legal).
- **Zero-download:** Skip the fastembed download entirely.

**Config (`config.json`):**
```json
{
  "memory": {
    "embedding_model": "custom",
    "local_model_path": "C:/Models/my-special-model.onnx",
    "embedding_dimension": 768
  }
}
```

**Implementation:**
If `embedding_model` is "custom", the `EmbeddingService` will initialize `fastembed` (or `ort` directly) using the `local_model_path`.


5. **Summarization model:** Use the same model as chat or a cheaper/faster model?

---

## References

- Agent Zero Memory: `agent-zero/lib/memory.py`
- sqlite-vec: https://github.com/asg017/sqlite-vec
- fastembed: https://github.com/Anush008/fastembed-rs
- Existing implementation: `tandem/src-tauri/src/memory/`
