use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};
use tui_markdown::from_str as markdown_to_text;

use crate::app::{ChatMessage, ContentBlock, MessageRole};

#[derive(Default)]
pub struct FlowListState {
    pub offset: usize,
}

pub struct FlowList<'a> {
    messages: &'a [ChatMessage],
    block: Option<Block<'a>>,
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

        // Flatten messages into wrapped render lines for narrow terminals.
        let mut lines: Vec<Line> = Vec::new();
        let max_width = area.width as usize;

        for msg in self.messages {
            let (role_color, role_prefix) = match msg.role {
                MessageRole::User => (Color::Cyan, "you: "),
                MessageRole::Assistant => (Color::Green, "ai:  "),
                MessageRole::System => (Color::Yellow, "sys: "),
            };

            // Header line
            lines.push(Line::from(vec![Span::styled(
                role_prefix,
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            )]));

            for block in &msg.content {
                match block {
                    ContentBlock::Text(text) => {
                        if matches!(msg.role, MessageRole::Assistant) {
                            let rendered = markdown_to_text(text);
                            for md_line in rendered.lines {
                                push_wrapped_line(
                                    &mut lines,
                                    "     ",
                                    &md_line.to_string(),
                                    Style::default(),
                                    max_width,
                                );
                            }
                        } else {
                            for line in text.lines() {
                                push_wrapped_line(
                                    &mut lines,
                                    "     ",
                                    line,
                                    Style::default(),
                                    max_width,
                                );
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
            lines.push(Line::from("")); // Spacing
        }

        // 2. Adjust offset/scrolling.
        // `state.offset` is interpreted as "lines from bottom":
        // 0 = stick to latest content, >0 = user scrolled up into history.
        let visible_height = area.height as usize;
        let total_lines = lines.len();
        let max_from_bottom = total_lines.saturating_sub(visible_height);
        let from_bottom = state.offset.min(max_from_bottom);
        state.offset = from_bottom;
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
