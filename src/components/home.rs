use color_eyre::eyre::Result;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

use super::{Component, Frame};
use crate::{action::Action, symbol};

#[derive(Default)]
pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
}

impl Home {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Component for Home {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Tick => {}
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let main_layout = Layout::new()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .horizontal_margin(1)
            .split(area);

        // Title bar
        let titlebar = Block::new()
            .title(Line::from(vec![Span::styled(
                "DMM ",
                Style::default().add_modifier(Modifier::BOLD),
            )]))
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::new().fg(Color::Yellow));
        // .title_position(block::Position::Bottom);
        let titlebar_content_area = titlebar.inner(main_layout[0]);
        f.render_widget(titlebar, main_layout[0]);

        let titlebar_content = Paragraph::new(Line::from(vec![
            Span::styled(
                symbol::PAUSE,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            Span::styled(
                symbol::PLAY,
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            format!("track {n}/{num}: ", n = 5, num = 25).into(),
            "It's The Most Wonderful Time Of The Year".into(),
        ]))
        .fg(Color::Gray);
        f.render_widget(titlebar_content, titlebar_content_area);

        Ok(())
    }
}
