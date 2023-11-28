#[macro_use]
extern crate log;

use std::{env, fs, path::PathBuf};

use anyhow::{anyhow, bail, Result};
use clap::{Parser, Subcommand};
use cpal::traits::HostTrait;
use heck::ToSnakeCase;
use notify_rust::Notification;
use rodio::DeviceTrait;
use symphonia::core::{
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    errors::Error as AudioError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use uuid::Uuid;

use crate::schema::DlPlaylist;

mod schema;

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
    pretty_env_logger::formatted_builder()
        .filter_level(log::LevelFilter::Debug)
        .try_init()?;
    match args.cmd {
        Command::Download { file } => download(file)?,
        Command::Play { playlist } => play(playlist)?,
    }
    Ok(())
}

mod output {
    use anyhow::{bail, Result};
    use rb::{RbConsumer, RbProducer, SpscRb, RB};
    use symphonia::core::audio::{AudioBufferRef, RawSample, SampleBuffer, SignalSpec};
    use symphonia::core::conv::{ConvertibleSample, IntoSample};
    use symphonia::core::units::Duration;

    use cpal::traits::{DeviceTrait, StreamTrait};

    pub trait AudioOutput {
        fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()>;
        fn flush(&mut self);
    }

    pub trait AudioOutputSample:
        cpal::Sample
        + cpal::SizedSample
        + ConvertibleSample
        + IntoSample<f32>
        + RawSample
        + std::marker::Send
        + 'static
    {
    }

    impl AudioOutputSample for f32 {}
    impl AudioOutputSample for i16 {}
    impl AudioOutputSample for u16 {}

    pub struct CpalAudioOutputImpl<T: AudioOutputSample>
    where
        T: AudioOutputSample,
    {
        ring_buf_producer: rb::Producer<T>,
        sample_buf: SampleBuffer<T>,
        stream: cpal::Stream,
    }

    impl<T: AudioOutputSample> CpalAudioOutputImpl<T> {
        pub fn try_open(
            spec: SignalSpec,
            duration: Duration,
            device: &cpal::Device,
        ) -> Result<Box<dyn AudioOutput>> {
            let num_channels = spec.channels.count();

            // Output audio stream config.
            let config = cpal::StreamConfig {
                channels: num_channels as cpal::ChannelCount,
                sample_rate: cpal::SampleRate(spec.rate),
                buffer_size: cpal::BufferSize::Default,
            };

            // Create a ring buffer with a capacity for up-to 200ms of audio.
            let ring_len = ((200 * config.sample_rate.0 as usize) / 1000) * num_channels;

            let ring_buf = SpscRb::new(ring_len);
            let (ring_buf_producer, ring_buf_consumer) = (ring_buf.producer(), ring_buf.consumer());

            let stream_result = device.build_output_stream(
                &config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    // Write out as many samples as possible from the ring buffer to the audio
                    // output.
                    let written = ring_buf_consumer.read(data).unwrap_or(0);

                    // Mute any remaining samples.
                    data[written..].iter_mut().for_each(|s| *s = T::MID);
                },
                move |err| error!("audio output error: {}", err),
                None,
            );

            if let Err(err) = stream_result {
                error!("audio output stream open error: {}", err);
                bail!("failed to initialize audio backend");
            }

            let stream = stream_result.unwrap();

            // Start the output stream.
            if let Err(err) = stream.play() {
                error!("audio output stream play error: {}", err);
                bail!("failed to initialize audio backend");
            }

            let sample_buf = SampleBuffer::<T>::new(duration, spec);

            Ok(Box::new(CpalAudioOutputImpl {
                ring_buf_producer,
                sample_buf,
                stream,
            }))
        }
    }

    impl<T: AudioOutputSample> AudioOutput for CpalAudioOutputImpl<T> {
        fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()> {
            // Do nothing if there are no audio frames.
            if decoded.frames() == 0 {
                return Ok(());
            }

            // Resampling is not required. Interleave the sample for cpal using a sample buffer.
            self.sample_buf.copy_interleaved_ref(decoded);
            let mut samples = self.sample_buf.samples();

            // Write all samples to the ring buffer.
            while let Some(written) = self.ring_buf_producer.write_blocking(samples) {
                samples = &samples[written..];
            }

            Ok(())
        }

        fn flush(&mut self) {
            // Flush is best-effort, ignore the returned result.
            let _ = self.stream.pause();
        }
    }
}

