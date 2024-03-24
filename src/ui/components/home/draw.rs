use std::iter;

use color_eyre::eyre::Result;
use ratatui::{prelude::*, widgets::*};

use super::{PlaylistID, Repeat, TrackSelectionMethod};
use crate::{
    cfg,
    player2::{self},
    ui::{action::Action, mode::Mode, symbol},
};

impl super::Home {
    pub(super) fn draw_titlebar(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        // Title bar
        let titlebar = Block::new()
            .title(Line::from(vec![Span::styled(
                "DMM".to_string() + " " + symbol::MUSIC_NOTES + " ",
                Style::default().add_modifier(Modifier::BOLD),
            )]))
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_style(Style::new().fg(Color::Yellow));
        // .title_position(block::Position::Bottom);
        let titlebar_content_area = titlebar.inner(area);
        f.render_widget(titlebar, area);

        let titlebar_content = Paragraph::new(Line::from(vec![
            symbol::SHUFFLE
                .fg(match self.sel_method {
                    TrackSelectionMethod::Random => Color::LightGreen,
                    TrackSelectionMethod::Sequential => Color::DarkGray,
                })
                .add_modifier(Modifier::BOLD),
            " ".into(),
            {
                let (color, sym) = match self.repeat {
                    Repeat::Never => (Color::LightRed, symbol::REPEAT_OFF),
                    Repeat::RepeatPlaylist => (Color::LightGreen, symbol::REPEAT),
                    Repeat::RepeatTrack => (Color::LightBlue, symbol::REPEAT_ONE),
                };
                sym.fg(color)
            }
            .add_modifier(Modifier::BOLD),
            " ".into(),
            symbol::OCTAGON
                .fg(if self.player.state() == player2::State::Stopped {
                    Color::LightRed
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
            " ".into(),
            symbol::PAUSE
                .fg(if self.player.state() == player2::State::Paused {
                    Color::LightRed
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
            " ".into(),
            symbol::PLAY
                .fg(if self.player.state() == player2::State::Playing {
                    Color::LightGreen
                } else {
                    Color::DarkGray
                })
                .add_modifier(Modifier::BOLD),
            " ".into(),
            "│".fg(Color::Yellow),
            format!(
                "{}:{:0>2}->{}:{:0>2}",
                self.player.timestamp() / 60,
                self.player.timestamp() % 60,
                self.player.duration() / 60,
                self.player.duration() % 60,
            )
            .into(),
            "│".fg(Color::Yellow),
            format!(
                "# {n}/{num}",
                n = self.current.track + 1,
                num = self.get_playlist(self.current.playlist).tracks.len(),
            )
            .into(),
            "│".fg(Color::Yellow),
            self.get_track(self.current).meta.name.clone().italic(),
        ]))
        .fg(Color::Gray);
        f.render_widget(titlebar_content, titlebar_content_area);
        Ok(())
    }

    fn draw_info(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let info_layout = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(6),
                Constraint::Max(6),
                Constraint::Min(0),
            ],
        )
        .split(area);

        let selected_playlist = self.get_playlist(PlaylistID {
            playlist: self
                .p_list_state
                .selected()
                .unwrap_or(self.current.playlist.playlist),
        });
        let playlist = Paragraph::new(vec![
            Line::from(selected_playlist.name.clone().italic()),
            Line::from(vec![
                selected_playlist.tracks.len().to_string().bold(),
                " track(s)".into(),
            ]),
            Line::from(vec![
                selected_playlist.sources.len().to_string().bold(),
                " source(s)".into(),
            ]),
            Line::from(vec![
                selected_playlist.import.len().to_string().bold(),
                " import(s)".into(),
            ]),
        ])
        .block(
            Block::new()
                .title("Playlist".bold())
                .border_style(Style::new().fg(Color::Yellow))
                .borders(Borders::ALL),
        );
        f.render_widget(playlist, info_layout[0]);

        let sel_track = &self.get_playlist(self.current.playlist).tracks
            [self.t_list_state.selected().unwrap_or(self.current.track)];
        let track = Paragraph::new(vec![
            Line::from(sel_track.meta.name.clone().italic()),
            Line::from(vec!["by: ".bold(), sel_track.meta.artist.clone().into()]),
        ])
        .block(
            Block::new()
                .title("Track".bold())
                .border_style(Style::new().fg(Color::Yellow))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
        f.render_widget(track, info_layout[1]);
        let mut lines = self
            .cfg
            .keybinds
            .0
            .get(&Mode::Home)
            .unwrap()
            .iter()
            .map(|(keys, action)| {
                let mut output = String::new();
                for key in keys {
                    output += "<";
                    output += cfg::key_event_to_string(key).as_str();
                    output += ">";
                }
                output += " ";
                output += match action {
                    Action::Quit => "quit",
                    Action::PausePlay => "pause/play",
                    Action::ChangeModeSelection => "toggle shuffle play",
                    Action::ChangeModeRepeat => "toggle repeat",
                    Action::NextTrack => "skip",
                    Action::ListLeft => "select track list",
                    Action::ListRight => "select playlist list",
                    Action::ListSelNext => "list: next",
                    Action::ListSelPrev => "list: prev",
                    Action::ListChooseSelected => "list: play track/select playlist",
                    other => panic!("Unexpected binding to key {other:?} (bound to {keys:?})"),
                };
                output
            })
            .collect::<Vec<_>>();
        lines.sort();
        let track = Paragraph::new(lines.into_iter().map(|x| x.into()).collect::<Vec<_>>())
            .block(
                Block::new()
                    .title("Keybinds".bold())
                    .border_style(Style::new().fg(Color::Yellow))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(track, info_layout[2]);
        Ok(())
    }

    pub(super) fn draw_inner(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let main_layout = Layout::new(
            Direction::Vertical,
            [Constraint::Length(3), Constraint::Min(0)],
        )
        .horizontal_margin(1)
        .split(area);
        self.draw_titlebar(f, main_layout[0])?;

        let content_layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Max(37), Constraint::Min(0)],
        )
        .split(main_layout[1]);

        self.draw_info(f, content_layout[0])?;

        let lists_layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(content_layout[1]);
        f.render_stateful_widget(
            List::new(
                self.get_playlist(self.current.playlist)
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(i, track)| {
                        let is_now_playing = i == self.current.track;
                        let i = i + 1;
                        let item = ListItem::new(Line::from(vec![
                            {
                                let fmt = i.to_string();
                                let n_zeroes = 3usize.saturating_sub(fmt.len());
                                let zeroes = iter::repeat('0').take(n_zeroes).collect::<String>();
                                zeroes.dim()
                            },
                            i.to_string().into(),
                            ": ".into(),
                            track.meta.name.clone().italic(),
                        ]));
                        if is_now_playing {
                            item.light_green()
                        } else {
                            item
                        }
                    })
                    .collect::<Vec<_>>(),
            )
            .block(
                Block::new()
                    .title("Track Selection".bold())
                    .border_style(Style::new().fg(Color::Yellow))
                    .borders(Borders::ALL),
            )
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::new().fg(Color::LightCyan)),
            lists_layout[0],
            &mut self.t_list_state,
        );

        f.render_stateful_widget(
            List::new(
                self.resolver
                    .out()
                    .playlists
                    .iter()
                    .enumerate()
                    .map(|(i, pl)| {
                        let is_now_playing = i == self.current.playlist.playlist;
                        let item = if self.p_list_state.selected().is_some_and(|x| x == i) {
                            ListItem::new(Line::from(vec!["> ".into(), pl.name.clone().into()]))
                        } else {
                            ListItem::new(Line::from(vec!["- ".into(), pl.name.clone().into()]))
                        };
                        if is_now_playing {
                            item.light_green()
                        } else {
                            item
                        }
                    })
                    .collect::<Vec<_>>(),
            )
            .block(
                Block::new()
                    .title("Playlist Selection".bold())
                    .border_style(Style::new().fg(Color::Yellow))
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::new().fg(Color::LightCyan)),
            lists_layout[1],
            &mut self.p_list_state,
        );

        Ok(())
    }
}
