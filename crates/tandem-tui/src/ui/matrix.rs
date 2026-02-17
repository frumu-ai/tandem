use rand::Rng;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::Color,
    widgets::{Block, BorderType, Borders, Widget},
};

const RAIN_GREEN: Color = Color::Rgb(20, 210, 90);
const RAIN_GREEN_DIM: Color = Color::Rgb(10, 120, 50);
const RAIN_GREEN_HI: Color = Color::Rgb(90, 255, 140);
const RAIN_DARK: Color = Color::Rgb(2, 10, 4);
const LOGO_FG: Color = RAIN_GREEN_HI;
const HEAD_COLOR: Color = Color::Rgb(220, 255, 235);

const CHAR_POOL: &str =
    "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜｦﾝ0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ@#$%&*";

// "TANDEM" in ANSI Shadow font
const LOGO_LINES: [&str; 6] = [
    "████████╗ █████╗ ███╗   ██╗██████╗ ███████╗███╗   ███╗",
    "╚══██╔══╝██╔══██╗████╗  ██║██╔══██╗██╔════╝████╗ ████║",
    "   ██║   ███████║██╔██╗ ██║██║  ██║█████╗  ██╔████╔██║",
    "   ██║   ██╔══██║██║╚██╗██║██║  ██║██╔══╝  ██║╚██╔╝██║",
    "   ██║   ██║  ██║██║ ╚████║██████╔╝███████╗██║ ╚═╝ ██║",
    "   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═══╝╚═════╝ ╚══════╝╚═╝     ╚═╝",
];

#[derive(Clone, Debug, PartialEq)]
struct Drop {
    x: u16,
    y: f32,
    speed: f32,
    chars: Vec<char>,
    len: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixEffect {
    drops: Vec<Drop>,
    width: u16,
    height: u16,
    tick_count: usize,
    char_pool: Vec<char>,
}

impl MatrixEffect {
    pub fn new(width: u16, height: u16) -> Self {
        let mut rng = rand::thread_rng();
        let mut drops = Vec::new();
        let char_pool: Vec<char> = CHAR_POOL.chars().collect();

        // Initial population
        for i in 0..width {
            if rng.gen_bool(0.15) {
                drops.push(Self::create_drop(i, height, &mut rng, &char_pool));
            }
        }

        Self {
            drops,
            width,
            height,
            tick_count: 0,
            char_pool,
        }
    }

    fn random_char<R: Rng>(rng: &mut R, pool: &[char]) -> char {
        let idx = rng.gen_range(0..pool.len());
        pool[idx]
    }

    fn random_len<R: Rng>(rng: &mut R, height: u16) -> usize {
        let min_len = 10usize;
        let max_len = (height as usize).saturating_sub(4).clamp(14, 36);
        if max_len <= min_len {
            min_len
        } else {
            rng.gen_range(min_len..=max_len)
        }
    }

    fn create_drop<R: Rng>(x: u16, height: u16, rng: &mut R, pool: &[char]) -> Drop {
        let len = Self::random_len(rng, height);
        let chars: Vec<char> = (0..len)
            .map(|_| {
                if rng.gen_bool(0.03) {
                    ' '
                } else {
                    Self::random_char(rng, pool)
                }
            })
            .collect();

        Drop {
            x,
            y: rng.gen_range(-(height as f32 * 2.0)..0.0),
            speed: rng.gen_range(0.8..2.2),
            chars,
            len,
        }
    }

    pub fn update(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.tick_count += 1;
        let mut rng = rand::thread_rng();
        if width == 0 || height == 0 {
            self.drops.clear();
            return;
        }

        let target = ((width as f32) * 0.9).round() as usize;
        let mut occupied = vec![false; width as usize];
        for drop in &self.drops {
            let x = drop.x as usize;
            if x < occupied.len() {
                occupied[x] = true;
            }
        }

        if self.drops.len() < target && rng.gen_bool(0.85) {
            let available: Vec<u16> = occupied
                .iter()
                .enumerate()
                .filter_map(|(i, taken)| if !*taken { Some(i as u16) } else { None })
                .collect();
            if !available.is_empty() {
                let count = (available.len() as f32 * 0.25).ceil() as usize;
                for _ in 0..count.min(3) {
                    let x = available[rng.gen_range(0..available.len())];
                    self.drops
                        .push(Self::create_drop(x, height, &mut rng, &self.char_pool));
                }
            }
        }

        // Update positions
        for drop in &mut self.drops {
            drop.y += drop.speed;
            if drop.x >= width {
                drop.x = drop.x % width;
            }

            // Randomly change characters
            if rng.gen_bool(0.45) && !drop.chars.is_empty() {
                drop.chars[0] = Self::random_char(&mut rng, &self.char_pool);
            }
            if rng.gen_bool(0.18) {
                let updates = rng.gen_range(1..=3);
                for _ in 0..updates {
                    let idx = rng.gen_range(0..drop.len);
                    drop.chars[idx] = if rng.gen_bool(0.06) {
                        ' '
                    } else {
                        Self::random_char(&mut rng, &self.char_pool)
                    };
                }
            }

            if drop.y - drop.len as f32 > height as f32 + 4.0 {
                *drop = Self::create_drop(drop.x, height, &mut rng, &self.char_pool);
            }
        }

        self.drops
            .retain(|d| (d.y as i32 - d.len as i32) < height as i32 + 6);
    }

