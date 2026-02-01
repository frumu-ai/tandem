# Tandem Project Review: Improvements & Security Assessment

This document provides a comprehensive review of the Tandem project, focusing on improving the AI session experience and identifying potential security vulnerabilities.

## 1. LLM Session Message Improvements

The current implementation of message handling between the OpenCode sidecar and the Tandem frontend can be improved to provide a cleaner, more intuitive user experience, especially for long or complex AI interactions.

### A. Collapsible Reasoning (Thinking Blocks)
Many modern LLMs provide a "reasoning" or "thinking" phase before generating the final answer.
- **Current State**: Reasoning parts are merged into the main message content, often appearing as raw text or sometimes with `[REDACTED]` markers.
- **Recommendation**:
    - Modify `src-tauri/src/sidecar.rs` to emit a distinct `StreamEvent::Reasoning` event when the sidecar sends a `reasoning` part.
    - Update `src/components/chat/Message.tsx` to include a `reasoning` field in `MessageProps`.
    - Render the reasoning in a collapsible accordion at the top of the assistant's message, labeled "Thinking..." or "Reasoning". This keeps the main chat flow focused on the answer while allowing users to inspect the AI's logic.

### B. Search Result Summarization
Searching for files or information often results in a "wall of text" that clutters the chat.
- **Current State**: Tool results (like from `grep` or `search`) are rendered as raw text/JSON blocks in the `ToolCallCard`.
- **Recommendation**:
    - Implement specialized rendering for tools like `search`, `ls`, and `websearch`.
    - Instead of a code block, display a concise list of results.
    - For file searches, extract paths and render them as clickable links (using the existing `FilePathParser` logic) that open the file in Tandem's file explorer.
    - For web searches, render a list of site titles with favicon-enabled links.

### C. Activity Grouping (Managing "Wall of Tools")
When the AI performs multiple steps (e.g., reading 10 files and then editing 5), the list of tool cards can become overwhelmingly long.
- **Current State**: Each tool call is rendered as an individual card in a vertical list.
- **Recommendation**:
    - Group consecutive tool calls into a single "Activity" accordion.
    - Show a summary line like "Tandem performed 12 operations (read 8 files, edited 4 files)".
    - Allow the user to expand this block to see the individual `ToolCallCard`s.

### D. Deep Nesting Handling
As sessions grow, the amount of nested data (messages -> tool calls -> results -> file changes) can become difficult to navigate.
- **Recommendation**:
    - Implement a "View Details" overlay or a side panel for very large tool outputs.
    - Use breadcrumbs or clear headings to maintain context when the user is deep in a multi-step operation review.

---

## 2. Security & Vulnerability Assessment

While Tandem is built with a "security-first" mindset, several areas were identified that could be hardened.

### A. Path Traversal Vulnerability
The current path validation logic in `src-tauri/src/state.rs` is susceptible to bypasses.
- **Issue**: `is_path_allowed` uses `path.starts_with(allowed_path)` on non-canonicalized paths.
- **Vulnerability**: An attacker (or a compromised AI agent) could provide a path like `/allowed/workspace/../../etc/passwd`. Since it "starts with" the allowed workspace component-wise (before normalization), it might pass the check but allow access to sensitive system files.
- **Fix**: **Always canonicalize** (resolve `..` and symlinks) both the base workspace path and the target path using `std::fs::canonicalize` before performing any prefix checks.

### B. Broad Asset Protocol Scope
In `src-tauri/tauri.conf.json`, the asset protocol scope is configured as:
```json
"assetProtocol": {
  "scope": { "allow": ["**"] }
}
```
- **Issue**: This allows the WebView to load any file on the entire disk using the `asset://` protocol.
- **Vulnerability**: If an XSS vulnerability is found in the frontend (e.g., through rendered markdown or tool results), an attacker could steal any file from the user's machine.
- **Fix**: Restrict the `assetProtocol` scope to only the specific directories the user has granted access to (the active project workspaces).

### C. Manual Denied Pattern Matching
The `denied_patterns` check in `state.rs` uses simple string operations:
```rust
if path_str.ends_with(pattern_suffix) || path_str.contains(&format!("/{}", pattern_suffix))
```
- **Issue**: This is fragile and can be bypassed by variations in path separators (Windows vs Unix), trailing dots, or case-sensitivity issues on some filesystems.
- **Fix**: Use a robust glob matching library or OS-native path comparison tools to enforce these denials.

### D. Unauthenticated Sidecar IPC
- **Issue**: Tandem communicates with the OpenCode sidecar over a local HTTP port without authentication.
- **Vulnerability**: Any other process running on the same machine could discover this port and send commands to the sidecar, potentially bypassing Tandem's UI-based approval system.
- **Fix**: Implement a shared secret (bearer token) generated by Tauri and passed to the sidecar on startup. All subsequent API calls from Tauri to the sidecar should include this token in the headers.

### E. Environment Variable Key Exposure
- **Issue**: API keys are passed to the sidecar as environment variables.
- **Vulnerability**: On some systems, environment variables can be viewed by other users or processes (e.g., via `ps` or `/proc`).
- **Fix**: Pass sensitive configuration via a secure IPC channel (like stdin) or an encrypted temporary file, rather than environment variables.

### F. CSP Hardening
The Content Security Policy (CSP) includes `unsafe-inline`:
```
script-src 'self' 'unsafe-inline';
```
- **Issue**: `unsafe-inline` makes the app significantly more vulnerable to XSS.
- **Fix**: Move all inline scripts to external files and use a nonce-based or hash-based CSP if inline scripts are absolutely necessary.

---

## 3. Recommended Roadmap

1.  **Immediate**: Fix the path traversal vulnerability by implementing strict path canonicalization.
2.  **Short-term**: Update the sidecar and frontend to support collapsible reasoning blocks and activity grouping.
3.  **Short-term**: Restrict the Tauri asset protocol scope to the active workspace.
4.  **Medium-term**: Add authentication to the sidecar IPC and move away from environment variables for API keys.
