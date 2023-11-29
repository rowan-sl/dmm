use anyhow::{bail, Result};
use cpal::{Device, SupportedStreamConfig};
use flume::{Receiver, Sender};
use symphonia::core::{
    codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
    errors::Error as AudioError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

use crate::output::{self, AudioOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    Pause,
    Play,
    Stop,
}

pub struct DecodeAndPlay<'a> {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    device: &'a Device,
    config: &'a SupportedStreamConfig,
    command_queue: Receiver<Command>,
    command_queue_sender: Sender<Command>,
}

impl<'a> DecodeAndPlay<'a> {
    pub fn open(
        device: &'a Device,
        config: &'a SupportedStreamConfig,
        mss: MediaSourceStream,
        fmt_hint: Hint,
    ) -> Self {
        // Use the default options for metadata and format readers.
        let meta_opts: MetadataOptions = Default::default();
        let mut fmt_opts: FormatOptions = Default::default();
        fmt_opts.enable_gapless = true;
        // Use the default options for the decoder.
        let dec_opts: DecoderOptions = Default::default();

        // Probe the media source.
        let probed = symphonia::default::get_probe()
            .format(&fmt_hint, mss, &fmt_opts, &meta_opts)
            .expect("unsupported format");

        // Get the instantiated format reader.
        let format = probed.format;

        // Find the first audio track with a known (decodeable) codec.
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .expect("no supported audio tracks");

        // Create a decoder for the track.
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &dec_opts)
            .expect("unsupported codec");

        // Store the track identifier, it will be used to filter packets.
        let track_id = track.id;

        let (command_queue_sender, command_queue) = flume::bounded(1);
        Self {
            format,
            decoder,
            track_id,
            device,
            config,
            command_queue,
            command_queue_sender,
        }
    }

    pub fn get_command_queue(&self) -> Sender<Command> {
        self.command_queue_sender.clone()
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut audio_output = None::<Box<dyn AudioOutput>>;
        // The decode loop.
        loop {
            if let Ok(cmd) = self.command_queue.try_recv() {
                match cmd {
                    Command::Pause => {
                        audio_output.as_mut().map(|out| out.hint_pause());
                        let cmd = self.command_queue.recv_async().await?;
                        match cmd {
                            Command::Pause => { /* already paused */ }
                            Command::Play => {
                                audio_output.as_mut().map(|out| out.hint_play());
                            }
                            Command::Stop => {
                                audio_output.take().map(|mut out| out.flush());
                                return Ok(());
                            }
                        }
                    }
                    Command::Play => { /* already playing */ }
                    Command::Stop => {
                        audio_output.take().map(|mut out| out.flush());
                        return Ok(());
                    }
                }
            }
            // Get the next packet from the media format.
            let packet = match self.format.next_packet() {
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
            while !self.format.metadata().is_latest() {
                // Pop the old head of the metadata queue.
                self.format.metadata().pop();

                // Consume the new metadata at the head of the metadata queue.
            }

            // If the packet does not belong to the selected track, skip over it.
            if packet.track_id() != self.track_id {
                continue;
            }

            // Decode the packet into audio samples.
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    // If the audio output is not open, try to open it.
                    if audio_output.is_none() {
                        // Get the audio buffer specification. This is a description of the decoded
                        // audio buffer's sample format and sample rate.
                        let spec = *decoded.spec();
                        trace!("init audio output... [{:?} {}Hz]", spec.channels, spec.rate);

                        // Get the capacity of the decoded buffer. Note that this is capacity, not
                        // length! The capacity of the decoded buffer is constant for the life of the
                        // decoder, but the length is not.
                        let _duration = decoded.capacity() as u64;

                        // Try to open the audio output.
                        // Select proper playback routine based on sample format.
                        let output = match self.config.sample_format() {
                            cpal::SampleFormat::F32 => {
                                output::CpalAudioOutputImpl::<f32>::try_open(spec, self.device)?
                            }
                            cpal::SampleFormat::I16 => {
                                output::CpalAudioOutputImpl::<i16>::try_open(spec, self.device)?
                            }
                            cpal::SampleFormat::U16 => {
                                output::CpalAudioOutputImpl::<u16>::try_open(spec, self.device)?
                            }
                            sample_format => {
                                error!("Unsupported sample format '{sample_format}'");
                                bail!("Failed to initialize audio backend");
                            }
                        };
                        audio_output.replace(output);
                        if let Some(audio_output) = audio_output.as_mut() {
                            audio_output.write(decoded).await.unwrap();
                            audio_output.hint_play();
                        }
                    } else {
                        // TODO: Check the audio spec. and duration hasn't changed.
                        if let Some(audio_output) = audio_output.as_mut() {
                            audio_output.write(decoded).await.unwrap()
                        }
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
        // clean up audio output for next track (TODO: only replace what needs to be changed)
        if let Some(mut output) = audio_output {
            trace!("flush and close audio output...");
            output.flush();
        }
        Ok(())
    }
}

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
