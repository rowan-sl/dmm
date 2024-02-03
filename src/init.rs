//! Handling of `dmm init`

use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::Path,
};

use color_eyre::eyre::{bail, Result};

const GITIGNORE: &str = include_str!("../assets/gitignore");
const DMM_DOT_RON: &str = include_str!("../assets/dmm.minimal.ron");
const YT_DLP: &str = include_str!("../examples/sources/yt-dlp.ron");
const EX_PLAYLIST: &str = include_str!("../assets/example-playlist.ron");

fn write_file(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(content.as_ref())?;
    Ok(())
}

pub fn dmm_init() -> Result<()> {
    crate::log::initialize_logging(None)?;
    info!(
        "Initializing a music directory. This will create the following folder structure\n\
                This includes creating an example playlist, and source to download from youtube.\n\
                IT IS RECOMMENDED TO DO THIS IN AN EMPTY DIRECTORY\n\n\
                \t. (you are here)\n\
                \t├─ .gitignore\n\
                \t├─ dmm.ron\n\
                \t├─ sources\n\
                \t│  └─ yt-dlp.ron\n\
                \t├─ playlists\n\
                \t│  └─ example.ron\n\
                \t├─ cache\n\
                \t│  └─ <content omitted>\n\
                \t└─ run\n\
                \t   └─ dmm.log\n\
            "
    );
    println!("do you want to continue? [y/N]:");
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
    write_file("./.gitignore", GITIGNORE)?;
    write_file("./dmm.ron", DMM_DOT_RON)?;
    fs::create_dir("sources")?;
    write_file("./sources/yt-dlp.ron", YT_DLP)?;
    fs::create_dir("playlists")?;
    write_file("./playlists/example.ron", EX_PLAYLIST)?;
    fs::create_dir("cache")?;
    fs::create_dir("run")?;
    write_file("./run/dmm.log", "DMM's Log File")?;

    info!("Created the directory structure");
    info!("Download the playlist with `dmm download pl 'example'`, and play it with `dmm player 'example'`");
    info!("For more information, check out the git page at <https://git.fawkes.io/mtnash/dmm>");
    warn!("Enjoy!");

    Ok(())
}
