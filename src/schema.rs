use std::{io::Write, path::Path, process::Command};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct DlPlaylist {
    pub name: String,
    pub sources: Vec<Source>,
    pub tracks: Vec<DlTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct DlTrack {
    pub track: Track,
    pub track_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct Playlist {
    pub name: String,
    pub sources: Vec<Source>,
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub fn find_source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|x| x.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub enum SourceKind {
    Shell { cmd: String, args: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct Track {
    pub meta: Meta,
    pub src: String,
    pub input: ron::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Hash)]
pub struct Meta {
    pub name: String,
    pub artist: String,
}
