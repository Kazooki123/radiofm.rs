//! Wideband-FM demodulation, written against raw IQ samples.
//!
//! This module DOES NOT care where the samples came from. Feed it a buffer
//! of complex baseband samples at some sample rate, and it hands back PCM
//! audio. That means the exact same code path runs whether the samples are
//! coming from a real RTL-SDR dongle or from `source::fm::SimulatedTuner`'s
//! synthetic signal — swapping in real hardware later is purely a matter of
//! implementing `FmTuner` (see `source::fm`), nothing in here changes actually.

use num_complex::Complex32;

/// A single-pole deemphasis filter, standard for broadcast FM (50µs in
/// most of the world, 75µs in the US). Broadcast FM pre-emphasizes high
/// frequencies before transmission; we undo that here.
pub struct DeEmphasis {
    alpha: f32,
    prev: f32,
}

impl DeEmphasis {
    pub fn new(sample_rate: f32, tau_micros: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let tau = tau_micros * 1e-6;
        let alpha = dt / (tau + dt);
        DeEmphasis { alpha, prev: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, sample: f32) -> f32 {
        self.prev += self.alpha * (sample - self.prev);
        self.prev
    }

    pub fn process_buf(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }
}

/// Quadrature FM discriminator: for each pair of consecutive IQ samples,
/// the instantaneous frequency is the phase difference between them. This
/// is the actual "FM demodulation" step -- everything else in this module
/// is filtering around it.
pub fn discriminate(iq: &[Complex32], prev_sample: &mut Complex32) -> Vec<f32> {
    let mut out = Vec::with_capacity(iq.len());
    for &s in iq {
        let d = s * prev_sample.conj();
        out.push(d.arg());
        *prev_sample = s;
    }
    out
}

/// Simple moving-average low-pass + integer decimation, used to bring a
/// wideband IQ capture rate (e.g. 1.024 Msps) down to something audio-rate
/// friendly before de-emphasis and output.
pub fn decimate(samples: &[f32], factor: usize) -> Vec<f32> {
    if factor <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(factor)
        .map(|chunk| chunk.iter().sum::<f32>() / chunk.len() as f32)
        .collect()
}

/// Full WBFM demod pipeline: IQ samples in, PCM f32 audio samples out.
pub struct WbfmDemodulator {
    prev_sample: Complex32,
    deemph: DeEmphasis,
    decimation: usize,
    gain: f32,
}

impl WbfmDemodulator {
    /// `iq_rate`: IQ capture sample rate in Hz.
    /// `audio_rate`: desired output PCM sample rate in Hz.
    /// `max_deviation`: FM max deviation in Hz (75kHz for broadcast FM).
    pub fn new(iq_rate: f32, audio_rate: f32, max_deviation: f32) -> Self {
        let decimation = (iq_rate / audio_rate).max(1.0).round() as usize;
        let gain = iq_rate / (2.0 * std::f32::consts::PI * max_deviation);
        WbfmDemodulator {
            prev_sample: Complex32::new(1.0, 0.0),
            deemph: DeEmphasis::new(audio_rate, 50.0),
            decimation,
            gain,
        }
    }

    pub fn process(&mut self, iq: &[Complex32]) -> Vec<f32> {
        let discriminated = discriminate(iq, &mut self.prev_sample);
        let mut audio = decimate(&discriminated, self.decimation);
        for s in audio.iter_mut() {
            *s = (*s * self.gain).clamp(-1.0, 1.0);
        }
        self.deemph.process_buf(&mut audio);
        audio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminator_recovers_constant_tone() {
        let n = 2000;
        let step = 0.05_f32;
        let iq: Vec<Complex32> = (0..n)
            .map(|i| Complex32::from_polar(1.0, step * i as f32))
            .collect();
        let mut prev = Complex32::new(1.0, 0.0);
        let out = discriminate(&iq, &mut prev);
        let avg: f32 = out[10..].iter().sum::<f32>() / (out.len() - 10) as f32;
        assert!((avg - step).abs() < 1e-3, "avg={avg} expected={step}");
    }
}
