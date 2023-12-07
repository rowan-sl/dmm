use std::{
    fs::File,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    thread,
};

use color_eyre::{
    eyre::{bail, Result},
    Report,
};
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Stream, SupportedStreamConfig,
};
use flume::Sender;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rb::{RbConsumer, RbProducer, SpscRb, RB};
use symphonia::core::{
    audio::{AudioBufferRef, RawSample, SampleBuffer, SignalSpec},
    codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
    conv::{ConvertibleSample, IntoSample},
    errors::Error as AudioError,
    formats::{FormatOptions, FormatReader, Packet, Track},
    io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe,
    units::Time,
};

pub trait AudioOutputSample:
    cpal::Sample
    + cpal::SizedSample
    + ConvertibleSample
    + IntoSample<f32>
    + RawSample
    + std::marker::Sync
    + std::marker::Send
    + 'static
{
}

impl AudioOutputSample for f32 {}
impl AudioOutputSample for i16 {}
impl AudioOutputSample for u16 {}

trait IsAudioWriter {
    fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()>;
}

struct AudioWriterImpl<T: AudioOutputSample> {
    ring_buf_producer: rb::Producer<T>,
    sample_buf: SampleBuffer<T>,
}

impl<T: AudioOutputSample> IsAudioWriter for AudioWriterImpl<T> {
    fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()> {
        // Do nothing if there are no audio frames.
        if decoded.frames() == 0 {
            return Ok(());
        }

        // Resampling is not required. Interleave the sample for cpal using a sample buffer.
        // AudioBuffer capacity is duration, SampleBuf capacity is duration * channels (total samples)
        if self.sample_buf.capacity() < decoded.capacity() * decoded.spec().channels.count() {
            self.sample_buf = SampleBuffer::new(decoded.capacity() as u64, *decoded.spec());
        }
        self.sample_buf.copy_interleaved_ref(decoded);

        let mut samples = self.sample_buf.samples();
        // Write enough samples to fill the ring buffer.
        while let Some(written) = self.ring_buf_producer.write_blocking(samples) {
            samples = &samples[written..];
        }
        Ok(())
    }
}

fn open_stream<T: AudioOutputSample>(
    spec: SignalSpec,
    device: &cpal::Device,
) -> Result<(Box<dyn IsAudioWriter>, Stream)> {
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

    let sample_buf = SampleBuffer::<T>::new(0, spec);
    Ok((
        Box::new(AudioWriterImpl {
            ring_buf_producer,
            sample_buf,
        }),
        stream,
    ))
}

struct AudioDecoder {
    fmt_reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: Track,
    track_id: u32,
}

impl AudioDecoder {
    pub fn new(source: impl MediaSource + 'static, format: probe::Hint) -> Result<Self> {
        let source_opts = MediaSourceStreamOptions::default();
        let metadata_opts = MetadataOptions::default();
        let mut format_opts = FormatOptions::default();
        format_opts.enable_gapless = true;
        let decoder_opts = DecoderOptions::default();

        let source_stream = MediaSourceStream::new(Box::new(source), source_opts);

        // probe the media source (look for a valid audio stream)
        let probe_res = symphonia::default::get_probe().format(
            &format,
            source_stream,
            &format_opts,
            &metadata_opts,
        )?;

        // get the created format reader
        let fmt_reader = probe_res.format;

        // Find the first audio track with a known (decodeable) codec.
        let track = fmt_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .expect("no supported audio tracks")
            .clone();

        // Create a decoder for the track.
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .expect("unsupported codec");

        // Store the track identifier, it will be used to filter packets.
        let track_id = track.id;

        Ok(Self {
            fmt_reader,
            decoder,
            track,
            track_id,
        })
    }

    pub fn decode_next<'buf>(&'buf mut self) -> Result<Decoded<'buf>, AudioError> {
        // Get the next packet from the media format.
        let packet = match self.fmt_reader.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::ResetRequired) => {
                // The track list has been changed. Re-examine it and create a new set of decoders,
                // then restart the decode loop. This is an advanced feature and it is not
                // unreasonable to consider this "the end." As of v0.5.0, the only usage of this is
                // for chained OGG physical streams.
                return Ok(Decoded::StreamEnd);
            }
            Err(err) => {
                // A unrecoverable error occured, halt decoding.
                ignore_end_of_stream_error(Err(err))?;
                // end of stream
                return Ok(Decoded::StreamEnd);
            }
        };

        // Consume any new metadata that has been read since the last packet.
        let _current_metadata = self.fmt_reader.metadata().skip_to_latest();

        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id() != self.track_id {
            return Ok(Decoded::Retry);
        }

        // Decode the packet into audio samples.
        match self.decoder.decode(&packet) {
            Ok(decoded) => Ok(Decoded::Decoded(packet, decoded)),
            Err(AudioError::IoError(_)) => {
                // The packet failed to decode due to an IO error, skip the packet.
                warn!("I/O Error during decoding [will attempt to continue]");
                Ok(Decoded::Retry)
            }
            Err(AudioError::DecodeError(err)) => {
                // The packet failed to decode due to invalid data, skip the packet.
                warn!("Decoder error: {err} [will attempt to continue]");
                Ok(Decoded::Retry)
            }
            Err(err) => {
                // An unrecoverable error occured, halt decoding.
                Err(err)
            }
        }
    }

    pub fn duration(&self) -> Time {
        self.track
            .codec_params
            .time_base
            .unwrap()
            .calc_time(self.track.codec_params.n_frames.unwrap())
    }
}

