// Adapted from Codex markdown rendering approach
// (codex/codex-rs/tui/src/markdown_render.rs), rewritten for tandem-tui.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub fn render_markdown_lines(input: &str) -> Vec<String> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, options);
    let mut writer = MarkdownWriter::default();
    for event in parser {
        writer.handle(event);
    }
    writer.finish()
}

#[derive(Default)]
struct MarkdownWriter {
    lines: Vec<String>,
    current: String,
    list_stack: Vec<ListState>,
    in_code_block: bool,
    code_fence_lang: Option<String>,
    pending_link: Option<String>,
    blockquote_depth: usize,
}

#[derive(Clone, Copy)]
enum ListState {
    Bullet,
    Ordered(u64),
}

impl MarkdownWriter {
    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_text(&format!("`{code}`")),
            Event::SoftBreak | Event::HardBreak => self.newline(),
            Event::Rule => {
                self.newline();
                self.push_text("---");
                self.newline();
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if !self.current.is_empty() {
                    self.newline();
                }
            }
            Tag::Heading { level, .. } => {
                if !self.current.is_empty() {
                    self.newline();
                }
                self.prefix();
                self.current.push_str(&"#".repeat(level as usize));
                self.current.push(' ');
            }
            Tag::BlockQuote => {
                self.blockquote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                self.newline();
                match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.trim();
                        if lang.is_empty() {
                            self.push_text("```");
                        } else {
                            self.push_text(&format!("```{lang}"));
                        }
                        self.code_fence_lang = Some(lang.to_string());
                    }
                    CodeBlockKind::Indented => {
                        self.push_text("```");
                        self.code_fence_lang = Some(String::new());
                    }
                }
                self.newline();
            }
            Tag::List(start) => match start {
                Some(v) => self.list_stack.push(ListState::Ordered(v)),
                None => self.list_stack.push(ListState::Bullet),
            },
            Tag::Item => {
                if !self.current.is_empty() {
                    self.newline();
                }
                self.prefix();
                if let Some(last) = self.list_stack.last_mut() {
                    match last {
                        ListState::Bullet => self.current.push_str("- "),
                        ListState::Ordered(n) => {
                            self.current.push_str(&format!("{}. ", *n));
                            *n += 1;
                        }
                    }
                }
            }
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {}
            Tag::Link { dest_url, .. } => {
                self.pending_link = Some(dest_url.to_string());
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => self.newline(),
            TagEnd::BlockQuote => {
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.newline();
            }
            TagEnd::CodeBlock => {
                self.newline();
                self.push_text("```");
                self.in_code_block = false;
                self.code_fence_lang = None;
                self.newline();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.newline();
            }
            TagEnd::Link => {
                if let Some(link) = self.pending_link.take() {
                    self.current.push_str(" (");
                    self.current.push_str(&link);
                    self.current.push(')');
                }
            }
            _ => {}
        }
    }

    fn prefix(&mut self) {
        if self.blockquote_depth > 0 {
            self.current
                .push_str(&format!("{} ", ">".repeat(self.blockquote_depth)));
        }
        if self.list_stack.len() > 1 {
            self.current
                .push_str(&"  ".repeat(self.list_stack.len().saturating_sub(1)));
        }
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.current.is_empty() && !self.in_code_block {
            self.prefix();
        }
        self.current.push_str(text);
    }

    fn newline(&mut self) {
        if !self.current.is_empty() {
            self.lines.push(std::mem::take(&mut self.current));
        } else if self.lines.last().map(|s| !s.is_empty()).unwrap_or(true) {
            self.lines.push(String::new());
        }
    }

    fn finish(mut self) -> Vec<String> {
        if !self.current.is_empty() {
            self.lines.push(self.current);
        }
        while self.lines.last().is_some_and(|s| s.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::render_markdown_lines;

    #[test]
    fn renders_nested_list_and_blockquote() {
        let out = render_markdown_lines("- one\n  - two\n> quote");
        assert!(out.iter().any(|l| l.contains("- one")));
        assert!(out.iter().any(|l| l.contains("- two")));
        assert!(out.iter().any(|l| l.contains("> quote")));
    }

    #[test]
    fn renders_ordered_list_numbering() {
        let out = render_markdown_lines("1. first\n2. second");
        assert!(out.iter().any(|l| l.contains("1. first")));
        assert!(out.iter().any(|l| l.contains("2. second")));
    }

    #[test]
    fn renders_link_with_destination() {
        let out = render_markdown_lines("read [docs](https://example.com)");
        let joined = out.join("\n");
        assert!(joined.contains("docs (https://example.com)"));
    }

    #[test]
    fn renders_fenced_code_blocks() {
        let out = render_markdown_lines("```rust\nfn main() {}\n```");
        let joined = out.join("\n");
        assert!(joined.contains("```rust"));
        assert!(joined.contains("fn main() {}"));
        assert!(joined.contains("```"));
    }

    #[test]
    fn renders_nested_ordered_and_bullet_lists() {
        let out = render_markdown_lines("1. one\n   - two\n   - three\n2. four");
        let joined = out.join("\n");
        assert!(joined.contains("1. one"));
        assert!(joined.contains("- two"));
        assert!(joined.contains("- three"));
        assert!(joined.contains("2. four"));
    }

    #[test]
    fn renders_blockquote_with_list() {
        let out = render_markdown_lines("> note\n> - a\n> - b");
        let joined = out.join("\n");
        assert!(joined.contains("> note"));
        assert!(joined.contains("> - a") || joined.contains("- a"));
        assert!(joined.contains("> - b") || joined.contains("- b"));
    }
}
