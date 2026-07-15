// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(crate) fn markdown_section_lists_entries(
    text: &str,
    heading: &str,
    entry_matches: impl Fn(&str) -> bool,
) -> bool {
    let lowered = text.to_ascii_lowercase();
    let Some(start) = lowered.find(&heading.to_ascii_lowercase()) else {
        return false;
    };
    let tail = &text[start..];
    tail.lines().skip(1).take(24).any(|line| {
        let trimmed = line.trim();
        let bullet_like = (trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            && entry_matches(trimmed);
        let table_like = trimmed.starts_with('|')
            && !trimmed
                .chars()
                .all(|ch| matches!(ch, '|' | '-' | ':' | ' ' | '\t'))
            && entry_matches(trimmed);
        bullet_like || table_like
    })
}

pub(crate) fn concrete_workspace_path_like(value: &str) -> bool {
    let trimmed = value.trim().trim_matches('`');
    !trimmed.is_empty()
        && !trimmed.contains('*')
        && !trimmed.contains('?')
        && !trimmed.ends_with('/')
}

pub(crate) fn files_reviewed_section_lists_paths(text: &str) -> bool {
    markdown_section_lists_entries(text, "files reviewed", |trimmed| {
        concrete_workspace_path_like(trimmed)
            && (trimmed.contains('/')
                || trimmed.contains(".md")
                || trimmed.contains(".txt")
                || trimmed.contains(".yaml")
                || trimmed.contains("readme"))
    })
}

pub(crate) fn markdown_citation_count(text: &str) -> usize {
    let markdown_links = text.match_indices("](").count();
    let bare_urls = text
        .split_whitespace()
        .filter(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(ch, ')' | '(' | '[' | ']' | ',' | '.' | ';' | '"' | '\'')
            });
            trimmed.starts_with("http://") || trimmed.starts_with("https://")
        })
        .count();
    markdown_links.max(bare_urls)
}

pub(crate) fn web_sources_reviewed_section_lists_sources(text: &str) -> bool {
    markdown_section_lists_entries(text, "web sources reviewed", |trimmed| {
        trimmed.contains("http://") || trimmed.contains("https://") || trimmed.contains("](")
    })
}

pub(crate) fn extract_markdown_section_paths(text: &str, heading: &str) -> Vec<String> {
    let mut collecting = false;
    let mut paths = Vec::new();
    let heading_normalized = heading.trim().to_ascii_lowercase();
    for line in text.lines() {
        let trimmed = line.trim();
        let normalized = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
        if trimmed.starts_with('#') {
            collecting = normalized == heading_normalized;
            continue;
        }
        if !collecting {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let candidate = trimmed
            .trim_start_matches(|ch: char| {
                ch == '-' || ch == '*' || ch.is_ascii_digit() || ch == '.' || ch == ')'
            })
            .trim();
        let token = candidate.split(['`', '(', ')']).find_map(|part| {
            let value = part.trim();
            if value.contains('/')
                || value.ends_with(".md")
                || value.ends_with(".txt")
                || value.ends_with(".yaml")
                || value.to_ascii_lowercase().contains("readme")
            {
                concrete_workspace_path_like(value).then(|| value.to_string())
            } else {
                None
            }
        });
        if let Some(path) = token.filter(|value| !value.is_empty()) {
            paths.push(path);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn contains_bare_tandem_artifact_href(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("href=\".tandem/artifacts/")
        || lowered.contains("href='.tandem/artifacts/")
        || lowered.contains("href=.tandem/artifacts/")
}
