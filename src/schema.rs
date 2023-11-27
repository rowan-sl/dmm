use std::{io::Write, path::PathBuf, process::Command};

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Hash)]
pub struct Playlist {
    name: String,
    sources: Vec<Source>,
    tracks: Vec<Track>,
}

impl Playlist {
    pub fn find_source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|x| x.name == name)
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Hash)]
pub struct Source {
    pub name: String,
    pub format: String,
    pub kind: SourceKind,
}

impl Source {
    pub fn execute(&self, input: ron::Value, output: PathBuf) -> Result<PathBuf> {
        let SourceKind::Shell { cmd, args } = &self.kind;
        let output = output.with_extension(&self.format);
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
        let res = Command::new(cmd).args(args).output()?;
        if res.status.success() {
            Ok(output)
        } else {
            std::io::stdout().lock().write_all(&res.stderr)?;
            Err(anyhow!(
                "Failed to download {input:?} from shell source {} - command exited with status {}",
                self.name,
                res.status
            ))
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Hash)]
pub enum SourceKind {
    Shell { cmd: String, args: Vec<String> },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Hash)]
pub struct Track {
    pub meta: Meta,
    pub src: String,
    pub input: ron::Value,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Hash)]
pub struct Meta {
    pub name: String,
    pub artist: String,
}
