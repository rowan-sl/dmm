use std::{env, fs, io::BufReader, path::PathBuf, sync::Arc};

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use heck::ToSnakeCase;
use rodio::{OutputStream, Sink};
use uuid::Uuid;

use crate::schema::DlPlaylist;

pub mod schema;

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

fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Command::Download { file } => download(file)?,
        Command::Play { playlist } => play(playlist)?,
    }
    Ok(())
}

fn play(pl_dir: PathBuf) -> Result<()> {
    println!("Loading playlist {pl_dir:?}");
    if !pl_dir.try_exists()? {
        bail!("Failed to load: playlist does not exist (no such directory)");
    }
    if !pl_dir.join("dl_playlist.ron").try_exists()? {
        bail!("Failed to load: playlist does not exist (no manifest `dl_playlist.ron` file in given directory)");
    }
    let dl_pl_str = fs::read_to_string(pl_dir.join("dl_playlist.ron"))?;
    let dl_pl = ron::from_str::<schema::DlPlaylist>(&dl_pl_str)?;
    println!("Loaded playlist {name}", name = dl_pl.name);
    // let track = dl_pl
    //     .tracks
    //     .get(0)
    //     .ok_or(anyhow!("Playlist contains no music :("))?;
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;
    for track in &dl_pl.tracks {
        sink.pause();
        println!(
            "Now Playing: {name} by {artist}",
            name = track.track.meta.name,
            artist = track.track.meta.artist
        );
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
        println!("loading audio...");
        let track_reader = BufReader::new(fs::File::open(track_path)?);
        let track_src = rodio::Decoder::new(track_reader)?;
        sink.append(track_src);
        println!("playing...");
        sink.play();
        sink.sleep_until_end();
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
