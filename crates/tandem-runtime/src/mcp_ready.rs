//! MCP readiness gate (Invariant 2 of `docs/SPINE.md`).
//!
//! Every concrete MCP tool call should reach a connected server through a
//! single readiness check, or fail fast with a typed error. This module is
//! the destination for that gate; Phase 2 fills it in. Today the reconnect
//! logic is spread across `mcp_parts/part0{1,2,3}.rs` (commits `852c453`,
//! `f6bf753`, `e88e951`).
//!
//! TODO(spine, phase-2):
//!   * Define `enum McpReadyError { NotConnected, Reconnecting, DeadServer,
//!     SchemaMismatch, Other(String) }`.
//!   * Define `pub async fn ensure_mcp_ready(server: &str, tool: &str)
//!     -> Result<McpConn, McpReadyError>` here.
//!   * Make the raw connection handle private to this crate so every
//!     caller must obtain it through `ensure_mcp_ready`.
//!   * Stress test: kill the MCP server mid-run; the run pauses cleanly
//!     with a typed error instead of panicking or hanging.
