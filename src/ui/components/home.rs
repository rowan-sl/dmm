use std::{cmp, fs, sync::Arc};

use color_eyre::eyre::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use flume::Sender;
use notify_rust::Notification;
use rand::Rng;
use ratatui::{prelude::*, widgets::*};

use super::Component;
use crate::{
    cache,
    cfg::Config,
    player2::{self, SingleTrackPlayer},
    resolver::Resolver,
    schema::{Playlist, Track},
    ui::action::Action,
};

mod draw;

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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TrackID {
    pub track: usize,
    pub playlist: PlaylistID,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PlaylistID {
    pub playlist: usize,
}

pub struct Home {
    command_tx: Option<Sender<Action>>,
    // resolver
    resolver: Arc<Resolver>,
    // playing state
    current: TrackID,
    // player
    player: SingleTrackPlayer,
    sel_method: TrackSelectionMethod,
    repeat: Repeat,
    /// weather or not to play the next track when this one is done
    /// disabled when the end of the playlist is reached on Repeat::Never
    /// enabled when a track is selected or play/pause is pressed
    autoplay: bool,
    // config
    cfg: Config,
    // track selection list
    t_list_state: ListState,
    // playlist selection list
    p_list_state: ListState,
    /// jump to track # when receiving TrackComplete (takes precedence over normal track selection)
    /// used in track selection (set jump_on_track_complete -> stop playback -> trigger Action::TrackComplete -> play jump_on_track_complete)
    jump_on_track_complete: Option<TrackID>,
}

impl Home {
    pub fn new(res: Arc<Resolver>) -> Result<Self> {
        debug!("Initializing audio backend");
        let host = cpal::default_host();
        let Some(device) = host.default_output_device().map(Arc::new) else {
            error!("No audio output device exists!");
            bail!("failed to initialize audio backend");
        };
        let config = Arc::new(match device.default_output_config() {
            Ok(config) => config,
            Err(err) => {
                error!("failed to get default audio output device config: {}", err);
                bail!("failed to initialize audio backend");
            }
        });
        let player = SingleTrackPlayer::new(config.clone(), device.clone())?;

        Ok(Self {
            command_tx: None,
            current: TrackID {
                track: 0,
                // FIXME: proper mechanism for selecting default playlist
                playlist: PlaylistID { playlist: 0 },
            },
            player,
            sel_method: TrackSelectionMethod::Sequential,
            repeat: Repeat::RepeatPlaylist,
            autoplay: true,
            cfg: Config::default(),
            t_list_state: ListState::default().with_selected(Some(0)),
            p_list_state: ListState::default().with_selected(None),
            jump_on_track_complete: None,
            resolver: res,
        })
    }

    fn get_track(&self, track: TrackID) -> &Track {
        &self.get_playlist(track.playlist).tracks[track.track]
    }

    fn get_playlist(&self, playlist: PlaylistID) -> &Playlist {
        &self.resolver.out().playlists[playlist.playlist]
    }

    fn select_next_track(&mut self) -> Result<()> {
        match (self.repeat, self.sel_method) {
            (
                Repeat::RepeatTrack,
                TrackSelectionMethod::Random | TrackSelectionMethod::Sequential,
            ) => { /* no-op: select current track */ }
            (Repeat::Never | Repeat::RepeatPlaylist, TrackSelectionMethod::Random) => {
                self.current.track = rand::thread_rng()
                    .gen_range(0..self.get_playlist(self.current.playlist).tracks.len());
            }
            (rep, TrackSelectionMethod::Sequential) => {
                if self.current.track != self.get_playlist(self.current.playlist).tracks.len() - 1 {
                    self.current.track += 1;
                } else {
                    match rep {
                        Repeat::Never => {
                            self.autoplay = false;
                            self.player.stop()?;
                            let _handle = Notification::new()
                                .summary("DMM Player")
                                .body("Playlist Complete - Stopping")
                                .show()?;
                        }
                        Repeat::RepeatPlaylist => {
                            self.current.track = 0;
                        }
                        Repeat::RepeatTrack => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }

    fn play_c_track(&mut self) -> Result<()> {
        let track = self.get_track(self.current);
        let hash = cache::Hash::generate(
            self.resolver
                .out()
                .sources
                .iter()
                .find(|x| x.name == track.src)
                .ok_or(anyhow!("could not find track source"))?,
            &track.input,
        );
        let track_path = self.resolver.out().cache.find(hash).ok_or_else(|| {
            error!("Could not find file for track. It is probably not downloaded");
            info!("Try downloading the playlist with `dmm download`");
            anyhow!("could not find file for track!")
        })?;
        let track_fmt = self
            .get_playlist(self.current.playlist)
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
                if self.autoplay {
                    trace!("Playing next track");
                    if let Some(idx) = self.jump_on_track_complete.take() {
                        self.current = idx;
                        // do not send notifications about playing a track by selection (the person using the app did this, they don't need to know)
                    } else {
                        self.select_next_track()?;
                        let track = self.get_track(self.current);
                        let _handle = Notification::new()
                            .summary("DMM Player")
                            .body(&format!(
                                "Now Playing: {name}\nby {artist}",
                                name = track.meta.name,
                                artist = track.meta.artist
                            ))
                            .show()?;
                    }
                    self.play_c_track()?;
                }
            }
            Action::PausePlay => {
                self.autoplay = true;
                match self.player.state() {
                    player2::State::Playing => self.player.pause()?,
                    player2::State::Paused => self.player.play()?,
                    player2::State::Stopped => {
                        match self.sel_method {
                            TrackSelectionMethod::Random => self.select_next_track()?,
                            TrackSelectionMethod::Sequential => self.current.track = 0,
                        }
                        self.play_c_track()?;
                    }
                }
            }
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
            Action::ListLeft => {
                self.t_list_state.select(Some(self.current.track));
                self.p_list_state.select(None);
            }
            Action::ListRight => {
                self.t_list_state.select(None);
                self.p_list_state
                    .select(Some(self.current.playlist.playlist));
            }
            Action::ListSelNext => {
                if self.t_list_state.selected().is_some() {
                    self.t_list_state.select(Some(cmp::min(
                        self.t_list_state.selected().unwrap() + 1,
                        self.get_playlist(self.current.playlist).tracks.len() - 1,
                    )))
                } else if self.p_list_state.selected().is_some() {
                    self.p_list_state.select(Some(cmp::min(
                        self.p_list_state.selected().unwrap() + 1,
                        self.resolver.out().playlists.len() - 1,
                    )))
                }
            }
            Action::ListSelPrev => {
                if self.t_list_state.selected().is_some() {
                    self.t_list_state.select(Some(
                        self.t_list_state.selected().unwrap().saturating_sub(1),
                    ))
                } else if self.p_list_state.selected().is_some() {
                    self.p_list_state.select(Some(
                        self.p_list_state.selected().unwrap().saturating_sub(1),
                    ))
                }
            }
            Action::ListChooseSelected => {
                if self.t_list_state.selected().is_some() {
                    self.autoplay = true;
                    if self.player.state() == player2::State::Stopped {
                        self.current.track = self.t_list_state.selected().unwrap();
                        self.play_c_track()?;
                    } else {
                        self.jump_on_track_complete = Some(TrackID {
                            track: self.t_list_state.selected().unwrap(),
                            playlist: self.current.playlist,
                        });
                        self.player.stop()?;
                    }
                } else if self.p_list_state.selected().is_some() {
                    if self.current.playlist.playlist != self.p_list_state.selected().unwrap() {
                        self.autoplay = false;
                        if self.player.state() != player2::State::Stopped {
                            self.player.stop()?;
                        }
                        self.current.track = 0;
                        self.current.playlist.playlist = self.p_list_state.selected().unwrap();
                        self.p_list_state.select(None);
                        self.t_list_state.select(Some(0));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        self.draw_inner(f, area)
    }
}
