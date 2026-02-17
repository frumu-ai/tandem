const DEFAULT_MAX_TITLE_CHARS: usize = 60;

pub fn sanitize_prompt_for_display(input: &str) -> String {
    let mut out = strip_memory_context_blocks(input).trim().to_string();
    if out.is_empty() {
        return out;
    }

    // Prefer explicit user-request marker payload when mode wrappers are present.
    if let Some(after_marker) = extract_after_marker(&out, "[user request]") {
        out = after_marker;
    } else if let Some(after_marker) = extract_after_marker(&out, "user request:") {
        out = after_marker;
    }

    // Fallback for wrappers without [User request] marker.
    loop {
        let lower = out.to_ascii_lowercase();
        if !lower.starts_with("[mode instructions]") {
            break;
        }
        if let Some(split_idx) = out.find("\n\n") {
            out = out[split_idx + 2..].trim().to_string();
        } else if let Some(line_idx) = out.find('\n') {
            out = out[line_idx + 1..].trim().to_string();
        } else {
            out.clear();
            break;
        }
    }

    out.trim().to_string()
}

pub fn derive_session_title_from_prompt(input: &str, max_chars: usize) -> Option<String> {
    let cleaned = sanitize_prompt_for_display(input);
    if cleaned.is_empty() {
        return None;
    }
    let limit = if max_chars == 0 {
        DEFAULT_MAX_TITLE_CHARS
    } else {
        max_chars
    };
    let title = cleaned
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(limit).collect::<String>())
            }
        })
        .unwrap_or_default();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

pub fn title_needs_repair(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower == "new session"
        || lower.starts_with("<memory_context>")
        || lower.starts_with("<current_session>")
        || lower.starts_with("<relevant_history>")
        || lower.starts_with("<project_facts>")
        || lower.starts_with("[mode instructions]")
        || lower == "[user request]"
        || lower == "user request:"
}

fn extract_after_marker(input: &str, marker: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    let marker_lower = marker.to_ascii_lowercase();
    let idx = lower.find(&marker_lower)?;
    let tail_start = idx + marker_lower.len();
    let tail = input.get(tail_start..)?.trim();
    Some(tail.to_string())
}

fn strip_memory_context_blocks(input: &str) -> String {
    let mut out = input.to_string();
    loop {
        let lower = out.to_ascii_lowercase();
        let Some(start) = lower.find("<memory_context>") else {
            break;
        };
        let search_from = start + "<memory_context>".len();
        let Some(rel_end) = lower[search_from..].find("</memory_context>") else {
            break;
        };
        let end = search_from + rel_end + "</memory_context>".len();
        out.replace_range(start..end, "");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_memory_context_and_prefers_user_request() {
        let raw = "<memory_context>\n- fact\n</memory_context>\n\n[Mode instructions]\nfoo\n\n[User request]\nHow do I fix this?";
        assert_eq!(
            sanitize_prompt_for_display(raw),
            "How do I fix this?".to_string()
        );
    }

    #[test]
    fn sanitize_handles_legacy_user_request_marker() {
        let raw = "User request:\nSummarize the repository";
        assert_eq!(
            sanitize_prompt_for_display(raw),
            "Summarize the repository".to_string()
        );
    }

    #[test]
    fn derive_title_uses_first_non_empty_line() {
        let raw = "\n\nFirst line\nSecond line";
        assert_eq!(
            derive_session_title_from_prompt(raw, 60),
            Some("First line".to_string())
        );
    }

    #[test]
    fn title_repair_detects_placeholders_and_wrappers() {
        assert!(title_needs_repair("New session"));
        assert!(title_needs_repair("<memory_context>"));
        assert!(!title_needs_repair("Implement retry logic"));
    }
}
