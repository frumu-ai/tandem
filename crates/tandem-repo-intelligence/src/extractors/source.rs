use crate::model::{Confidence, ExtractedFacts, ExtractedSymbol, ImportEdge, SymbolKind};

pub fn extract_source_facts(path: &str, body: &str) -> ExtractedFacts {
    let mut facts = ExtractedFacts::default();
    for (index, line) in body.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        if is_rust(path) {
            extract_rust_line(path, line_number, trimmed, &mut facts);
        } else if is_typescript(path) {
            extract_typescript_line(path, line_number, trimmed, &mut facts);
        } else if is_python(path) {
            extract_python_line(path, line_number, trimmed, &mut facts);
        }
    }
    facts
}

fn extract_rust_line(path: &str, line: usize, text: &str, facts: &mut ExtractedFacts) {
    if let Some(target) = text
        .strip_prefix("use ")
        .and_then(|value| value.strip_suffix(';'))
    {
        facts.imports.push(import(path, line, target.trim()));
    }
    if let Some(name) = rust_named_after(text, "fn ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Function));
    } else if let Some(name) = rust_named_after(text, "struct ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Struct));
    } else if let Some(name) = rust_named_after(text, "enum ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Enum));
    } else if let Some(name) = rust_named_after(text, "trait ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Trait));
    } else if let Some(name) = rust_named_after(text, "mod ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Module));
    } else if let Some(name) = rust_named_after(text, "impl ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Impl));
    }
}

fn extract_typescript_line(path: &str, line: usize, text: &str, facts: &mut ExtractedFacts) {
    if text.starts_with("import ") {
        if let Some(target) = quoted_module(text) {
            facts.imports.push(import(path, line, target));
        }
    }
    if let Some(name) = ts_named_after(text, "function ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Function));
    } else if let Some(name) = ts_named_after(text, "class ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Class));
    } else if let Some(name) = ts_named_after(text, "interface ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Interface));
    } else if let Some(name) = ts_named_after(text, "type ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::TypeAlias));
    } else if let Some(name) = ts_named_after(text, "const ") {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Constant));
    }
}

fn extract_python_line(path: &str, line: usize, text: &str, facts: &mut ExtractedFacts) {
    if let Some(target) = text.strip_prefix("import ") {
        facts.imports.push(import(path, line, target.trim()));
    } else if let Some(target) = text
        .strip_prefix("from ")
        .and_then(|value| value.split_once(" import ").map(|parts| parts.0))
    {
        facts.imports.push(import(path, line, target.trim()));
    }

    if let Some(name) = text
        .strip_prefix("def ")
        .and_then(name_before_python_suffix)
    {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Function));
    } else if let Some(name) = text
        .strip_prefix("async def ")
        .and_then(name_before_python_suffix)
    {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Function));
    } else if let Some(name) = text
        .strip_prefix("class ")
        .and_then(name_before_python_suffix)
    {
        facts
            .symbols
            .push(symbol(path, line, name, SymbolKind::Class));
    }
}

fn rust_named_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let after = text.strip_prefix(marker).or_else(|| {
        text.strip_prefix("pub ")
            .and_then(|value| value.strip_prefix(marker))
    })?;
    name_prefix(after)
}

fn ts_named_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let text = text.strip_prefix("export default ").unwrap_or(text);
    let text = text.strip_prefix("export ").unwrap_or(text);
    text.strip_prefix(marker).and_then(name_prefix)
}

fn name_before_python_suffix(value: &str) -> Option<&str> {
    value
        .split(['(', ':'])
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn name_prefix(value: &str) -> Option<&str> {
    value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn quoted_module(text: &str) -> Option<&str> {
    let quote = if text.contains('\'') { '\'' } else { '"' };
    let (_, rest) = text.split_once(quote)?;
    let (module, _) = rest.split_once(quote)?;
    Some(module)
}

fn symbol(path: &str, line: usize, name: &str, kind: SymbolKind) -> ExtractedSymbol {
    ExtractedSymbol {
        file_path: path.to_string(),
        line,
        name: name.to_string(),
        kind,
        confidence: Confidence::Extracted,
    }
}

fn import(path: &str, line: usize, target: &str) -> ImportEdge {
    ImportEdge {
        source_file: path.to_string(),
        line,
        target: target.to_string(),
        confidence: Confidence::Extracted,
    }
}

fn is_rust(path: &str) -> bool {
    path.ends_with(".rs")
}

fn is_typescript(path: &str) -> bool {
    path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".js")
        || path.ends_with(".jsx")
}

fn is_python(path: &str) -> bool {
    path.ends_with(".py")
}
