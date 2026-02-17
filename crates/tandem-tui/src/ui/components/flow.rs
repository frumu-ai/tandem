use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate::app::{ChatMessage, ContentBlock, MessageRole};
use crate::ui::markdown::render_markdown_lines;

#[derive(Default)]
pub struct FlowListState {
    pub offset: usize,
}

pub struct FlowList<'a> {
    messages: &'a [ChatMessage],
    block: Option<Block<'a>>,
}

const FLOW_RENDER_CACHE_MAX_ENTRIES: usize = 1024;

#[derive(Default)]
struct MessageRenderCache {
    map: HashMap<u64, Vec<Line<'static>>>,
    order: VecDeque<u64>,
}

impl MessageRenderCache {
    fn get(&mut self, key: u64) -> Option<Vec<Line<'static>>> {
        self.map.get(&key).cloned()
    }

    fn insert(&mut self, key: u64, value: Vec<Line<'static>>) {
        if !self.map.contains_key(&key) {
            self.order.push_back(key);
        }
        self.map.insert(key, value);
        while self.map.len() > FLOW_RENDER_CACHE_MAX_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

fn flow_render_cache() -> &'static Mutex<MessageRenderCache> {
    static CACHE: OnceLock<Mutex<MessageRenderCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(MessageRenderCache::default()))
}

