use std::{fs, path::PathBuf, sync::Arc};

use color_eyre::eyre::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

use super::{Component, Frame};
use crate::{
    player2::{self, SingleTrackPlayer},
    schema::{self, DlPlaylist, Track},
    ui::{action::Action, symbol},
};

pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
    // info bar
    c_track_number: usize,
    c_track: Track,
    number_tracks: usize,
    playlist: DlPlaylist,
    //
    player: SingleTrackPlayer,
}

impl Home {
    pub fn new(pl_dir: PathBuf) -> Result<Self> {
        info!("Loading playlist {pl_dir:?}");
        if !pl_dir.try_exists()? {
            bail!("Failed to load: playlist does not exist (no such directory)");
        }
        if !pl_dir.join("dl_playlist.ron").try_exists()? {
            bail!("Failed to load: playlist does not exist (no manifest `dl_playlist.ron` file in given directory)");
        }
        let dl_pl_str = fs::read_to_string(pl_dir.join("dl_playlist.ron"))?;
        let dl_pl = ron::from_str::<schema::DlPlaylist>(&dl_pl_str)?;
        info!("Loaded playlist {name}", name = dl_pl.name);

        let track = dl_pl.tracks.get(1).unwrap();
        let track_path = pl_dir
            .read_dir()?
            .find(|res| {
                res.as_ref().is_ok_and(|entry| {
                    entry
                        .path()
                        .file_stem()
                        .is_some_and(|name| name.to_string_lossy() == track.track_id.to_string())
                })
            })
            .ok_or(anyhow!("BUG: could not file file for downloaded track"))??
            .path();
        let track_fmt = dl_pl.find_source(&track.track.src).unwrap().format.clone();

        debug!("Initializing audio backend");
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            error!("No audio output device exists!");
            bail!("failed to initialize audio backend");
        };
        let config = match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                error!("failed to get default audio output device config: {}", err);
                bail!("failed to initialize audio backend");
            }
        };
        let mut player = SingleTrackPlayer::new(Arc::new(config), Arc::new(device))?;

        player.set_track(fs::File::open(&track_path)?, track_fmt)?;
        player.play()?;

        Ok(Self {
            command_tx: None,
            c_track_number: 2,
            c_track: track.track.clone(),
            number_tracks: dl_pl.tracks.len(),
            playlist: dl_pl,
            player,
        })
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
                    .fg(if self.player.state() == player2::State::Stopped {
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
                    .fg(if self.player.state() == player2::State::Paused {
                        Color::LightRed
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            " ".into(),
            Span::styled(
                symbol::PLAY,
                Style::default()
                    .fg(if self.player.state() == player2::State::Playing {
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
                n = self.c_track_number,
                num = self.number_tracks
            )
            .into(),
            self.c_track.meta.name.clone().italic(),
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
            Line::from(self.playlist.name.clone().italic()),
            Line::from(vec![
                self.playlist.tracks.len().to_string().bold(),
                " track(s)".into(),
            ]),
            Line::from(vec![
                self.playlist.sources.len().to_string().bold(),
                " source(s)".into(),
            ]),
        ]);
        f.render_widget(content_inner, content_inner_area);

        Ok(())
    }
}
