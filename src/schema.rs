use std::{
    path::{Path, PathBuf},
    process::Command,
};

use color_eyre::eyre::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Link {
    pub music_directory: PathBuf,
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
                Ok(arg.replace("${input}", &input).replace(
                    "${output}",
                    output
                        .to_str()
                        .ok_or(anyhow!("output path not valid UTF-8"))?,
                ))
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