enum Decoded<'a> {
    StreamEnd,
    /// something uninformative happened, need to consume another packet
    /// this indicates that decode_next should be called again.
    Retry,
    Decoded(Packet, AudioBufferRef<'a>),
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

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoPrimitive, TryFromPrimitive)]
pub enum State {
    Playing = 0,
    Paused = 1,
    Stopped = 2,
}

enum PlayTaskCmd {
    Play,
    Pause,
    Stop,
    // start playing (from stopped)
    Start,
    SetOnTrackComplete(Box<dyn Fn() + Send + Sync + 'static>),
    SetNewSource { track_src: File, filetype: String },
}

pub struct SingleTrackPlayer {
    state: Arc<AtomicU8>,
    tx: Sender<PlayTaskCmd>,
    duration: Arc<AtomicU64>,
    time: Arc<AtomicU64>,
}

impl SingleTrackPlayer {
    pub fn new(config: Arc<SupportedStreamConfig>, device: Arc<cpal::Device>) -> Result<Self> {
        let (tx, rx) = flume::unbounded::<PlayTaskCmd>();
        let state = Arc::new(AtomicU8::new(State::Stopped as u8));
        let state_2 = state.clone();
        let duration = Arc::new(AtomicU64::new(0));
        let duration_2 = duration.clone();
        let time = Arc::new(AtomicU64::new(0));
        let time_2 = time.clone();

        thread::Builder::new()
            .name("audio-decode".to_string())
            .spawn(move || {
                let mut on_track_complete = None::<Box<dyn Fn() + Send + Sync + 'static>>;
                let mut outer_decoder = None;
                state_2.store(State::Stopped as u8, Ordering::SeqCst);
                'run: loop {
                    match rx.recv() {
                        Ok(PlayTaskCmd::Start) => {
                            assert!(outer_decoder.is_some(), "cannot start stream with no source set");
                        },
                        Ok(PlayTaskCmd::SetNewSource { track_src, filetype }) => {
                            // Create the media source stream.
                            let mss = MediaSourceStream::new(Box::new(track_src), Default::default());

                            // Create a probe hint using the file's extension. [Optional]
                            let mut hint = probe::Hint::new();
                            hint.with_extension(&filetype);
                            outer_decoder = Some(AudioDecoder::new(mss, hint)?);
                            continue 'run;
                        },
                        Ok(PlayTaskCmd::SetOnTrackComplete(call)) => {
                            on_track_complete = Some(call);
                            continue 'run;
                        }
                        Ok(..) => unreachable!(),
                        Err(flume::RecvError::Disconnected) => break 'run,
                    }
                    let mut decoder = outer_decoder.take().unwrap();
                    let tb = decoder.track.codec_params.time_base.unwrap();
                    let dur = decoder.duration();
                    let mut audio_output = None::<(Box<dyn IsAudioWriter>, cpal::Stream)>;
                    state_2.store(State::Playing as u8, Ordering::SeqCst);
                    'play: loop {
                        match rx.try_recv() {
                            Ok(PlayTaskCmd::Play) => {
                                warn!("Received play command, but audio is already playing")
                            }
                            Ok(PlayTaskCmd::Pause) => {
                                state_2.store(State::Paused as u8, Ordering::SeqCst);
                                if let Some(audio_output) = audio_output.as_mut() {
                                    let _ = audio_output.1.pause();
                                }
                                'paused: loop {
                                    match rx.recv() {
                                        Ok(PlayTaskCmd::Pause) => {
                                            warn!("Received pause command, but audio is already paused")
                                        }
                                        Ok(PlayTaskCmd::Play) => {
                                            if let Some(audio_output) = audio_output.as_mut() {
                                                let _ = audio_output.1.play();
                                            }
                                            break 'paused;
                                        }
                                        Ok(PlayTaskCmd::Stop) => break 'play,
                                        Ok(PlayTaskCmd::SetOnTrackComplete(call)) => {
                                            on_track_complete = Some(call)
                                        }
                                        // player is stopped before this happens
                                        Ok(PlayTaskCmd::SetNewSource { .. }) => unreachable!(),
                                        Ok(PlayTaskCmd::Start) => unreachable!(),
                                        Err(flume::RecvError::Disconnected) => break 'run,
                                    }
                                }
                                state_2.store(State::Playing as u8, Ordering::SeqCst);
                            }
                            Ok(PlayTaskCmd::Stop) => {
                                if let Some(audio_output) = audio_output.as_mut() {
                                    let _ = audio_output.1.pause();
                                }
                                break 'play;
                            }
                            Ok(PlayTaskCmd::SetOnTrackComplete(call)) => on_track_complete = Some(call),
                            // player is stopped before this happens
                            Ok(PlayTaskCmd::SetNewSource { .. }) => unreachable!(),
                            Ok(PlayTaskCmd::Start) => unreachable!(),
                            Err(flume::TryRecvError::Empty) => {}
                            Err(flume::TryRecvError::Disconnected) => {
                                if let Some(audio_output) = audio_output.as_mut() {
                                    let _ = audio_output.1.pause();
                                }
                                break 'run;
                            }
                        }
                        match decoder.decode_next() {
                            // call on_track_complete and clean up audio stream
                            Ok(Decoded::StreamEnd) => break 'play,
                            Ok(Decoded::Retry) => continue,
                            Ok(Decoded::Decoded(packet, buffer)) => {
                                duration_2.store(dur.seconds, std::sync::atomic::Ordering::Relaxed);
                                time_2.store(tb.calc_time(packet.ts()).seconds, std::sync::atomic::Ordering::Relaxed);
                                // If the audio output is not open, try to open it.
                                if audio_output.is_none() {
                                    // Get the audio buffer specification. This is a description of the decoded
                                    // audio buffer's sample format and sample rate.
                                    let spec = *buffer.spec();
                                    trace!(
                                        "init audio output... [{:?} {}Hz]",
                                        spec.channels,
                                        spec.rate
                                    );

                                    // Get the capacity of the decoded buffer. Note that this is capacity, not
                                    // length! The capacity of the decoded buffer is constant for the life of the
                                    // decoder, but the length is not.
                                    let _duration = buffer.capacity() as u64;

                                    // Try to open the audio output.
                                    // Select proper playback routine based on sample format.
                                    let output = match config.sample_format() {
                                        cpal::SampleFormat::F32 => open_stream::<f32>(spec, &device)?,
                                        cpal::SampleFormat::I16 => open_stream::<i16>(spec, &device)?,
                                        cpal::SampleFormat::U16 => open_stream::<u16>(spec, &device)?,
                                        sample_format => {
                                            error!("Unsupported sample format '{sample_format}'");
                                            bail!("Failed to initialize audio backend");
                                        }
                                    };
                                    audio_output.replace(output);
                                    if let Some(audio_output) = audio_output.as_mut() {
                                        audio_output.0.write(buffer)?;
                                        // Start the output stream.
                                        if let Err(err) = audio_output.1.play() {
                                            error!("audio output stream play error: {}", err);
                                            bail!("failed to initialize audio backend");
                                        }
                                    }
                                } else {
                                    // TODO: Check the audio spec. and duration hasn't changed.
                                    if let Some(audio_output) = audio_output.as_mut() {
                                        audio_output.0.write(buffer)?
                                    }
                                }
                            }
                            Err(error) => {
                                // report error and clean up audio stream
                                if let Some(audio_output) = audio_output.as_mut() {
                                    let _ = audio_output.1.pause();
                                }
                                Err(error)?
                            }
                        }
                    }
                    state_2.store(State::Stopped as u8, Ordering::SeqCst);
                    // flush audio stream
                    if let Some(audio_output) = audio_output.as_mut() {
                        let _ = audio_output.1.pause();
                    }
                    if let Some(call) = on_track_complete.as_ref() {
                        (call)();
                    }
                }
                state_2.store(State::Stopped as u8, Ordering::SeqCst);
                Ok::<_, Report>(())
            })?;

        Ok(Self {
            state,
            tx,
            duration,
            time,
        })
    }

    pub fn duration(&mut self) -> u64 {
        self.duration.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn timestamp(&mut self) -> u64 {
        self.time.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn state(&self) -> State {
        self.state
            .load(std::sync::atomic::Ordering::SeqCst)
            .try_into()
            .unwrap()
    }

    pub fn set_track(&mut self, track_src: File, filetype: String) -> Result<()> {
        self.stop()?;
        self.tx.try_send(PlayTaskCmd::SetNewSource {
            track_src,
            filetype,
        })?;
        Ok(())
    }

    pub fn on_track_complete(&mut self, call: impl Fn() + Send + Sync + 'static) -> Result<()> {
        self.tx
            .try_send(PlayTaskCmd::SetOnTrackComplete(Box::new(call)))?;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        if let State::Playing = self.state() {
            self.tx.try_send(PlayTaskCmd::Pause)?;
        }
        Ok(())
    }

    pub fn play(&mut self) -> Result<()> {
        if let State::Paused = self.state() {
            self.tx.try_send(PlayTaskCmd::Play)?;
        }
        if let State::Stopped = self.state() {
            self.tx.try_send(PlayTaskCmd::Start)?;
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        if let State::Paused | State::Playing = self.state() {
            self.tx.try_send(PlayTaskCmd::Stop)?;
        }
        Ok(())
    }
}
