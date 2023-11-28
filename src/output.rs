use anyhow::{bail, Result};
use rb::{RbConsumer, RbProducer, SpscRb, RB};
use symphonia::core::audio::{AudioBufferRef, RawSample, SampleBuffer, SignalSpec};
use symphonia::core::conv::{ConvertibleSample, IntoSample};

use cpal::traits::{DeviceTrait, StreamTrait};

pub trait AudioOutput {
    fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()>;
    /// attempt to pause the output stream at the hardware level.
    /// this may not be supported, but if it isn't, the data callback will simply produce silent audio
    fn hint_pause(&mut self);
    /// attempt to start the stream at the hardware level.
    /// if this method is not supported, the result will be the same (stream plays)
    fn hint_play(&mut self);
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
    pub fn try_open(spec: SignalSpec, device: &cpal::Device) -> Result<Box<dyn AudioOutput>> {
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

        let sample_buf = SampleBuffer::<T>::new(0, spec);

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
        // AudioBuffer capacity is duration, SampleBuf capacity is duration * channels (total samples)
        if self.sample_buf.capacity() < decoded.capacity() * decoded.spec().channels.count() {
            self.sample_buf = SampleBuffer::new(decoded.capacity() as u64, *decoded.spec());
        }
        self.sample_buf.copy_interleaved_ref(decoded);
        let mut samples = self.sample_buf.samples();

        // Write all samples to the ring buffer.
        while let Some(written) = self.ring_buf_producer.write_blocking(samples) {
            samples = &samples[written..];
        }

        Ok(())
    }

    fn hint_pause(&mut self) {
        let _ = self.stream.pause();
    }

    fn hint_play(&mut self) {
        let _ = self.stream.play();
    }

    fn flush(&mut self) {
        // Flush is best-effort, ignore the returned result.
        let _ = self.stream.pause();
    }
}