fn play(pl_dir: PathBuf) -> Result<()> {
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
    let mut audio_output = None;

    for track in &dl_pl.tracks {
        info!(
            "Now Playing: {name} by {artist}",
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
        hint.with_extension("mp3");

        // Use the default options for metadata and format readers.
        let meta_opts: MetadataOptions = Default::default();
        let fmt_opts: FormatOptions = Default::default();

        // Probe the media source.
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .expect("unsupported format");

        // Get the instantiated format reader.
        let mut format = probed.format;

        // Find the first audio track with a known (decodeable) codec.
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .expect("no supported audio tracks");

        // Use the default options for the decoder.
        let dec_opts: DecoderOptions = Default::default();

        // Create a decoder for the track.
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &dec_opts)
            .expect("unsupported codec");

        // Store the track identifier, it will be used to filter packets.
        let track_id = track.id;

        fn ignore_end_of_stream_error(
            result: symphonia::core::errors::Result<()>,
        ) -> symphonia::core::errors::Result<()> {
            match result {
                Err(AudioError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof
                        && err.to_string() == "end of stream" =>
                {
                    // Do not treat "end of stream" as a fatal error. It's the currently only way a
                    // format reader can indicate the media is complete.
                    Ok(())
                }
                _ => result,
            }
        }

        // The decode loop.
        loop {
            // Get the next packet from the media format.
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(AudioError::ResetRequired) => {
                    // The track list has been changed. Re-examine it and create a new set of decoders,
                    // then restart the decode loop. This is an advanced feature and it is not
                    // unreasonable to consider this "the end." As of v0.5.0, the only usage of this is
                    // for chained OGG physical streams.
                    break;
                }
                Err(err) => {
                    // A unrecoverable error occured, halt decoding.
                    ignore_end_of_stream_error(Err(err))?;
                    // end of stream
                    break;
                }
            };

            // Consume any new metadata that has been read since the last packet.
            while !format.metadata().is_latest() {
                // Pop the old head of the metadata queue.
                format.metadata().pop();

                // Consume the new metadata at the head of the metadata queue.
            }

            // If the packet does not belong to the selected track, skip over it.
            if packet.track_id() != track_id {
                continue;
            }

            // Decode the packet into audio samples.
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    // If the audio output is not open, try to open it.
                    if audio_output.is_none() {
                        // Get the audio buffer specification. This is a description of the decoded
                        // audio buffer's sample format and sample rate.
                        let spec = *decoded.spec();

                        // Get the capacity of the decoded buffer. Note that this is capacity, not
                        // length! The capacity of the decoded buffer is constant for the life of the
                        // decoder, but the length is not.
                        let duration = decoded.capacity() as u64;

                        // Try to open the audio output.
                        // Select proper playback routine based on sample format.
                        let output = match config.sample_format() {
                            cpal::SampleFormat::F32 => {
                                output::CpalAudioOutputImpl::<f32>::try_open(
                                    spec, duration, &device,
                                )?
                            }
                            cpal::SampleFormat::I16 => {
                                output::CpalAudioOutputImpl::<i16>::try_open(
                                    spec, duration, &device,
                                )?
                            }
                            cpal::SampleFormat::U16 => {
                                output::CpalAudioOutputImpl::<u16>::try_open(
                                    spec, duration, &device,
                                )?
                            }
                            sample_format => {
                                error!("Unsupported sample format '{sample_format}'");
                                bail!("Failed to initialize audio backend");
                            }
                        };
                        audio_output.replace(output);
                    } else {
                        // TODO: Check the audio spec. and duration hasn't changed.
                    }

                    if let Some(audio_output) = audio_output.as_mut() {
                        audio_output.write(decoded).unwrap()
                    }
                }
                Err(AudioError::IoError(_)) => {
                    // The packet failed to decode due to an IO error, skip the packet.
                    warn!("I/O Error during decoding [will attempt to continue]");
                    continue;
                }
                Err(AudioError::DecodeError(err)) => {
                    // The packet failed to decode due to invalid data, skip the packet.
                    warn!("Decoder error: {err} [will attempt to continue]");
                    continue;
                }
                Err(err) => {
                    // An unrecoverable error occured, halt decoding.
                    Err(err)?
                }
            }
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
