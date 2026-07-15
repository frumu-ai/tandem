// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! TAN-684: guard for the nextest quarantine policy.
//!
//! TAN-230/TAN-684 drained the CI exclusion list; this test keeps it drained.
//! Any future `test(=...)` exclusion in `.config/nextest.toml` must carry, on
//! the same or the immediately preceding line, a comment referencing an open
//! Linear issue plus an owner and an expiry date:
//!
//! ```toml
//! # TAN-999 owner=evan expires=2026-08-01
//! | test(=path::to::flaky_test)
//! ```

use std::path::Path;

#[test]
fn nextest_quarantine_entries_reference_issue_owner_and_expiry() {
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".config/nextest.toml");
    let config = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", config_path.display()));

    let lines: Vec<&str> = config.lines().collect();
    let mut violations = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        // Only quarantine entries count: a `test(=...)` filter expression on a
        // non-comment line.
        let code = line.split('#').next().unwrap_or_default();
        if !code.contains("test(=") {
            continue;
        }
        let same_line_comment = line.split_once('#').map(|(_, c)| c).unwrap_or_default();
        let previous_line = index
            .checked_sub(1)
            .and_then(|prev| lines.get(prev))
            .copied()
            .unwrap_or_default();
        let previous_comment = previous_line
            .trim_start()
            .strip_prefix('#')
            .unwrap_or_default();
        let documented = [same_line_comment, previous_comment].iter().any(|comment| {
            comment.contains("TAN-") && comment.contains("owner=") && comment.contains("expires=")
        });
        if !documented {
            violations.push(format!("line {}: {}", index + 1, line.trim()));
        }
    }

    assert!(
        violations.is_empty(),
        "every nextest quarantine entry needs `# TAN-<issue> owner=<who> expires=<date>` on the \
         same or preceding line (TAN-684 quarantine policy):\n{}",
        violations.join("\n")
    );
}
