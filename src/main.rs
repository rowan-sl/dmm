#[macro_use]
extern crate log;

use std::{env, fs, path::PathBuf};

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait};
use heck::ToSnakeCase;
use notify_rust::Notification;
use symphonia::core::{io::MediaSourceStream, probe::Hint};
use uuid::Uuid;

use crate::schema::DlPlaylist;

mod output;
mod player;
mod schema;
mod waker;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Download playlists
    Download {
        /// Playlist file to read
        #[arg()]
        file: PathBuf,
    },
    /// Play the given playlist (sequentially)
    Play {
        /// Playlist to play
        #[arg()]
        playlist: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Trace)
        .filter_module("symphonia_core::probe", log::LevelFilter::Warn)
        .filter_module("async_io", log::LevelFilter::Warn)
        .filter_module("polling", log::LevelFilter::Warn)
        .try_init()?;
    match args.cmd {
        Command::Download { file } => download(file)?,
        Command::Play { playlist } => play(playlist).await?,
    }
    Ok(())
}

async fn play(pl_dir: PathBuf) -> Result<()> {
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

    for track in &dl_pl.tracks {
        info!(
            "Playing: {name} by {artist}",
            name = track.track.meta.name,
            artist = track.track.meta.artist
        );
        let _handle = Notification::new()
            .summary("DMM [play]")
            .body(&format!(
                "Now Playing: {name}\nby {artist}",
                name = track.track.meta.name,
                artist = track.track.meta.artist
            ))
            .show()?;
        let track_path = pl_dir
            .read_dir()?
            .find(|res| {
                res.as_ref().is_ok_and(|entry| {
                    entry
                        .path()
                        .file_stem()
                        .is_some_and(|name| name.to_string_lossy() == track.track_id.to_string())
                })
            })
            .ok_or(anyhow!("BUG: could not file file for downloaded track"))?
            .unwrap()
            .path();
        debug!("loading audio...");
        // Open the media source.
        let track_src = std::fs::File::open(&track_path).expect("failed to open media");

        // Create the media source stream.
        let mss = MediaSourceStream::new(Box::new(track_src), Default::default());

        // Create a probe hint using the file's extension. [Optional]
        let mut hint = Hint::new();
        hint.with_extension(&dl_pl.find_source(&track.track.src).unwrap().format);

        let mut player = player::DecodeAndPlay::open(&device, &config, mss, hint);
        player.run().await?;
    }
    Ok(())
}

fn download(file: PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let playlist = ron::from_str::<schema::Playlist>(&content)?;
    let out_dir_name = playlist.name.to_snake_case();
    let out_dir = env::current_dir()?.join(out_dir_name);
    println!("Downloading playlist {} to {:?}", playlist.name, out_dir);
    if out_dir.try_exists()? {
        println!("Playlist already exists, checking for changes");
        let dl_playlist_str = fs::read_to_string(out_dir.join("dl_playlist.ron"))?;
        let dl_playlist = ron::from_str::<schema::DlPlaylist>(&dl_playlist_str)?;
        let diff = dl_playlist.gen_diff(&playlist);
        diff.display();
    } else {
        fs::create_dir(&out_dir)?;
        let mut dl_playlist = DlPlaylist {
            name: playlist.name.clone(),
            sources: playlist.sources.clone(),
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
        fs::write(out_dir.join("dl_playlist.ron"), dl_playlist_str.as_bytes())?;
        println!("Downloading playlist complete");
    }

    Ok(())
}
