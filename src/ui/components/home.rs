use std::{cmp, fs, iter, sync::Arc};

use color_eyre::eyre::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use flume::Sender;
use notify_rust::Notification;
use rand::Rng;
use ratatui::{prelude::*, widgets::*};

use super::{Component, Frame};
use crate::{
    cache,
    cfg::{self, Config},
    player2::{self, SingleTrackPlayer},
    resolver::Resolver,
    schema::Playlist,
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
    command_tx: Option<Sender<Action>>,
    // info bar
    c_track_idx: usize,
    playlist: Playlist,
    // player
    player: SingleTrackPlayer,
    sel_method: TrackSelectionMethod,
    repeat: Repeat,
    /// has a single run-through (on Repeat::Never) been completed
    play_complete: bool,
    // config
    cfg: Config,
    // track selection list
    t_list_state: ListState,
    /// jump to track # when receiving TrackComplete (takes precedence over normal track selection)
    jump_on_track_complete: Option<usize>,
    // resolver
    resolver: Arc<Resolver>,
}

impl Home {
    pub fn new(pl: Playlist, res: Arc<Resolver>) -> Result<Self> {
        info!("Loaded playlist {name}", name = pl.name);

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
            playlist: pl,
            player,
            sel_method: TrackSelectionMethod::Sequential,
            repeat: Repeat::RepeatPlaylist,
            play_complete: false,
            cfg: Config::default(),
            t_list_state: ListState::default().with_selected(Some(0)),
            jump_on_track_complete: None,
            resolver: res,
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
                            let _handle = Notification::new()
                                .summary("DMM Player")
                                .body("Playlist Complete - Stopping")
                                .show()?;
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
        let hash = cache::Hash::generate(
            self.resolver
                .out()
                .sources
                .iter()
                .find(|x| x.name == track.src)
                .ok_or(anyhow!("could not find track source"))?,
            &track.input,
        );
        let track_path = self
            .resolver
            .out()
            .cache
            .find(hash)
            .ok_or(anyhow!("could not find file for track!"))?;
        let track_fmt = self
            .playlist
            .find_source(&track.src)
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
    fn init(&mut self, _area: Rect) -> Result<()> {
        if self.cfg.play_on_start {
            self.play_c_track()?;
        }
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.cfg = config;
        Ok(())
    }

    fn register_action_handler(&mut self, tx: Sender<Action>) -> Result<()> {
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
            Action::TrackComplete => {
                trace!("Received Track Complete");
                assert_eq!(self.player.state(), player2::State::Stopped);
                trace!("Playing next track");
                if let Some(idx) = self.jump_on_track_complete.take() {
                    self.c_track_idx = idx;
                    // do not send notifications about playing a track by selection (the person using the app did this, they don't need to know)
                } else {
                    self.select_next_track()?;
                    let track = &self.playlist.tracks[self.c_track_idx];
                    let _handle = Notification::new()
                        .summary("DMM Player")
                        .body(&format!(
                            "Now Playing: {name}\nby {artist}",
                            name = track.meta.name,
                            artist = track.meta.artist
                        ))
                        .show()?;
                }
                self.t_list_state.select(Some(self.c_track_idx));
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
            Action::TrackListSelNext => self.t_list_state.select(Some(cmp::min(
                self.t_list_state.selected().unwrap() + 1,
                self.playlist.tracks.len() - 1,
            ))),
            Action::TrackListSelPrev => self.t_list_state.select(Some(
                self.t_list_state.selected().unwrap().saturating_sub(1),
            )),
            Action::TrackListPlaySelected => {
                if self.player.state() == player2::State::Stopped {
                    self.c_track_idx = self.t_list_state.selected().unwrap();
                    self.play_c_track()?;
                } else {
                    self.jump_on_track_complete = Some(self.t_list_state.selected().unwrap());
                    self.player.stop()?;
                }
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

        let sel_track = &self.playlist.tracks[self.t_list_state.selected().unwrap()];
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
                    Action::TrackListSelNext => "track list: next",
                    Action::TrackListSelPrev => "track list: prev",
                    Action::TrackListPlaySelected => "track list: play track",
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

        f.render_stateful_widget(
            List::new(
                self.playlist
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(i, track)| {
                        let i = i + 1;
                        ListItem::new(Line::from(vec![
                            {
                                let fmt = i.to_string();
                                let n_zeroes = 3usize.saturating_sub(fmt.len());
                                let zeroes = iter::repeat('0').take(n_zeroes).collect::<String>();
                                zeroes.dim()
                            },
                            i.to_string().into(),
                            ": ".into(),
                            track.meta.name.clone().italic(),
                        ]))
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
            .highlight_style(Style::new().fg(Color::LightGreen)),
            content_layout[1],
            &mut self.t_list_state,
        );

        Ok(())
    }
}
