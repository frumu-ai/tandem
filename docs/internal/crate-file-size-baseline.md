# Crate File Size Baseline

Last updated: 2026-04-17

Scope: Rust source files under `crates/` only.

This snapshot captures all `.rs` files with `>= 1500` lines and is used as the baseline
for the Rust file-size refactor in `CRATE_FILE_SIZE_REFACTOR_KANBAN.md`.

How the snapshot was generated:

```bash
find crates -name '*.rs' -type f | sort | xargs wc -l | sort -nr | awk '$1 >= 1500'
```

- Total `.rs` files in `crates/`: 500
- Files at or above `1500` lines: 64

## Priority 1 (>= 2,000 lines)

| lines | file |
| --- | --- |

## Priority 2 (1,500–1,999 lines)

| lines | file |
| --- | --- |
| 1998 | `crates/tandem-core/src/engine_loop/prompt_execution.rs` |
| 1950 | `crates/tandem-server/src/pack_builder_parts/part01.rs` |
| 1947 | `crates/tandem-server/src/http/config_providers_parts/part01.rs` |
| 1945 | `crates/tandem-server/src/http/tests/workflow_planner_parts/part01.rs` |
| 1944 | `crates/tandem-server/src/http/tests/coder_parts/part08.rs` |
| 1944 | `crates/tandem-server/src/http/context_runs_parts/part02.rs` |
| 1942 | `crates/tandem-server/src/app/state/automation/logic/part02.rs` |
| 1937 | `crates/tandem-server/src/agent_teams_parts/part01.rs` |
| 1935 | `crates/tandem-server/src/app/state/automation/logic_parts/part02.rs` |
| 1932 | `crates/tandem-server/src/http/tests/global_parts/part03.rs` |
| 1932 | `crates/tandem-server/src/http/tests/coder_parts/part02.rs` |
| 1931 | `crates/tandem-server/src/app/state/automation/logic_parts/part04.rs` |
| 1930 | `crates/tandem-server/src/http/skills_memory_parts/part01.rs` |
| 1928 | `crates/tandem-server/src/http/routines_automations_parts/part01.rs` |
| 1927 | `crates/tandem-server/src/http/tests/global_parts/part01.rs` |
| 1924 | `crates/tandem-server/src/http/tests/coder_parts/part01.rs` |
| 1922 | `crates/tandem-server/src/app/state/tests/automations/validation_parts/part02.rs` |
| 1921 | `crates/tandem-server/src/http/tests/coder_parts/part05.rs` |
| 1921 | `crates/tandem-plan-compiler/src/workflow_plan_parts/part01.rs` |
| 1920 | `crates/tandem-server/src/http/coder_parts/part04.rs` |
| 1916 | `crates/tandem-server/src/http/bug_monitor_parts/part01.rs` |
| 1911 | `crates/tandem-server/src/http/tests/global_parts/part02.rs` |
| 1910 | `crates/tandem-server/src/http/context_runs_parts/part01.rs` |
| 1909 | `crates/tandem-server/src/automation_v2/executor.rs` |
| 1902 | `crates/tandem-server/src/http/tests/coder_parts/part03.rs` |
| 1901 | `crates/tandem-server/src/app/state/app_state_impl_parts/part02.rs` |
| 1899 | `crates/tandem-server/src/app/state/app_state_impl_parts/part03.rs` |
| 1893 | `crates/tandem-server/src/http/routines_automations_parts/part02.rs` |
| 1893 | `crates/tandem-server/src/http/coder_parts/part05.rs` |
| 1891 | `crates/tandem-server/src/http/coder_parts/part06.rs` |
| 1891 | `crates/tandem-memory/src/memory_database_impl_parts/part01.rs` |
| 1883 | `crates/tandem-server/src/http/tests/coder_parts/part04.rs` |
| 1881 | `crates/tandem-server/src/app/state/tests/automations_parts/part02.rs` |
| 1876 | `crates/tandem-server/src/http/tests/context_runs_parts/part01.rs` |
| 1875 | `crates/tandem-tui/src/app_update_match_arms_parts/part01.rs` |
| 1869 | `crates/tandem-server/src/app/state/tests/automations_parts/part01.rs` |
| 1860 | `crates/tandem-server/src/app/state/tests/automations/validation_parts/part01.rs` |
| 1854 | `crates/tandem-server/src/http/skills_memory_parts/part02.rs` |
| 1851 | `crates/tandem-server/src/http/coder_parts/part03.rs` |
| 1844 | `crates/tandem-server/src/http/tests/optimizations_parts/part01.rs` |
| 1843 | `crates/tandem-server/src/http/tests/coder_parts/part07.rs` |
| 1835 | `crates/tandem-server/src/http/coder_parts/part07.rs` |
| 1808 | `crates/tandem-server/src/http/coder_parts/part01.rs` |
| 1791 | `crates/tandem-server/src/http/tests/coder_parts/part06.rs` |
| 1790 | `crates/tandem-server/src/app/state/app_state_impl_parts/part01.rs` |
| 1780 | `crates/tandem-server/src/http/tests/bug_monitor_parts/part01.rs` |
| 1776 | `crates/tandem-server/src/app/state/automation/workflow_impl.rs` |
| 1751 | `crates/tandem-tui/src/ui/mod.rs` |
| 1750 | `crates/tandem-server/src/optimization.rs` |
| 1742 | `crates/tandem-server/src/app/state/automation/logic_parts/part05.rs` |
| 1738 | `crates/tandem-server/src/app/state/tests/automations_parts/part03.rs` |
| 1736 | `crates/tandem-skills/src/lib.rs` |
| 1722 | `crates/tandem-tui/src/app_impl_parts/part01.rs` |
| 1711 | `crates/tandem-core/src/engine_loop/tool_parsing/normalize.rs` |
| 1700 | `crates/tandem-server/src/http/tests/routines.rs` |
| 1688 | `crates/tandem-server/src/app/state/mod.rs` |
| 1651 | `crates/tandem-server/src/app/state/automation/logic_parts/part01.rs` |
| 1596 | `crates/tandem-server/src/http/coder_parts/part02.rs` |
| 1594 | `crates/tandem-server/src/http/sessions.rs` |
| 1563 | `crates/tandem-browser/src/lib.rs` |
| 1517 | `crates/tandem-server/src/app/state/automation/logic/part03_parts/part02.rs` |
| 1502 | `crates/tandem-tui/src/app.rs` |
| 1502 | `crates/tandem-server/src/http/workflow_planner_host.rs` |
| 1502 | `crates/tandem-server/src/http/missions_teams.rs` |

## Commands

```bash
scripts/check-file-sizes.sh
```

Scoped refresh command (crates-only):

```bash
find crates -name '*.rs' -type f | sort | xargs wc -l | sort -nr | awk '$1 >= 1500' | sort -nr
```