impl<'a> FlowList<'a> {
    pub fn new(messages: &'a [ChatMessage]) -> Self {
        Self {
            messages,
            block: None,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl<'a> StatefulWidget for FlowList<'a> {
    type State = FlowListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let area = if let Some(block) = self.block {
            let inner_area = block.inner(area);
            block.render(area, buf);
            inner_area
        } else {
            area
        };

        if area.height == 0 || area.width == 0 {
            return;
        }

        let max_width = area.width as usize;
        let visible_height = area.height as usize;
        const OVERSCAN_LINES: usize = 200;
        let (lines, _messages_scanned) = build_virtualized_lines(
            self.messages,
            max_width,
            state.offset,
            visible_height,
            OVERSCAN_LINES,
        );

        // 2. Adjust offset/scrolling.
        // `state.offset` is interpreted as "lines from bottom":
        // 0 = stick to latest content, >0 = user scrolled up into history.
        let total_lines = lines.len();
        let max_from_bottom = total_lines.saturating_sub(visible_height);
        let from_bottom = state.offset.min(max_from_bottom);
        let start = total_lines.saturating_sub(visible_height + from_bottom);

        // 3. Render visible lines
        if total_lines > 0 {
            // Simple render loop
            for (i, line) in lines.iter().skip(start).take(visible_height).enumerate() {
                let x = area.x;
                let y = area.y + i as u16;
                buf.set_line(x, y, line, area.width);
            }
        }
    }
}

fn render_message_lines(msg: &ChatMessage, max_width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let (role_color, role_prefix) = match msg.role {
        MessageRole::User => (Color::Cyan, "you: "),
        MessageRole::Assistant => (Color::Green, "ai:  "),
        MessageRole::System => (Color::Yellow, "sys: "),
    };

    lines.push(Line::from(vec![Span::styled(
        role_prefix,
        Style::default().fg(role_color).add_modifier(Modifier::BOLD),
    )]));

    for block in &msg.content {
        match block {
            ContentBlock::Text(text) => {
                if matches!(msg.role, MessageRole::Assistant) {
                    let mut in_fence = false;
                    for md_line in render_markdown_lines(text) {
                        let style = markdown_line_style(&md_line, in_fence);
                        if md_line.trim_start().starts_with("```") {
                            in_fence = !in_fence;
                        }
                        push_wrapped_line(&mut lines, "     ", &md_line, style, max_width);
                    }
                } else {
                    for line in text.lines() {
                        push_wrapped_line(&mut lines, "     ", line, Style::default(), max_width);
                    }
                }
            }
            ContentBlock::Code { language, code } => {
                push_wrapped_line(
                    &mut lines,
                    "     ",
                    &format!("```{}", language),
                    Style::default().fg(Color::DarkGray),
                    max_width,
                );
                for line in code.lines() {
                    push_wrapped_line(
                        &mut lines,
                        "     ",
                        line,
                        Style::default().fg(Color::Gray),
                        max_width,
                    );
                }
                push_wrapped_line(
                    &mut lines,
                    "     ",
                    "```",
                    Style::default().fg(Color::DarkGray),
                    max_width,
                );
            }
            ContentBlock::ToolCall(info) => {
                push_wrapped_line(
                    &mut lines,
                    "     ",
                    &format!("> Tool Call: {}({})", info.name, info.args),
                    Style::default().fg(Color::Magenta),
                    max_width,
                );
            }
            ContentBlock::ToolResult(output) => {
                push_wrapped_line(
                    &mut lines,
                    "     ",
                    &format!("> Tool Result: {}", output),
                    Style::default().fg(Color::DarkGray),
                    max_width,
                );
            }
        }
    }
    lines.push(Line::from(""));
    lines
}

fn message_cache_key(msg: &ChatMessage, max_width: usize) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    max_width.hash(&mut hasher);
    match msg.role {
        MessageRole::User => 1u8.hash(&mut hasher),
        MessageRole::Assistant => 2u8.hash(&mut hasher),
        MessageRole::System => 3u8.hash(&mut hasher),
    }
    for block in &msg.content {
        match block {
            ContentBlock::Text(t) => {
                1u8.hash(&mut hasher);
                t.hash(&mut hasher);
            }
            ContentBlock::Code { language, code } => {
                2u8.hash(&mut hasher);
                language.hash(&mut hasher);
                code.hash(&mut hasher);
            }
            ContentBlock::ToolCall(info) => {
                3u8.hash(&mut hasher);
                info.id.hash(&mut hasher);
                info.name.hash(&mut hasher);
                info.args.hash(&mut hasher);
            }
            ContentBlock::ToolResult(output) => {
                4u8.hash(&mut hasher);
                output.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn render_message_lines_cached(msg: &ChatMessage, max_width: usize) -> Vec<Line<'static>> {
    let key = message_cache_key(msg, max_width);
    let mut cache = flow_render_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(lines) = cache.get(key) {
        return lines;
    }
    let lines = render_message_lines(msg, max_width);
    cache.insert(key, lines.clone());
    lines
}

fn build_virtualized_lines(
    messages: &[ChatMessage],
    max_width: usize,
    offset: usize,
    visible_height: usize,
    overscan_lines: usize,
) -> (Vec<Line<'static>>, usize) {
    let required_lines = visible_height
        .saturating_add(offset)
        .saturating_add(overscan_lines);

    let mut selected_messages_reversed: Vec<Vec<Line<'static>>> = Vec::new();
    let mut selected_line_count = 0usize;
    let mut scanned_messages = 0usize;
    for msg in messages.iter().rev() {
        let rendered = render_message_lines_cached(msg, max_width);
        selected_line_count = selected_line_count.saturating_add(rendered.len());
        selected_messages_reversed.push(rendered);
        scanned_messages += 1;
        if selected_line_count >= required_lines {
            break;
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(selected_line_count);
    for msg_lines in selected_messages_reversed.into_iter().rev() {
        lines.extend(msg_lines);
    }
    (lines, scanned_messages)
}

fn push_wrapped_line(
    out: &mut Vec<Line>,
    indent: &str,
    text: &str,
    style: Style,
    max_width: usize,
) {
    if max_width == 0 {
        return;
    }
    let indent_chars = indent.chars().count();
    let content_width = max_width.saturating_sub(indent_chars).max(1);

    if text.is_empty() {
        out.push(Line::from(vec![Span::raw(indent.to_string())]));
        return;
    }

    let mut rest = text;
    while !rest.is_empty() {
        let split = wrap_split_index(rest, content_width);
        let (head, tail) = rest.split_at(split);
        out.push(Line::from(vec![
            Span::raw(indent.to_string()),
            Span::styled(head.to_string(), style),
        ]));
        rest = tail.trim_start();
    }
}

fn wrap_split_index(s: &str, max_chars: usize) -> usize {
    let total_chars = s.chars().count();
    if total_chars <= max_chars {
        return s.len();
    }

    let mut count = 0usize;
    let mut split_at = s.len();
    let mut last_ws: Option<usize> = None;

    for (idx, ch) in s.char_indices() {
        if ch.is_whitespace() {
            last_ws = Some(idx);
        }
        count += 1;
        if count >= max_chars {
            split_at = idx + ch.len_utf8();
            break;
        }
    }

    if let Some(ws) = last_ws {
        if ws > 0 {
            return ws;
        }
    }
    split_at
}

fn markdown_line_style(line: &str, in_fence: bool) -> Style {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") {
        return Style::default().fg(Color::DarkGray);
    }
    if in_fence {
        return Style::default().fg(Color::Gray);
    }
    if trimmed.starts_with('#') {
        return Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD);
    }
    if trimmed.starts_with('>') {
        return Style::default().fg(Color::Green);
    }
    let ordered =
        trimmed.chars().take_while(|c| c.is_ascii_digit()).count() > 0 && trimmed.contains(". ");
    if ordered || trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Style::default().fg(Color::LightBlue);
    }
    Style::default()
}

#[cfg(test)]
mod tests {
    use super::{
        build_virtualized_lines, flow_render_cache, markdown_line_style, render_message_lines,
        render_message_lines_cached, FlowList, FlowListState,
    };
    use crate::app::{ChatMessage, ContentBlock, MessageRole};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use ratatui::widgets::StatefulWidget;
    use std::time::{Duration, Instant};

    fn buffer_lines(buf: &Buffer, area: Rect) -> Vec<String> {
        let mut out = Vec::new();
        for y in area.y..area.y + area.height {
            let mut row = String::new();
            for x in area.x..area.x + area.width {
                row.push_str(buf[(x, y)].symbol());
            }
            out.push(row.trim_end().to_string());
        }
        out
    }

    #[test]
    fn assistant_markdown_renders_list_markers() {
        let messages = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text("- one\n- two".to_string())],
        }];
        let area = Rect::new(0, 0, 40, 8);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        assert!(lines.iter().any(|l| l.contains("ai:")));
        assert!(lines.iter().any(|l| l.contains("- one")));
        assert!(lines.iter().any(|l| l.contains("- two")));
    }

