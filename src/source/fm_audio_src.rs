//! Bridges the FM demod pipeline (which produces PCM samples in bursts on
//! a background thread) into something `rodio::Sink` can play, by pulling
//! samples off an mpsc channel.

use rodio::Source;
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub struct FmAudioSource {
    rx: Receiver<f32>,
    sample_rate: u32,
}

impl FmAudioSource {
    pub fn new(rx: Receiver<f32>, sample_rate: u32) -> Self {
        FmAudioSource { rx, sample_rate }
    }
}

impl Iterator for FmAudioSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.rx.recv().ok()
    }
}

impl Source for FmAudioSource {
    fn current_frame_len(&self) -> Option<usize> {
        None // unbounded stream
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
