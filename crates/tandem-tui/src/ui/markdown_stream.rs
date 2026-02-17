// Adapted from Codex markdown streaming commit strategy
// (codex/codex-rs/tui/src/markdown_stream.rs), simplified for tandem-tui.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkdownStreamCollector {
    pending: String,
}

impl MarkdownStreamCollector {
    pub fn new() -> Self {
        Self::default()
    }

    // Append a raw SSE delta and return only newly completed lines (newline-gated).
    pub fn push_delta_commit_complete(&mut self, delta: &str) -> String {
        if delta.is_empty() {
            return String::new();
        }
        self.pending.push_str(delta);
        let Some(last_newline) = self.pending.rfind('\n') else {
            return String::new();
        };
        let emit = self.pending[..=last_newline].to_string();
        let tail = self.pending[last_newline + 1..].to_string();
        self.pending = tail;
        emit
    }

    // Finalize stream and emit the remaining tail (no trailing newline required).
    pub fn finalize(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }
        let out = self.pending.clone();
        self.pending.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::MarkdownStreamCollector;

    #[test]
    fn commits_only_on_newline() {
        let mut c = MarkdownStreamCollector::new();
        assert_eq!(c.push_delta_commit_complete("hello"), "");
        assert_eq!(c.push_delta_commit_complete(" world\nhow"), "hello world\n");
        assert_eq!(c.push_delta_commit_complete(" are"), "");
        assert_eq!(c.finalize(), "how are");
    }

    #[test]
    fn multiple_completed_lines_in_one_chunk() {
        let mut c = MarkdownStreamCollector::new();
        let out = c.push_delta_commit_complete("a\nb\nc\n");
        assert_eq!(out, "a\nb\nc\n");
        assert_eq!(c.finalize(), "");
    }

    fn stream_roundtrip(chunks: &[&str]) -> String {
        let mut c = MarkdownStreamCollector::new();
        let mut out = String::new();
        for chunk in chunks {
            out.push_str(&c.push_delta_commit_complete(chunk));
        }
        out.push_str(&c.finalize());
        out
    }

    #[test]
    fn roundtrip_markdown_mixed_chunks() {
        let chunks = [
            "## Heading",
            "\n- item ",
            "one\n- item",
            " two\n\n```rust\n",
            "fn main() {}\n",
            "```\nTail",
        ];
        let source = chunks.concat();
        let out = stream_roundtrip(&chunks);
        assert_eq!(out, source);
    }

    #[test]
    fn roundtrip_preserves_whitespace_and_blank_lines() {
        let chunks = ["line1", "\n", "\n", "  indented", "\n", "tail  "];
        let source = chunks.concat();
        let out = stream_roundtrip(&chunks);
        assert_eq!(out, source);
    }

    fn split_at_positions(source: &str, cuts: &[usize]) -> Vec<String> {
        let mut out = Vec::new();
        let mut start = 0usize;
        for &cut in cuts {
            if cut > start && cut <= source.len() {
                out.push(source[start..cut].to_string());
                start = cut;
            }
        }
        if start < source.len() {
            out.push(source[start..].to_string());
        }
        out
    }

    #[test]
    fn roundtrip_complex_markdown_with_varied_chunk_boundaries() {
        let source = "## Plan\n\n1. First\n2. Second\n   - Nested A\n   - Nested B\n\n> Quote line\n> second line\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\nTail\n";
        let boundary_sets: [&[usize]; 6] = [
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[5, 11, 17, 29, 41, 63, 87, 101],
            &[10, 20, 30, 40, 50, 60, 70, 80, 90],
            &[13, 27, 44, 58, 73, 96, 121],
            &[source.len() / 2],
            &[source.len() - 1],
        ];
        for cuts in boundary_sets {
            let chunks = split_at_positions(source, cuts);
            let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
            let out = stream_roundtrip(&refs);
            assert_eq!(out, source, "failed boundary set: {cuts:?}");
        }
    }

    #[test]
    fn roundtrip_utf8_boundaries() {
        let source = "🙂🙂🙂\n汉字漢字\nA\u{0304}B\n";
        let chunks = ["🙂", "🙂🙂\n汉", "字漢", "字\nA", "\u{0304}", "B\n"];
        let out = stream_roundtrip(&chunks);
        assert_eq!(out, source);
    }
}
