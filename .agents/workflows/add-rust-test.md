---
description: how to add Rust tests without bloating main source files
---

> [!IMPORTANT]
> Do not keep growing large Rust implementation files by appending more tests to
> their inline `#[cfg(test)] mod tests`.
> Prefer dedicated test files or test submodules so production code stays
> readable.

## Rules

1. Before adding a Rust test, check whether the crate or feature already has a
   dedicated test location:
   - a sibling `tests.rs`
   - a sibling `tests/` directory with domain-specific files
   - a crate-level `tests/` integration test directory

2. If one of those exists, add the new test there instead of the main source
   file.

3. If the existing source file already has a very large inline test module, do
   not extend it further unless there is a strong reason the test must stay
   inline for access to private helpers.

4. When creating or extending extracted Rust tests, group them by behavior or
   feature area instead of dumping unrelated cases into one giant file.

5. Keep test helpers shared:
   - move reusable setup into `tests/support.rs`, `tests/mod.rs`, or a local
     helper module
   - avoid copy-pasting large fixtures into multiple test files

6. For HTTP handler tests in `tandem-server`, follow
   `.agents/workflows/add-http-test.md` and add tests under
   `crates/tandem-server/src/http/tests/<domain>.rs`.

## Preferred layout

Use one of these patterns when possible:

```rust
// src/foo.rs
pub fn my_logic() { /* ... */ }
```

```rust
// src/foo/tests.rs
use super::*;

#[test]
fn handles_basic_case() {
    // ...
}
```

```rust
// src/foo/mod.rs
mod parser;
#[cfg(test)]
mod tests;
```

```rust
// tests/foo_behavior.rs
use my_crate::my_logic;

#[test]
fn handles_basic_case() {
    // ...
}
```

## Decision rule

If choosing between:

- adding one more test to an already-long source file, or
- creating/extending a focused test file,

pick the focused test file.
