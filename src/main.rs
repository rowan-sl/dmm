#[macro_use]
extern crate tracing;

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, BufRead},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{anyhow, bail, Result};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use resolver::Resolver;
use schema::Playlist;

mod cache;
mod cfg;
mod log;
mod panic;
mod player2;
mod project_meta;
mod resolver;
mod schema;
mod ui;

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
#[command(alias = "dl")]
enum Download {
    /// download the given playlist
    #[command(alias = "pl")]
    Playlist {
        /// playlist to download
        playlist: String,
    },
    /// download all playlists
    All,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Download playlists
    Download {
        /// directory to "run in"
        #[arg(long = "in")]
        run_in: Option<PathBuf>,
        #[command(subcommand)]
        cmd: Download,
    },
    /// Play the given playlist
    Player {
        /// playlist to play (Name)
        #[arg()]
        playlist: String,
        /// directory to "run in"
        #[arg(long = "in")]
        run_in: Option<PathBuf>,
    },
    /// Print version information
    Version,
    /// Management of DMM's download store
    #[command(subcommand)]
    Store(Store),
}

/// Management of DMM's download store
#[derive(Subcommand, Debug)]
enum Store {
    /// Garbage collect store
    ///
    /// deletes any downloaded files that are no longer referenced
    /// by any playlists
    GC {
        /// directory to "run in"
        #[arg(long = "in")]
        run_in: Option<PathBuf>,
        /// find, but do not remove, unreferenced files
        #[arg(long)]
        dry_run: bool,
    },
    /// Extract a downloaded file from the store - use this if a download link/primary source disapears
    ///
    /// This is playlist-independant - only the source and input must be the same
    Extract {
        /// name of the source that this was downloaded from originally
        source: String,
        /// input to that source [string only]
        input: String,
        /// path to copy the file to (if found)
        /// the extension of this file will be automatically set
        #[arg(long, short)]
        copy_to: Option<PathBuf>,
        /// directory to "run in"
        #[arg(long = "in")]
        run_in: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    panic::initialize_panic_handler()?;
    let args = Args::parse();
    match args.cmd {
        Command::Download {
            run_in,
            cmd: Download::Playlist { playlist },
        } => {
            log::initialize_logging(None)?;
            download(run_in, Some(playlist))?;
        }
        Command::Download {
            run_in,
            cmd: Download::All,
        } => {
            log::initialize_logging(None)?;
            download(run_in, None)?;
        }
        Command::Player { playlist, run_in } => {
            let mut res = Resolver::new(resolve_run_path(run_in)?);
            res.create_dirs()?;
            log::initialize_logging(Some(res.tmp_file("dmm.log")))?;
            res.resolve()?;
            let chosen: Playlist = {
                let mut scores = vec![];
                let matcher = SkimMatcherV2::default().ignore_case();
                for (i, j) in res.out().playlists.iter().enumerate() {
                    if let Some(score) = matcher.fuzzy_match(&j.name, &playlist) {
                        scores.push((score, i));
                    }
                }
                if scores.is_empty() {
                    println!(
                        "Failed to find matching playlist in input (searched for name: {playlist:?})"
                    );
                    return Ok(());
                } else {
                    scores.sort_by_key(|score| score.0);
                    let chosen = &res.out().playlists[scores[0].1];
                    chosen.clone()
                }
            };
            let mut app = ui::app::App::new(res, 15.0, chosen)?;
            app.run()?;
        }
        Command::Version => {
            println!("{}", project_meta::version());
        }
        Command::Store(Store::GC { run_in, dry_run }) => {
            log::initialize_logging(None)?;
            gc(run_in, dry_run)?;
        }
        Command::Store(Store::Extract {
            source,
            input,
            copy_to,
            run_in,
        }) => {
            let mut res = Resolver::new(resolve_run_path(run_in)?);
            res.create_dirs()?;
            log::initialize_logging(None)?;
            res.resolve()?;
            let Some(source) = res.out().sources.iter().find(|s| s.name == source) else {
                error!("Could not find the source named {source:?}");
                bail!("query failed");
            };
            let hash = cache::Hash::generate(source, &ron::Value::String(input));
            let Some(found) = res.out().cache.find(hash) else {
                info!("Calculated hash is {}", hash.to_string());
                error!("Could not find the requested download in the store");
                bail!("query failed");
            };
            info!("File path is {found:?} (file format: '{}')", source.format);
            if let Some(path) = copy_to {
                let path = path.with_extension(&source.format);
                info!("Copying file to {path:?}");
                std::fs::copy(found, path)?;
            }
        }
    }
    Ok(())
}

/// selects the path to run in, in this order
/// - `--in` argument
/// - path specified in .dmm-link.ron
/// - current directory
fn resolve_run_path(run_in: Option<PathBuf>) -> Result<PathBuf> {
    run_in.map(Ok).unwrap_or_else(|| {
        let cdir = env::current_dir()?;
        let path = cdir.join(".dmm-link.ron");
        Ok(if path.try_exists()? {
            let content = fs::read_to_string(path)?;
            let link = ron::from_str::<schema::Link>(&content)?;
            link.music_directory
        } else {
            if !cdir.join("dmm.ron").try_exists()? {
                bail!("Cannot locate music directory (it is not the current directory, and no .dmm-link.ron exists)");
            }
            cdir
        })
    })
}

fn download(run_in: Option<PathBuf>, name: Option<String>) -> Result<()> {
    let mut res = Resolver::new(resolve_run_path(run_in)?);
    res.create_dirs()?;
    res.resolve()?;
    if let Some(name) = name {
        let mut scores = vec![];
        let matcher = SkimMatcherV2::default().ignore_case();
        for (i, playlist) in res.out().playlists.iter().enumerate() {
            if let Some(score) = matcher.fuzzy_match(&playlist.name, &name) {
                scores.push((score, i));
            }
        }
        if scores.is_empty() {
            error!("Failed to find matching playlist in input (searched for name: {name:?})");
            return Ok(());
        } else {
            scores.sort_by_key(|score| score.0);
            let chosen = &res.out().playlists[scores[0].1];
            info!(
                "search returned playlist {:?} : {:?}",
                chosen.name, chosen.file_path
            );
            println!("is this correct (cont/abort)? [y/N]:");
            let Some(next) = io::stdin().lock().lines().next() else {
                bail!("Failed to get input");
            };
            match next?.as_str() {
                "y" | "Y" => {}
                _ => {
                    info!("Aborting");
                    return Ok(());
                }
            }
            let src = chosen.clone();
            download_playlist(src, &res.out().cache)?;
        }
    } else {
        for playlist in res.out().playlists.iter() {
            info!("Downloading playlist {}", playlist.name);
            download_playlist(playlist.clone(), &res.out().cache)?;
        }
    }
    Ok(())
}

fn download_playlist(playlist: schema::Playlist, cache: &cache::CacheDir) -> Result<()> {
    info!("downloading tracks in playlist {} to cache", playlist.name);
    for track in &playlist.tracks {
        info!("downloading {}", track.meta.name);
        let source = playlist.find_source(&track.src).ok_or(anyhow!(
            "Could not find source {} for track {}",
            track.src,
            track.meta.name
        ))?;
        let hash = cache::Hash::generate(source, &track.input);
        if cache.find(hash).is_some() {
            info!("track exists in cache [skiping]");
            continue;
        }
        let path = cache.create(hash);
        source.execute(track.input.clone(), &path)?;
        debug!("download complete");
    }
    info!("Done!");
    Ok(())
}

fn gc(run_in: Option<PathBuf>, dry_run: bool) -> Result<()> {
    let mut res = Resolver::new(resolve_run_path(run_in)?);
    res.create_dirs()?;
    res.resolve()?;
    let mut hashes = HashSet::new();
    let mut source_map = HashMap::new();
    for playlist in &res.out().playlists {
        for source in playlist.resolved_sources.as_ref().unwrap() {
            source_map.insert(source.name.clone(), source.clone());
        }
        for track in &playlist.tracks {
            let source = source_map
                .get(&track.src)
                .expect("Cannot find source for track");
            let hash = cache::Hash::generate(source, &track.input);
            hashes.insert(hash);
        }
    }
    let mut bytes_removed = 0u64;
    let mut files_removed = 0usize;
    for entry in res.dirs().cache.read_dir()? {
        let entry = entry?;
        let hash = entry
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .expect("path not utf-8")
            .parse::<cache::Hash>()?;
        if !hashes.contains(&hash) {
            info!("deleting {}", hash.to_string());
            bytes_removed += entry.metadata()?.len();
            files_removed += 1;
            if !dry_run {
                fs::remove_file(entry.path())?;
            }
        }
    }
    info!("removed {files_removed} entries, freed {bytes_removed} bytes");
    Ok(())
}
