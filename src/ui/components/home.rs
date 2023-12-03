use color_eyre::eyre::Result;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

use super::{Component, Frame};
use crate::{
    schema::Track,
    ui::{action::Action, symbol},
};

pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
    // info bar
    playing: bool,
    stopped: bool,
    c_track_number: Option<usize>,
    c_track: Option<Track>,
    number_tracks: Option<usize>,
}

impl Home {
    pub fn new() -> Self {
        Self {
            command_tx: None,
            playing: false,
            stopped: true,
            c_track_number: None,
            c_track: None,
            number_tracks: None,
        }
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
                "DMM".to_string() + " " + symbol::MUSIC_NOTES + " ",
                Style::default().add_modifier(Modifier::BOLD),
            )]))
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::new().fg(Color::Yellow));
        // .title_position(block::Position::Bottom);
        let titlebar_content_area = titlebar.inner(main_layout[0]);
        f.render_widget(titlebar, main_layout[0]);

        let titlebar_content = Paragraph::new(Line::from(vec![
            Span::styled(
                symbol::OCTAGON,
                Style::default()
                    .fg(if self.stopped {
                        Color::LightRed
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            Span::styled(
                symbol::PAUSE,
                Style::default()
                    .fg(if self.playing {
                        Color::DarkGray
                    } else {
                        Color::LightRed
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            Span::styled(
                symbol::PLAY,
                Style::default()
                    .fg(if self.playing {
                        Color::LightGreen
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            "│".fg(Color::Yellow),
            format!(
                "track {n}/{num}: ",
                n = self
                    .c_track_number
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "?".into()),
                num = self
                    .number_tracks
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "?".into())
            )
            .into(),
            self.c_track
                .as_ref()
                .map(|track| track.meta.name.clone())
                .unwrap_or_default()
                .italic(),
        ]))
        .fg(Color::Gray);
        f.render_widget(titlebar_content, titlebar_content_area);

        let content_area = main_layout[1];
        let content = Block::new()
            .title("Playlist".bold())
            .border_style(Style::new().fg(Color::Yellow))
            .borders(Borders::ALL);
        let content_inner_area = content.inner(content_area);
        f.render_widget(content, content_area);

        let content_inner = Paragraph::new(vec![
            Line::from("Classic Christmas Songs".italic()),
            Line::from(vec![25.to_string().bold(), " track(s)".into()]),
            Line::from(vec![1.to_string().bold(), " source(s)".into()]),
        ]);
        f.render_widget(content_inner, content_inner_area);

        Ok(())
    }
}
