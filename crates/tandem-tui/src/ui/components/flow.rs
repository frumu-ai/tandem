use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, StatefulWidget, Widget},
};

use crate::app::{ChatMessage, ContentBlock, MessageRole, ToolCallInfo};

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

        // 1. Flatten all messages into renderable lines (simple approach for now)
        // In a real optimized version, we'd only render what's visible, but for now we flatten all.
        let mut lines: Vec<Line> = Vec::new();

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
                        for line in text.lines() {
                            lines.push(Line::from(vec![Span::raw("     "), Span::raw(line)]));
                        }
                    }
                    ContentBlock::Code { language, code } => {
                        lines.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(
                                format!("```{}", language),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                        for line in code.lines() {
                            lines.push(Line::from(vec![
                                Span::raw("     "),
                                Span::styled(line, Style::default().fg(Color::Gray)),
                            ]));
                        }
                        lines.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled("```", Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                    ContentBlock::ToolCall(info) => {
                        lines.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(
                                format!("> Tool Call: {}({})", info.name, info.args),
                                Style::default().fg(Color::Magenta),
                            ),
                        ]));
                    }
                    ContentBlock::ToolResult(output) => {
                        lines.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled(
                                format!("> Tool Result: {}", output),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }
            }
            lines.push(Line::from("")); // Spacing
        }

        // 2. Adjust offset/scrolling
        let visible_height = area.height as usize;
        let total_lines = lines.len();

        let mut scroll = state.offset;
        if total_lines > visible_height {
            // Auto-scroll logic could go here or be handled by the caller updating state
        }

        // 3. Render visible lines
        if total_lines > 0 {
            // Simple render loop
            for (i, line) in lines.iter().skip(scroll).take(visible_height).enumerate() {
                let x = area.x;
                let y = area.y + i as u16;
                buf.set_line(x, y, line, area.width);
            }
        }
    }
}
