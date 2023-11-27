use std::{env, path::PathBuf};

use anyhow::{anyhow, Result};
use clap::Parser;

pub mod schema;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Playlist file to read
    #[arg()]
    playlist: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let content = std::fs::read_to_string(args.playlist)?;
    let playlist = ron::from_str::<schema::Playlist>(&content)?;
    println!("{playlist:#?}");
    let track = &playlist.tracks()[0];
    let source = playlist
        .find_source(&track.src)
        .ok_or(anyhow!("Could not find source for track"))?;
    let path = env::current_dir()?.join("first_track");
    let out = source.execute(track.input.clone(), path)?;
    println!("Downloaded {} to {out:?}", track.meta.name);
    Ok(())
}
