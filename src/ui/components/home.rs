use std::{fs, path::PathBuf, sync::Arc};

use color_eyre::eyre::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use rand::Rng;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

use super::{Component, Frame};
use crate::{
    cfg::{self, Config},
    player2::{self, SingleTrackPlayer},
    schema::{self, DlPlaylist},
    ui::{action::Action, mode::Mode, symbol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TrackSelectionMethod {
    Random,
    Sequential,
}

impl TrackSelectionMethod {
    pub fn next(&mut self) {
        match self {
            Self::Random => *self = Self::Sequential,
            Self::Sequential => *self = Self::Random,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Repeat {
    Never,
    RepeatPlaylist,
    RepeatTrack,
}

impl Repeat {
    pub fn next(&mut self) {
        *self = match self {
            Self::Never => Self::RepeatPlaylist,
            Self::RepeatPlaylist => Self::RepeatTrack,
            Self::RepeatTrack => Self::Never,
        };
    }
}

pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
    // info bar
    c_track_idx: usize,
    playlist: DlPlaylist,
    playlist_dir: PathBuf,
    // player
    player: SingleTrackPlayer,
    sel_method: TrackSelectionMethod,
    repeat: Repeat,
    /// has a single run-through (on Repeat::Never) been completed
    play_complete: bool,
    // config
    cfg: Config,
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
        let player = SingleTrackPlayer::new(Arc::new(config), Arc::new(device))?;

        Ok(Self {
            command_tx: None,
            c_track_idx: 0,
            playlist: dl_pl,
            playlist_dir: pl_dir,
            player,
            sel_method: TrackSelectionMethod::Sequential,
            repeat: Repeat::RepeatPlaylist,
            play_complete: false,
            cfg: Config::default(),
        })
    }

    fn select_next_track(&mut self) -> Result<()> {
        match (self.repeat, self.sel_method) {
            (
                Repeat::RepeatTrack,
                TrackSelectionMethod::Random | TrackSelectionMethod::Sequential,
            ) => { /* no-op: select current track */ }
            (Repeat::Never | Repeat::RepeatPlaylist, TrackSelectionMethod::Random) => {
                self.c_track_idx = rand::thread_rng().gen_range(0..self.playlist.tracks.len());
            }
            (rep, TrackSelectionMethod::Sequential) => {
                if self.c_track_idx != self.playlist.tracks.len() - 1 {
                    self.c_track_idx += 1;
                } else {
                    match rep {
                        Repeat::Never => {
                            self.player.stop()?;
                            self.play_complete = true;
                        }
                        Repeat::RepeatPlaylist => {
                            self.c_track_idx = 0;
                        }
                        Repeat::RepeatTrack => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }

    fn play_c_track(&mut self) -> Result<()> {
        if self.play_complete {
            return Ok(());
        }
        let track = self.playlist.tracks.get(self.c_track_idx).unwrap();
        let track_path =
            self.playlist_dir
                .read_dir()?
                .find(|res| {
                    res.as_ref().is_ok_and(|entry| {
                        entry.path().file_stem().is_some_and(|name| {
                            name.to_string_lossy() == track.track_id.to_string()
                        })
                    })
                })
                .ok_or(anyhow!("BUG: could not file file for downloaded track"))??
                .path();
        let track_fmt = self
            .playlist
            .find_source(&track.track.src)
            .unwrap()
            .format
            .clone();
        self.player
            .set_track(fs::File::open(&track_path)?, track_fmt)?;
        self.player.play()?;
        Ok(())
    }
}

impl Component for Home {
    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.cfg = config;
        Ok(())
    }

    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.command_tx = Some(tx);
        let copy = self.command_tx.as_ref().unwrap().clone();
        self.player.on_track_complete(move || {
            trace!("Track Complete");
            let _ = copy.send(Action::TrackComplete);
        })?;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::Tick => {}
            Action::TrackComplete => {
                trace!("Received Track Complete");
                assert_eq!(self.player.state(), player2::State::Stopped);
                trace!("Playing next track");
                self.select_next_track()?;
                self.play_c_track()?;
            }
            Action::PausePlay => match self.player.state() {
                player2::State::Playing => self.player.pause()?,
                player2::State::Paused => self.player.play()?,
                player2::State::Stopped => {
                    if self.play_complete {
                        self.play_complete = false;
                        match self.sel_method {
                            TrackSelectionMethod::Random => self.select_next_track()?,
                            TrackSelectionMethod::Sequential => self.c_track_idx = 0,
                        }
                    } else {
                        // first play of the playlist
                        self.play_c_track()?;
                    }
                }
            },
            Action::ChangeModeSelection => {
                self.sel_method.next();
            }
            Action::ChangeModeRepeat => {
                self.repeat.next();
            }
            Action::NextTrack => {
                // will trigger Action::TrackComplete
                self.player.stop()?;
            }
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
                n = self.c_track_idx + 1,
                num = self.playlist.tracks.len(),
            )
            .into(),
            "│".fg(Color::Yellow),
            self.playlist.tracks[self.c_track_idx]
                .track
                .meta
                .name
                .clone()
                .italic(),
        ]))
        .fg(Color::Gray);
        f.render_widget(titlebar_content, titlebar_content_area);

        let content_layout = Layout::new()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Max(37), Constraint::Min(0)])
            .split(main_layout[1]);
        let info_layout = Layout::new()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Max(6),
                Constraint::Min(0),
            ])
            .split(content_layout[0]);

        let playlist = Paragraph::new(vec![
            Line::from(self.playlist.name.clone().italic()),
            Line::from(vec![
                self.playlist.tracks.len().to_string().bold(),
                " track(s)".into(),
            ]),
            Line::from(vec![
                self.playlist.sources.len().to_string().bold(),
                " source(s)".into(),
            ]),
        ])
        .block(
            Block::new()
                .title("Playlist".bold())
                .border_style(Style::new().fg(Color::Yellow))
                .borders(Borders::ALL),
        );
        f.render_widget(playlist, info_layout[0]);

        let track = Paragraph::new(vec![
            Line::from(
                self.playlist.tracks[self.c_track_idx]
                    .track
                    .meta
                    .name
                    .clone()
                    .italic(),
            ),
            Line::from(vec![
                "by: ".into(),
                self.playlist.tracks[self.c_track_idx]
                    .track
                    .meta
                    .artist
                    .clone()
                    .bold(),
            ]),
        ])
        .block(
            Block::new()
                .title("Track".bold())
                .border_style(Style::new().fg(Color::Yellow))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
        f.render_widget(track, info_layout[1]);
        // let track = Paragraph::new(vec![
        //     "<q> quit".into(),
        //     "<s> toggle shuffle play".into(),
        //     "<r> toggle repeat".into(),
        //     "<n> skip to next track".into(),
        // ])
        let track = Paragraph::new(
            self.cfg
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
                        other => panic!("Unexpected binding to key {other:?} (bound to {keys:?})"),
                    };
                    output.into()
                })
                .collect::<Vec<_>>(),
        )
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
}
