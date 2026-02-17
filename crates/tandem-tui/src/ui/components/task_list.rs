use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};

use crate::app::{Task, TaskStatus};

#[derive(Default)]
pub struct TaskListState {
    pub selected_index: usize,
}

pub struct TaskList<'a> {
    tasks: &'a [Task],
    block: Option<Block<'a>>,
    spinner_frame: usize,
}

impl<'a> TaskList<'a> {
    pub fn new(tasks: &'a [Task]) -> Self {
        Self {
            tasks,
            block: None,
            spinner_frame: 0,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    pub fn spinner_frame(mut self, frame: usize) -> Self {
        self.spinner_frame = frame;
        self
    }
}

impl<'a> StatefulWidget for TaskList<'a> {
    type State = TaskListState;

    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut Self::State) {
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

        let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let spinner = spinners[self.spinner_frame % spinners.len()];

        for (i, task) in self.tasks.iter().enumerate() {
            if i >= area.height as usize {
                break;
            }

            let y = area.y + i as u16;

            let (symbol, style) = match task.status {
                TaskStatus::Pending => ("○", Style::default().fg(Color::Gray)),
                TaskStatus::Working => (spinner, Style::default().fg(Color::Yellow)),
                TaskStatus::Done => ("●", Style::default().fg(Color::Green)),
                TaskStatus::Failed => ("✖", Style::default().fg(Color::Red)),
            };

            let desc_style = if task.pinned {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled(format!("{} ", symbol), style),
                Span::styled(&task.description, desc_style),
            ]);

            buf.set_line(area.x, y, &line, area.width);
        }
    }
}
