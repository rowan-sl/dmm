use std::time::Instant;

use color_eyre::eyre::Result;
use ratatui::{prelude::*, widgets::*};

use super::Component;
use crate::ui::{action::Action, tui::Frame};

#[derive(Debug, Clone, PartialEq)]
pub struct FpsCounter {
    render_start_time: Instant,
    render_frames: u32,
    render_fps: f64,
}

impl Default for FpsCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            render_start_time: Instant::now(),
            render_frames: 0,
            render_fps: 0.0,
        }
    }

    fn render_tick(&mut self) -> Result<()> {
        self.render_frames += 1;
        let now = Instant::now();
        let elapsed = (now - self.render_start_time).as_secs_f64();
        if elapsed >= 1.0 {
            self.render_fps = self.render_frames as f64 / elapsed;
            self.render_start_time = now;
            self.render_frames = 0;
        }
        Ok(())
    }
}

impl Component for FpsCounter {
    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::Render = action {
            self.render_tick()?
        };
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, rect: Rect) -> Result<()> {
        let rects = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(1), // row 2
                Constraint::Min(0),
            ])
            .horizontal_margin(1)
            .split(rect);

        let rect = rects[0];

        let s = format!("{:.2} fps (render)", self.render_fps);
        let block = Block::default().title(block::Title::from(s.dim()).alignment(Alignment::Right));
        f.render_widget(block, rect);
        Ok(())
    }
}
