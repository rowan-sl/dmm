use std::{
    fs,
    path::{Path, PathBuf},
};

use color_eyre::eyre::{anyhow, Result};

use crate::{
    cache::CacheDir,
    cfg::Config,
    schema::{self, Playlist, Source},
};

struct State {
    pub resolved: bool,
}

#[derive(Default)]
pub struct Output {
    pub config: Config,
    pub sources: Vec<Source>,
    pub playlists: Vec<Playlist>,
    pub cache: CacheDir,
}

pub struct Directories {
    pub root: PathBuf,
    pub run: PathBuf,
    pub sources: PathBuf,
    pub playlists: PathBuf,
    pub cache: PathBuf,
}

impl Directories {
    pub fn from_root(root: PathBuf) -> Self {
        let subpath = |arg: &str| root.join(arg);
        Self {
            root: root.clone(),
            run: subpath("run"),
            sources: subpath("sources"),
            playlists: subpath("playlists"),
            cache: subpath("cache"),
        }
    }
}

pub struct Resolver {
    s: State,
    d: Directories,
    o: Output,
}

impl Resolver {
    pub fn new(path: PathBuf) -> Self {
        Self {
            s: State { resolved: false },
            d: Directories::from_root(path),
            o: Output::default(),
        }
    }

    pub fn create_dirs(&mut self) -> Result<()> {
        if !self.d.run.try_exists()? {
            fs::create_dir(&self.d.run)?
        }
        if !self.d.playlists.try_exists()? {
            fs::create_dir(&self.d.sources)?
        }
        if !self.d.playlists.try_exists()? {
            fs::create_dir(&self.d.playlists)?
        }
        if !self.d.cache.try_exists()? {
            fs::create_dir(&self.d.cache)?
        }
        Ok(())
    }

    pub fn tmp_file(&mut self, name: impl AsRef<Path>) -> PathBuf {
        self.d.run.join(name)
    }

    pub fn out(&self) -> &Output {
        assert!(self.s.resolved, "Resolver has not yet been run!");
        &self.o
    }

    pub fn dirs(&self) -> &Directories {
        &self.d
    }

    pub fn resolve(&mut self) -> Result<()> {
        self.o = Output::default();

        self.o.config = Config::new(self.d.root.clone())?;

        {
            for src_file in fs::read_dir(&self.d.sources)?.filter_map(Result::ok) {
                if src_file.file_type()?.is_file() {
                    let read = fs::read_to_string(src_file.path())?;
                    let decode = ron::from_str::<schema::Source>(&read)?;
                    self.o.sources.push(decode);
                }
            }
        }

        {
            for src_file in fs::read_dir(&self.d.playlists)?.filter_map(Result::ok) {
                if src_file.file_type()?.is_file() {
                    let read = fs::read_to_string(src_file.path())?;
                    let mut pl = ron::from_str::<schema::Playlist>(&read)?;
                    pl.resolved_sources = Some(pl.sources.clone());
                    pl.file_path = src_file.path();
                    for schema::Import::Source(source) in &pl.import {
                        let source = self
                            .o
                            .sources
                            .iter()
                            .find(|src| &src.name == source)
                            .ok_or(anyhow!("Failed to find source {source}"))?;
                        let res = pl.resolved_sources.as_mut().unwrap();
                        res.push(source.clone());
                    }
                    self.o.playlists.push(pl);
                }
            }
        }

        {
            self.o.cache = CacheDir::new(self.d.cache.clone());
        }

        self.s.resolved = true;
        Ok(())
    }
}