    #[test]
    fn user_text_is_not_markdown_transformed() {
        let messages = vec![ChatMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text("**raw** _text_".to_string())],
        }];
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        assert!(lines.iter().any(|l| l.contains("you:")));
        assert!(lines.iter().any(|l| l.contains("**raw** _text_")));
    }

    #[test]
    fn snapshot_assistant_markdown_heading_and_code() {
        let messages = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(
                "## H\n```rust\nfn main() {}\n```".to_string(),
            )],
        }];
        let area = Rect::new(0, 0, 50, 10);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        let joined = lines.join("\n");
        assert!(joined.contains("ai:"));
        assert!(joined.contains("## H"));
        assert!(joined.contains("```rust"));
        assert!(joined.contains("fn main() {}"));
        assert!(joined.contains("```"));
    }

    #[test]
    fn wraps_long_assistant_line_in_narrow_view() {
        let messages = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(
                "This is a very long assistant line that should wrap.".to_string(),
            )],
        }];
        let area = Rect::new(0, 0, 24, 10);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        let non_empty: Vec<_> = lines.iter().filter(|l| !l.is_empty()).collect();
        assert!(non_empty.len() >= 3);
    }

    #[test]
    fn markdown_style_rules_apply_expected_colors() {
        assert_eq!(
            markdown_line_style("## heading", false).fg,
            Some(Color::LightYellow)
        );
        assert_eq!(markdown_line_style("> quote", false).fg, Some(Color::Green));
        assert_eq!(
            markdown_line_style("- item", false).fg,
            Some(Color::LightBlue)
        );
        assert_eq!(
            markdown_line_style("1. item", false).fg,
            Some(Color::LightBlue)
        );
        assert_eq!(
            markdown_line_style("```rs", false).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(markdown_line_style("code()", true).fg, Some(Color::Gray));
    }

    #[test]
    fn snapshot_complex_markdown_layout_contains_expected_structure() {
        let markdown = "## H1\n\n1. First item\n2. Second item\n   - Nested bullet\n\n> Quote line\n\n```rust\nfn main() {\n    println!(\"ok\");\n}\n```\n";
        let messages = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(markdown.to_string())],
        }];
        let area = Rect::new(0, 0, 64, 16);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        let joined = lines.join("\n");
        assert!(joined.contains("ai:"));
        assert!(joined.contains("## H1"));
        assert!(joined.contains("1. First item"));
        assert!(joined.contains("2. Second item"));
        assert!(joined.contains("- Nested bullet"));
        assert!(joined.contains("> Quote line"));
        assert!(joined.contains("```rust"));
        assert!(joined.contains("fn main() {"));
        assert!(joined.contains("println!(\"ok\");"));
        assert!(joined.contains("```"));
    }

    #[test]
    fn snapshot_narrow_complex_markdown_wraps_and_keeps_markers() {
        let markdown = "1. A long ordered item that should wrap cleanly\n> A quoted line that also wraps\n- Bullet item that wraps too";
        let messages = vec![ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(markdown.to_string())],
        }];
        let area = Rect::new(0, 0, 28, 14);
        let mut buf = Buffer::empty(area);
        let mut state = FlowListState::default();
        FlowList::new(&messages).render(area, &mut buf, &mut state);
        let lines = buffer_lines(&buf, area);
        let non_empty: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
        assert!(non_empty.len() >= 5);
        let joined = lines.join("\n");
        assert!(joined.contains("1. A long"));
        assert!(joined.contains("> A quoted"));
        assert!(joined.contains("- Bullet"));
    }

    fn generate_long_fixture(message_count: usize) -> Vec<ChatMessage> {
        let mut out = Vec::with_capacity(message_count);
        for i in 0..message_count {
            let role = match i % 3 {
                0 => MessageRole::User,
                1 => MessageRole::Assistant,
                _ => MessageRole::System,
            };
            let text = if matches!(role, MessageRole::Assistant) {
                format!(
                    "## Step {}\n\n1. Do thing {}\n2. Verify output\n\n```rust\nprintln!(\"{}\");\n```\n> note {}\n- item a\n- item b",
                    i, i, i, i
                )
            } else {
                format!(
                    "message {} lorem ipsum dolor sit amet, consectetur adipiscing elit",
                    i
                )
            };
            out.push(ChatMessage {
                role,
                content: vec![ContentBlock::Text(text)],
            });
        }
        out
    }

    fn render_naive_visible(messages: &[ChatMessage], area: Rect, offset: usize) -> Vec<String> {
        let max_width = area.width as usize;
        let mut lines = Vec::new();
        for msg in messages {
            lines.extend(render_message_lines(msg, max_width));
        }
        let visible_height = area.height as usize;
        let total_lines = lines.len();
        let max_from_bottom = total_lines.saturating_sub(visible_height);
        let from_bottom = offset.min(max_from_bottom);
        let start = total_lines.saturating_sub(visible_height + from_bottom);
        lines
            .iter()
            .skip(start)
            .take(visible_height)
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect()
    }

    #[test]
    fn virtualization_scans_subset_for_recent_view() {
        let messages = generate_long_fixture(3_000);
        let area = Rect::new(0, 0, 100, 30);
        let (_, scanned) =
            build_virtualized_lines(&messages, area.width as usize, 0, area.height as usize, 200);
        assert!(scanned < messages.len());
    }

    #[test]
    fn virtualized_and_naive_visible_output_match() {
        let messages = generate_long_fixture(400);
        let area = Rect::new(0, 0, 90, 24);
        let offsets = [0usize, 8, 25, 64];
        for offset in offsets {
            let mut buf = Buffer::empty(area);
            let mut state = FlowListState { offset };
            FlowList::new(&messages).render(area, &mut buf, &mut state);
            let virt = buffer_lines(&buf, area);
            let naive = render_naive_visible(&messages, area, offset)
                .into_iter()
                .map(|s| s.trim_end().to_string())
                .collect::<Vec<_>>();
            assert_eq!(virt, naive, "mismatch at offset {}", offset);
        }
    }

    #[test]
    #[ignore]
    fn benchmark_virtualized_vs_naive_long_transcript() {
        let messages = generate_long_fixture(5_000);
        let area = Rect::new(0, 0, 100, 30);
        let offsets = [0usize, 20, 100];

        let mut virt_total = Duration::ZERO;
        let mut naive_total = Duration::ZERO;
        let runs = 12usize;

        for run in 0..runs {
            let offset = offsets[run % offsets.len()];

            let mut buf = Buffer::empty(area);
            let mut state = FlowListState { offset };
            let start = Instant::now();
            FlowList::new(&messages).render(area, &mut buf, &mut state);
            virt_total += start.elapsed();

            let start = Instant::now();
            let _ = render_naive_visible(&messages, area, offset);
            naive_total += start.elapsed();
        }

        eprintln!(
            "flow benchmark runs={} virtualized_ms={} naive_ms={}",
            runs,
            virt_total.as_millis(),
            naive_total.as_millis()
        );
    }

    #[test]
    fn cached_render_matches_uncached() {
        let msg = ChatMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text(
                "## Title\n- a\n- b\n```rust\nfn x() {}\n```".to_string(),
            )],
        };
        let uncached = render_message_lines(&msg, 80);
        let cached = render_message_lines_cached(&msg, 80);
        let uncached_text = uncached
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>();
        let cached_text = cached
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>();
        assert_eq!(uncached_text, cached_text);
    }

    #[test]
    fn cache_is_bounded() {
        for i in 0..(super::FLOW_RENDER_CACHE_MAX_ENTRIES + 128) {
            let msg = ChatMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text(format!("msg {}", i))],
            };
            let _ = render_message_lines_cached(&msg, 80);
        }

        let cache = flow_render_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(cache.map.len() <= super::FLOW_RENDER_CACHE_MAX_ENTRIES);
    }
}
