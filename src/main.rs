#[macro_use]
extern crate tracing;

use std::{
    env,
    fmt::Write,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicI8, AtomicUsize},
        Arc,
    },
};

use clap::{Parser, Subcommand};
use color_eyre::eyre::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use heck::ToSnakeCase;
use notify_rust::Notification;
use symphonia::core::{io::MediaSourceStream, probe::Hint};
use tokio::{
    io::{self, AsyncBufReadExt},
    spawn,
};
use uuid::Uuid;

use crate::schema::DlPlaylist;

mod cfg;
mod log;
mod panic;
mod player;
mod player2;
mod project_meta;
mod schema;
mod ui;
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
    UI {},
}

#[tokio::main]
async fn main() -> Result<()> {
    log::initialize_logging()?;
    panic::initialize_panic_handler()?;
    let args = Args::parse();
    let mut app = ui::app::App::new(1.0, 24.0)?;
    match args.cmd {
        Command::Download { file } => download(file)?,
        Command::Play { playlist } => play(playlist).await?,
        Command::UI {} => app.run().await?,
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

    let mut selected_track = 0;
    loop {
        let track = dl_pl.tracks.get(selected_track).unwrap();
        info!(
            "Playing [{track}/{num_tracks}]: {name} by {artist}",
            track = selected_track + 1,
            num_tracks = dl_pl.tracks.len(),
            name = track.track.meta.name,
            artist = track.track.meta.artist,
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

        // let mut player = player::DecodeAndPlay::open(&device, &config, mss, hint);
        // let queue = player.get_command_queue();
        // let exit_flag = Arc::new(AtomicBool::new(false));
        // let exit_flag2 = exit_flag.clone();
        // let order_flag = Arc::new(AtomicI8::new(1));
        // let order_flag2 = order_flag.clone();
        // let track_select = Arc::new(AtomicUsize::new(usize::MAX));
        // let track_select2 = track_select.clone();
        // let tracks_listing = dl_pl.tracks.clone();
        // let cmd_task = spawn(async move {
        //     let mut input = io::BufReader::new(io::stdin()).lines();
        //     while let Some(line) = input.next_line().await? {
        //         match line.as_str() {
        //             "pause" => {
        //                 queue.send(player::Command::Pause)?;
        //                 info!("[command] pause")
        //             }
        //             "play" => {
        //                 queue.send(player::Command::Play)?;
        //                 info!("[command] play");
        //             }
        //             "stop" | "quit" => {
        //                 exit_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
        //                 queue.send(player::Command::Stop)?;
        //                 info!("[command] stop|quit");
        //                 break;
        //             }
        //             "next" | "ff" => {
        //                 order_flag2.store(2, std::sync::atomic::Ordering::Relaxed);
        //                 queue.send(player::Command::Stop)?;
        //                 info!("[command] next|ff");
        //                 break;
        //             }
        //             "prev" | "fr" => {
        //                 order_flag2.store(-1, std::sync::atomic::Ordering::Relaxed);
        //                 queue.send(player::Command::Stop)?;
        //                 info!("[command] prev|fr");
        //                 break;
        //             }
        //             "repeat" | "re" => {
        //                 order_flag2.store(0, std::sync::atomic::Ordering::Relaxed);
        //                 queue.send(player::Command::Stop)?;
        //                 info!("[command] repeat|re");
        //                 break;
        //             }
        //             "list" | "ls" => {
        //                 let mut output = String::new();
        //                 for (i, track) in tracks_listing.iter().enumerate() {
        //                     let _ = writeln!(
        //                         &mut output,
        //                         " - {num}\t: {name} by {artist}",
        //                         num = i + 1,
        //                         name = track.track.meta.name,
        //                         artist = track.track.meta.artist
        //                     );
        //                 }
        //                 info!("-- track listing --\n{output}");
        //             }
        //             "select" | "sel" => {
        //                 info!("[command/select] enter track to play:");
        //                 if let Some(track) = input.next_line().await? {
        //                     if let Ok(val) = track.trim().parse::<usize>() {
        //                         if val <= tracks_listing.len() {
        //                             info!("[command/select] jump to track {val}");
        //                             track_select2
        //                                 .store(val - 1, std::sync::atomic::Ordering::Relaxed);
        //                             queue.send(player::Command::Stop)?;
        //                             break;
        //                         } else {
        //                             warn!("Track {val} is out of range");
        //                         }
        //                     } else {
        //                         warn!("Invalid track {track:?}");
        //                     }
        //                 } else {
        //                     break;
        //                 }
        //             }
        //             "search" | "sea" => {
        //                 info!("[command/search] enter (partial) name of track:");
        //                 if let Some(name) = input.next_line().await? {
        //                     let matcher = SkimMatcherV2::default().ignore_case();
        //                     let mut scores = vec![];
        //                     for (i, track) in tracks_listing.iter().enumerate() {
        //                         if let Some(score) =
        //                             matcher.fuzzy_match(&track.track.meta.name, &name)
        //                         {
        //                             scores.push((score, i));
        //                         }
        //                     }
        //                     if scores.is_empty() {
        //                         warn!("[command/search] no results");
        //                     } else {
        //                         scores.sort_by_key(|score| score.0);
        //                         let mut output = String::new();
        //                         for (i, (_score, track)) in scores.into_iter().enumerate() {
        //                             let track = tracks_listing.get(track).unwrap();
        //                             let _ = writeln!(
        //                                 &mut output,
        //                                 " - {}\t: {} by {}",
        //                                 i + 1,
        //                                 track.track.meta.name,
        //                                 track.track.meta.artist,
        //                             );
        //                         }
        //                         info!("--- search ---\n{output}");
        //                     }
        //                 } else {
        //                     break;
        //                 }
        //             }
        //             "help" | "h" => {
        //                 info!(
        //                     "-- help --\n\t\
        //                     commands:\n\t\
        //                      - pause\n\t\
        //                      - play\n\t\
        //                      - stop   | quit : exit DMM\n\t\
        //                      - next   | ff   : go to the next track in the playlist\n\t\
        //                      - prev   | fr   : go to the previous track in the playlist\n\t\
        //                      - repeat | re   : repeat the current track\n\t\
        //                      - list   | ls   : list all tracks in the current playlist\n\t\
        //                      - select | sel  : select a track by its number\n\t\
        //                      - search | sea  : search the playlist for a track"
        //                 );
        //             }
        //             _ => warn!("Unknown command {line:?}"),
        //         }
        //     }
        //     Ok::<(), color_eyre::eyre::Error>(())
        // });
        // player.run().await?;
        // cmd_task.abort();
        // if exit_flag.load(std::sync::atomic::Ordering::Relaxed) {
        //     info!("[command] exiting");
        //     break;
        // }
        // match order_flag.load(std::sync::atomic::Ordering::Relaxed) {
        //     -1 => {
        //         if selected_track == 0 {
        //             warn!("[command/prev] no track before this one exists -> repeating");
        //         } else {
        //             selected_track -= 1;
        //         }
        //     }
        //     0 => {
        //         continue;
        //     }
        //     v @ (1 | 2) => {
        //         if selected_track == dl_pl.tracks.len() - 1 {
        //             if v != 1 {
        //                 error!("[command/next] no track after this one -> repeating");
        //             } else {
        //                 info!("end of playlist, exiting");
        //                 break;
        //             }
        //         } else {
        //             selected_track += 1;
        //         }
        //     }
        //     _ => unreachable!(),
        // }
        // match track_select.load(std::sync::atomic::Ordering::Relaxed) {
        //     usize::MAX => {}
        //     selected => {
        //         selected_track = selected;
        //     }
        // }
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
