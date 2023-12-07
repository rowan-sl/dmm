#[macro_use]
extern crate tracing;

use std::{env, fs, path::PathBuf};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{anyhow, Result};
use heck::ToSnakeCase;
use uuid::Uuid;

use crate::schema::DlPlaylist;

mod cfg;
mod log;
mod panic;
mod player2;
mod project_meta;
mod schema;
mod ui;
mod waker;

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
        /// Playlist file to read
        #[arg()]
        file: PathBuf,
    },
    /// Play the given playlist
    Player {
        /// playlist to play (directory)
        #[arg()]
        playlist: PathBuf,
    },
    /// Print version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    log::initialize_logging()?;
    panic::initialize_panic_handler()?;
    let args = Args::parse();
    match args.cmd {
        Command::Download { file } => download(file)?,
        Command::Player { playlist } => {
            let mut app = ui::app::App::new(20.0, 30.0, playlist)?;
            app.run().await?;
        }
        Command::Version => {
            println!("{}", project_meta::version());
        }
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
