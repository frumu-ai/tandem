use crate::model::{Confidence, ConfigReference, DocHeading, ExtractedFacts};

pub fn extract_config_doc_facts(path: &str, body: &str) -> ExtractedFacts {
    let mut facts = ExtractedFacts::default();
    if is_markdown(path) {
        extract_markdown(path, body, &mut facts);
    }
    if is_config(path) {
        extract_config(path, body, &mut facts);
    }
    facts
}

fn extract_markdown(path: &str, body: &str, facts: &mut ExtractedFacts) {
    let lines: Vec<_> = body.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        if !(1..=6).contains(&level) || !trimmed[level..].starts_with(' ') {
            continue;
        }
        facts.doc_headings.push(DocHeading {
            file_path: path.to_string(),
            line: index + 1,
            level,
            title: trimmed[level..].trim().to_string(),
            excerpt: next_excerpt(&lines, index + 1),
            confidence: Confidence::Extracted,
        });
    }
}

fn extract_config(path: &str, body: &str, facts: &mut ExtractedFacts) {
    let mut section = String::new();
    for (index, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed.trim_matches(['[', ']']).to_string();
            facts
                .config_references
                .push(config(path, index + 1, "section", &section));
            continue;
        }
        if let Some((key, value)) = split_config_pair(trimmed) {
            let key = if section.is_empty() {
                key.to_string()
            } else {
                format!("{section}.{key}")
            };
            facts
                .config_references
                .push(config(path, index + 1, &key, value));
        }
    }
}

fn split_config_pair(line: &str) -> Option<(&str, &str)> {
    line.split_once('=')
        .or_else(|| line.split_once(':'))
        .map(|(key, value)| {
            (
                key.trim().trim_matches('"'),
                value.trim().trim_matches([',', '"']),
            )
        })
        .filter(|(key, value)| !key.is_empty() && !value.is_empty())
}

fn config(path: &str, line: usize, key: &str, value: &str) -> ConfigReference {
    ConfigReference {
        file_path: path.to_string(),
        line,
        key: key.to_string(),
        value: value.to_string(),
        confidence: Confidence::Extracted,
    }
}

fn next_excerpt(lines: &[&str], start: usize) -> String {
    lines
        .iter()
        .skip(start)
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}

fn is_markdown(path: &str) -> bool {
    path.ends_with(".md") || path.ends_with(".mdx")
}

fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(name, "Cargo.toml" | "package.json" | "pyproject.toml")
        || path.ends_with(".toml")
        || path.ends_with(".yaml")
        || path.ends_with(".yml")
        || path.ends_with(".json")
}
