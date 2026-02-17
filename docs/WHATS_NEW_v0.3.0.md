# What's New in Tandem v0.3.0 (Beta)

This is the first Tandem Engine + TUI release.

This release includes major improvements across reliability, planning, memory, and multi-client behavior.

## Beta Status

- Tandem Engine and TUI are currently in **beta**.
- Both are still **early WIP** and may change quickly across upcoming releases.
- You may encounter rough edges while we continue hardening stability and UX.

## First Release Notes

- This is the first public release cycle for:
  - `tandem-engine`: the native Rust runtime that powers Tandem's sessions, tools, memory, and multi-agent execution.
  - `tandem-tui`: a terminal-first Tandem experience for fast, keyboard-driven workflows.
- Why this is exciting:
  - You now get the same core Tandem intelligence across both Desktop and Terminal.
  - The engine is now a real standalone foundation: easier to test, script, integrate, and evolve.
  - This unlocks a cleaner future for SDK/CLI-style usage, automation, and advanced power-user workflows.
- Desktop now aligns with engine-first architecture and shared runtime behaviors.
- Release packaging now includes platform-specific engine binaries for desktop installers.

## Highlights

- Better Plan Mode outcomes:
  - Fixed `todowrite` empty-loop behavior.
  - Added structured clarification fallback when a concrete task list cannot be generated.
  - Prevented prose-only text from being converted into phantom todo tasks.

- Stronger desktop + session safety:
  - Permission prompts are now scoped to the active session.
  - Question prompts are normalized into the proper question modal flow.

- Memory system upgrade:
  - Desktop now uses the shared `tandem-memory` crate directly.
  - Added strict-scoped `memory_search` behavior (session/project scoped).
  - Added embedding health visibility in retrieval telemetry and settings.
  - Improved startup recovery for malformed memory databases.

- TUI startup and reliability improvements:
  - Engine bootstrap happens before PIN entry for a clearer startup path.
  - Better download/install progress visibility and retry/backoff behavior.
  - Better handling for unreadable/corrupt keystore states.

- Skills and agent capability expansion:
  - Multi-directory skill discovery with deterministic precedence.
  - Per-agent equipped skills support.
  - Mode-level access improvements for the `skill` tool.

- Provider and model updates:
  - Added `copilot` and `cohere` providers.
  - Updated default Gemini model to `gemini-2.5-flash`.

## Quality and Stability

- Improved stream watchdog behavior while idle (fewer false degraded states).
- Fixed Windows memory test CRT mismatch (`LNK2038`) for `tandem-memory`.
- Additional recovery and parsing hardening for provider/tool execution paths.

## UX and Performance

- Smart session titling to better reflect user intent.
- Debounced history refresh behavior for improved frontend responsiveness.

## Next

If you hit anything unexpected after the upgrade, open Settings -> Logs and include diagnostics with your report.
