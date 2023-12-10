#[macro_use]
extern crate tracing;

use std::{
    env, fs,
    io::{self, BufRead},
    path::PathBuf,
};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{anyhow, bail, Result};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use heck::ToSnakeCase;
use resolver::Resolver;
use uuid::Uuid;

use crate::schema::DlPlaylist;

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
enum Command {
    /// Download playlists
    Download {
        /// directory to "run in"
        #[arg(long = "in")]
        run_in: Option<PathBuf>,
        /// playlist (name) to download
        playlist: String,
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
}

fn main() -> Result<()> {
    panic::initialize_panic_handler()?;
    let args = Args::parse();
    match args.cmd {
        Command::Download { run_in, playlist } => {
            log::initialize_logging(None)?;
            download(run_in, playlist)?;
        }
        Command::Player { playlist, run_in } => {
            let mut res = Resolver::new(resolve_run_path(run_in)?);
            res.create_dirs()?;
            log::initialize_logging(Some(res.tmp_file("dmm.log")))?;
            res.resolve()?;
            let config = res.out().config.clone();
            let chosen = {
                let mut scores = vec![];
                let matcher = SkimMatcherV2::default().ignore_case();
                for (i, j) in res.out().cache.playlists.iter().enumerate() {
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
                    let chosen = &res.out().cache.playlists[scores[0].1];
                    chosen
                }
            };
            let mut app = ui::app::App::new(config, 30.0, chosen.clone())?;
            app.run()?;
        }
        Command::Version => {
            println!("{}", project_meta::version());
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
            cdir
        })
    })
}

fn download(run_in: Option<PathBuf>, name: String) -> Result<()> {
    let mut res = Resolver::new(resolve_run_path(run_in)?);
    res.create_dirs()?;
    res.resolve()?;
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
        info!("Downloading...");
        let src = chosen.clone();
        let dest = res.dirs().cache.clone();
        download_playlist(src, dest)?;
    }
    Ok(())
}

/// src: <playlist>.ron file (in playlists/)
/// dest: (cache/) directory (a new subdir will be created for this playlist)
fn download_playlist(playlist: schema::Playlist, dest: PathBuf) -> Result<()> {
    let out_dir_name = playlist.name.to_snake_case();
    let out_dir = dest.join(out_dir_name);
    println!("Downloading playlist {} to {:?}", playlist.name, out_dir);
    if out_dir.try_exists()? {
        println!("Playlist already exists, checking for changes");
        let dl_playlist_str = fs::read_to_string(out_dir.join("index.ron"))?;
        let dl_playlist = ron::from_str::<schema::DlPlaylist>(&dl_playlist_str)?;
        let diff = dl_playlist.gen_diff(&playlist);
        diff.display();
    } else {
        fs::create_dir(&out_dir)?;
        let mut dl_playlist = DlPlaylist {
            directory: Default::default(),
            name: playlist.name.clone(),
            sources: playlist.resolved_sources.clone().unwrap(),
            tracks: vec![],
        };
        for track in &playlist.tracks {
            println!("Downloading {}", track.meta.name);
            let source = playlist.find_source(&track.src).ok_or(anyhow!(
                "Could not find source {} for track {}",
                track.src,
                track.meta.name
            ))?;
            let uuid = Uuid::new_v4();
            let path = out_dir.join(uuid.to_string());
            source.execute(track.input.clone(), &path)?;
            println!("Download complete");
            dl_playlist.tracks.push(schema::DlTrack {
                track: track.clone(),
                track_id: uuid,
            });
        }
        let dl_playlist_str = ron::ser::to_string_pretty(
            &dl_playlist,
            ron::ser::PrettyConfig::new().struct_names(true),
        )?;
        fs::write(out_dir.join("index.ron"), dl_playlist_str.as_bytes())?;
        println!("Downloading playlist complete");
    }

    Ok(())
}
