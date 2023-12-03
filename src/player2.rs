use symphonia::core::{
    audio::{RawSample, SampleBuffer},
    conv::{ConvertibleSample, IntoSample},
};

use crate::waker::Waker;

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

struct CpalOutputWriter<T: AudioOutputSample> {
    ring_buf_producer: rb::Producer<T>,
    sample_buf: SampleBuffer<T>,
    // woken by the audio thread when more samples are needed
    waker: Waker,
}
