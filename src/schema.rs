use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};

use color_eyre::eyre::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Link {
    pub music_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DlPlaylist {
    #[serde(skip)]
    pub directory: PathBuf,
    pub name: String,
    /// resolved source
    pub sources: Vec<Source>,
    pub tracks: Vec<DlTrack>,
}

impl DlPlaylist {
    #[allow(dead_code)]
    pub fn find_source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|x| x.name == name)
    }

    pub fn gen_diff(&self, other: &Playlist) -> DlPlaylistDiff {
        let mut diff = DlPlaylistDiff { changes: vec![] };
        if self.name != other.name {
            diff.changes.push(DiffChange::Name {
                old: self.name.clone(),
                new: other.name.clone(),
            });
        }
        let mut source_map: HashMap<String, Source> = HashMap::new();
        // set of sources (by [new] name) that have changed output specification
        let mut source_changed_output: HashSet<String> = HashSet::new();
        // map of source names (old name -> new name)
        let mut source_o2n_names: HashMap<String, String> = HashMap::new();
        for source in self.sources.clone() {
            source_map.insert(source.name.clone(), source);
        }
        for source in other.resolved_sources.clone().unwrap() {
            if let Some(old) = source_map.remove(&source.name) {
                if source.format != old.format {
                    source_changed_output.insert(source.name.clone());
                    diff.changes.push(DiffChange::SourceChangeFormat {
                        name: source.name.clone(),
                        old: old.format.clone(),
                        new: source.format.clone(),
                    })
                }
                // if these are not matching, then emit DiffChange::SourceReplaceKind
                if old.kind != source.kind {
                    source_changed_output.insert(source.name.clone());
                    diff.changes.push(DiffChange::SourceModifyKind {
                        name: source.name.clone(),
                        old: old.kind,
                        new: source.kind,
                    });
                }
            } else {
                diff.changes.push(DiffChange::AddSource { new: source });
            }
        }
        for source in source_map.into_values() {
            diff.changes.push(DiffChange::DelSource { removed: source });
        }
        let mut source_map: HashMap<SourceKind, String> = HashMap::new();
        for source in self.sources.clone() {
            source_map.insert(source.kind, source.name);
        }
        for source in other.sources.clone() {
            if let Some(old) = source_map.remove(&source.kind) {
                if old != source.name {
                    source_o2n_names.insert(old.clone(), source.name.clone());
                    diff.changes.push(DiffChange::SourceChangeName {
                        old,
                        new: source.name,
                    })
                }
            }
        }

        let mut track_map: HashMap<Meta, Track> = HashMap::new();
        for track in self.tracks.clone() {
            track_map.insert(track.track.meta.clone(), track.track.clone());
        }
        for track in other.tracks.clone() {
            if let Some(old) = track_map.remove(&track.meta) {
                if track.src != old.src || track.input != old.input {
                    diff.changes
                        .push(DiffChange::TrackChangedSource { old, new: track });
                }
            } else {
                diff.changes.push(DiffChange::AddTrack { new: track });
            }
        }
        for track in track_map.into_values() {
            diff.changes.push(DiffChange::DelTrack { removed: track });
        }
        let mut track_map: HashMap<(String, ron::Value), Track> = HashMap::new();
        for track in self.tracks.clone() {
            track_map.insert(
                (track.track.src.clone(), track.track.input.clone()),
                track.track.clone(),
            );
        }
        for track in other.tracks.clone() {
            if let Some(old) = track_map.remove(&(track.src.clone(), track.input.clone())) {
                if old.meta != track.meta {
                    diff.changes
                        .push(DiffChange::TrackLikelyChangedMeta { old, new: track })
                }
            }
        }
        diff
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DlPlaylistDiff {
    pub changes: Vec<DiffChange>,
}

impl DlPlaylistDiff {
    pub fn display(&self) {
        println!(" --- playlist diff --- ");
        for change in &self.changes {
            use DiffChange::*;
            match change.clone() {
                Name { old, new } => println!(" M [playlist/name]\t {old} -> {new}"),
                AddSource { new } => println!(
                    " + [source]\t {name} | output: {format} | kind: {variant}",
                    name = new.name,
                    format = new.format,
                    variant = match new.kind {
                        SourceKind::Shell { .. } => "shell",
                    }
                ),
                DelSource { removed } => println!(" - [source]\t {name}", name = removed.name),
                SourceChangeName { old, new } => println!(" M [source/name]\t {old} -> {new}"),
                SourceChangeFormat { name, old, new } => {
                    println!(" M [source/format]\t of source {name}: {old} -> {new}")
                }
                SourceReplaceKind { name, old, new } => {
                    println!(
                        " M [source/kind]\t of source {name}: {old} -> {new}",
                        old = match old {
                            SourceKind::Shell { .. } => "shell",
                        },
                        new = match new {
                            SourceKind::Shell { .. } => "shell",
                        }
                    )
                }
                SourceModifyKind { name, old, new } => match (old, new) {
                    (
                        SourceKind::Shell {
                            cmd: old_cmd,
                            args: old_args,
                        },
                        SourceKind::Shell { cmd, args },
                    ) => {
                        if old_cmd != cmd && old_args != args {
                            println!(" M [source/kind/all]\t of source {name}:\n    M [cmd]\t {old_cmd} -> {cmd}\n    M [args]\t {old_args:?} -> {args:?}")
                        } else if old_cmd != cmd {
                            println!(" M [source/kind/-cmd]\t of source {name}: {old_cmd} -> {cmd}")
                        } else if old_args != args {
                            println!(" M [source/kind/-args]\t of source {name}: {old_args:?} -> {args:?}")
                        }
                    }
                },
                AddTrack { new } => println!(
                    " + [track]\t '{name}' by {artist}",
                    name = new.meta.name,
                    artist = new.meta.artist
                ),
                DelTrack { removed } => println!(
                    " - [track]\t '{name}' by {artist}",
                    name = removed.meta.name,
                    artist = removed.meta.artist
                ),
                TrackLikelyChangedMeta { old, new } => println!(" M [track/rename]\n    M [track/rename/name]\t {} -> {}\n    M [track/rename/artist]\t {} -> {}", old.meta.name, new.meta.name, old.meta.artist, new.meta.artist),
                TrackChangedSource { old, new } => println!(" M [track/source]\t track '{name}' by {artist}: {} with {:?} -> {} with {:?}",  old.src, old.input, new.src, new.input, name=old.meta.name, artist=old.meta.artist,),
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DiffChange {
    Name {
        old: String,
        new: String,
    },
    AddSource {
        new: Source,
    },
    DelSource {
        removed: Source,
    },
    SourceChangeName {
        old: String,
        new: String,
    },
    SourceChangeFormat {
        name: String,
        old: String,
        new: String,
    },
    SourceReplaceKind {
        name: String,
        old: SourceKind,
        new: SourceKind,
    },
    /// modification to the *content* of SourceKind (variant is the same)
    SourceModifyKind {
        name: String,
        old: SourceKind,
        new: SourceKind,
    },
    AddTrack {
        new: Track,
    },
    DelTrack {
        removed: Track,
    },
    /// the metadata of a track (name or author) changed, but the `src` and `input` did not.
    /// this likely means that the track was re-named
    TrackLikelyChangedMeta {
        old: Track,
        new: Track,
    },
    /// the metadata of a track (name/author) is the same, but the `src` and/or `input` changed.
    TrackChangedSource {
        old: Track,
        new: Track,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DlTrack {
    pub track: Track,
    pub track_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Playlist {
    #[serde(skip)]
    pub file_path: PathBuf,
    pub name: String,
    pub import: Vec<Import>,
    pub sources: Vec<Source>,
    #[serde(skip)]
    pub resolved_sources: Option<Vec<Source>>,
    pub tracks: Vec<Track>,
}

impl Playlist {
    /// Panics if playlist sources are not yet resolved
    pub fn find_source(&self, name: &str) -> Option<&Source> {
        self.resolved_sources
            .as_ref()
            .unwrap()
            .iter()
            .find(|x| x.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Import {
    Source(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Source {
    pub name: String,
    pub format: String,
    pub kind: SourceKind,
}

impl Source {
    pub fn execute(&self, input: ron::Value, output: &Path) -> Result<()> {
        let SourceKind::Shell { cmd, args } = &self.kind;
        let ron::Value::String(input) = input else {
            bail!("shell source expects a string for its input argument (found: {input:?})");
        };
        let args = args
            .iter()
            .map(|arg| {
                Ok(if arg.contains("${input}") {
                    arg.replace("${input}", &input)
                } else if arg.contains("${output}") {
                    arg.replace(
                        "${output}",
                        output
                            .to_str()
                            .ok_or(anyhow!("output path not valid UTF-8"))?,
                    )
                } else {
                    arg.to_owned()
                })
            })
            .collect::<Result<Vec<String>>>()?;
        let res = Command::new(cmd).args(args).status()?;
        if res.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to download {input:?} from shell source {} - command exited with status {}",
                self.name,
                res
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Shell { cmd: String, args: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Track {
    pub meta: Meta,
    pub src: String,
    pub input: ron::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Meta {
    pub name: String,
    pub artist: String,
}