    fn render_logo(&self, area: Rect, buf: &mut Buffer) {
        let logo_width = LOGO_LINES[0].chars().count() as u16;
        let logo_height = LOGO_LINES.len() as u16;

        let center_x = area.x + area.width / 2;
        let center_y = area.y + area.height / 2;

        let start_x = center_x.saturating_sub(logo_width / 2);
        let start_y = center_y.saturating_sub(logo_height / 2);

        // Define Box Area (Padding around logo)
        let box_padding_x = 6;
        let box_padding_y = 2;
        let box_area = Rect {
            x: start_x.saturating_sub(box_padding_x),
            y: start_y.saturating_sub(box_padding_y),
            width: logo_width + (box_padding_x * 2),
            height: logo_height + (box_padding_y * 2),
        };

        // Render Black Box Background + Border
        // Clear area first
        for y in box_area.y..box_area.y + box_area.height {
            for x in box_area.x..box_area.x + box_area.width {
                if x < area.width && y < area.height {
                    buf[(x, y)].set_bg(Color::Black).set_char(' ');
                }
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(ratatui::style::Style::default().fg(RAIN_GREEN))
            .title(ratatui::text::Span::styled(
                " TANDEM ",
                ratatui::style::Style::default().fg(RAIN_GREEN_HI),
            ))
            .title_alignment(Alignment::Center);

        block.render(box_area, buf);

        // Render Logo
        for (i, line) in LOGO_LINES.iter().enumerate() {
            let y = start_y + i as u16;
            if y >= area.height {
                break;
            }

            let mut x_off = 0;
            for char in line.chars() {
                let x = start_x + x_off;
                x_off += 1;
                if x >= area.width {
                    continue;
                }

                let cell = &mut buf[(x, y)];
                cell.set_char(char).set_fg(LOGO_FG);
            }
        }

        // Instructions below logo
        let instr = "Press <ENTER> to Start";
        let instr_x = center_x.saturating_sub(instr.len() as u16 / 2);
        let instr_y = box_area.y + box_area.height + 1;
        if instr_y < area.height {
            buf.set_string(
                instr_x,
                instr_y,
                instr,
                ratatui::style::Style::default().fg(Color::Gray),
            );
        }
    }

    fn logo_layout(area: Rect) -> (Rect, Option<Rect>) {
        let logo_width = LOGO_LINES[0].chars().count() as u16;
        let logo_height = LOGO_LINES.len() as u16;
        let center_x = area.x + area.width / 2;
        let center_y = area.y + area.height / 2;
        let start_x = center_x.saturating_sub(logo_width / 2);
        let start_y = center_y.saturating_sub(logo_height / 2);
        let box_padding_x = 6;
        let box_padding_y = 2;
        let box_area = Rect {
            x: start_x.saturating_sub(box_padding_x),
            y: start_y.saturating_sub(box_padding_y),
            width: logo_width + (box_padding_x * 2),
            height: logo_height + (box_padding_y * 2),
        };

        let instr = "Press <ENTER> to Start";
        let instr_x = center_x.saturating_sub(instr.len() as u16 / 2);
        let instr_y = box_area.y + box_area.height + 1;
        let instr_area = if instr_y < area.height {
            Some(Rect {
                x: instr_x,
                y: instr_y,
                width: instr.len() as u16,
                height: 1,
            })
        } else {
            None
        };

        (box_area, instr_area)
    }

    fn color_for_offset(&self, index: usize, len: usize) -> Color {
        if index == 0 {
            return HEAD_COLOR;
        }
        let denom = (len.saturating_sub(1)).max(1) as f32;
        let t = 1.0 - (index as f32 / denom);
        let t = t * t;
        let g = (18.0 + 170.0 * t).round() as u8;
        let r = (4.0 + 16.0 * t).round() as u8;
        let b = (4.0 + 16.0 * t).round() as u8;
        Color::Rgb(r, g, b)
    }

    fn render_in_area(&self, area: Rect, buf: &mut Buffer, show_logo: bool) {
        let (logo_area, instr_area) = if show_logo {
            Self::logo_layout(area)
        } else {
            (Rect::default(), None)
        };

        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_bg(RAIN_DARK).set_char(' ');
            }
        }

        for drop in &self.drops {
            let x = drop.x;
            if x >= area.width {
                continue;
            }

            for (i, char) in drop.chars.iter().enumerate() {
                let y = (drop.y as i32) - (i as i32);
                if y >= 0 && y < area.height as i32 {
                    let y = y as u16;

                    if show_logo {
                        let in_logo = x >= logo_area.x
                            && x < logo_area.x + logo_area.width
                            && y >= logo_area.y
                            && y < logo_area.y + logo_area.height;
                        let in_instr =
                            instr_area.map_or(false, |r| x >= r.x && x < r.x + r.width && y == r.y);
                        if in_logo || in_instr {
                            continue;
                        }
                    }

                    let color = if i < drop.len {
                        self.color_for_offset(i, drop.len)
                    } else {
                        RAIN_GREEN_DIM
                    };

                    buf[(x + area.x, y + area.y)]
                        .set_char(*char)
                        .set_fg(color)
                        .set_bg(RAIN_DARK);
                }
            }
        }

        if show_logo {
            self.render_logo(area, buf);
        }
    }

    pub fn layer(&self, show_logo: bool) -> MatrixLayer<'_> {
        MatrixLayer {
            matrix: self,
            show_logo,
        }
    }
}

impl Widget for &MatrixEffect {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_in_area(area, buf, true);
    }
}

pub struct MatrixLayer<'a> {
    matrix: &'a MatrixEffect,
    show_logo: bool,
}

impl Widget for MatrixLayer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.matrix.render_in_area(area, buf, self.show_logo);
    }
}
