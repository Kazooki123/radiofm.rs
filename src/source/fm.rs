//! FM hardware tuning abstraction.
//!
//! `FmTuner` is the only thing a real device driver needs to implement.
//! Everything else — demodulation (`dsp`), the audio pipeline, and the TUI —
//! is written purely against this trait and doesn't know or care whether
//! the IQ samples came from silicon or from `SimulatedTuner`.

use anyhow::Result;
use num_complex::Complex32;

pub trait FmTuner: Send {
    fn set_frequency_khz(&mut self, khz: u32) -> Result<()>;
    fn sample_rate(&self) -> f32;
    fn read_iq(&mut self, n: usize) -> Vec<Complex32>;
    fn is_hardware(&self) -> bool;
}

pub const FM_BAND_MIN_KHZ: u32 = 87_500;
pub const FM_BAND_MAX_KHZ: u32 = 108_000;

/// A tuner with no physical device behind it. It synthesizes a broadcast-FM
/// style signal (carrier + a couple of audible tones frequency-modulated
/// onto it, plus a little noise) so the whole receive chain — discriminator,
/// de-emphasis, decimation, playback — runs against real signal math end to
/// end, even without a dongle plugged in.
pub struct SimulatedTuner {
    freq_khz: u32,
    sample_rate: f32,
    phase: f32,
    t: f32,
}

impl SimulatedTuner {
    pub fn new() -> Self {
        SimulatedTuner {
            freq_khz: 100_000,
            sample_rate: 1_024_000.0,
            phase: 0.0,
            t: 0.0,
        }
    }
}

impl FmTuner for SimulatedTuner {
    fn set_frequency_khz(&mut self, khz: u32) -> Result<()> {
        self.freq_khz = khz.clamp(FM_BAND_MIN_KHZ, FM_BAND_MAX_KHZ);
        Ok(())
    }

    fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    fn read_iq(&mut self, n: usize) -> Vec<Complex32> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let dt = 1.0 / self.sample_rate;
        let max_dev = 75_000.0_f32;
        let tone_a = 440.0 + (self.freq_khz % 1000) as f32;
        let tone_b = 660.0 - (self.freq_khz % 500) as f32;

        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let msg = 0.6 * (2.0 * std::f32::consts::PI * tone_a * self.t).sin()
                + 0.3 * (2.0 * std::f32::consts::PI * tone_b * self.t).sin();
            let inst_freq = msg * max_dev;
            self.phase += 2.0 * std::f32::consts::PI * inst_freq * dt;
            let noise = rng.gen_range(-0.02..0.02);
            out.push(Complex32::from_polar(1.0 + noise, self.phase));
            self.t += dt;
        }
        out
    }

    fn is_hardware(&self) -> bool {
        false
    }
}

/// Real RTL-SDR backend, built on the `rtl-sdr-rs` crate
/// (https://github.com/ccostes/rtl-sdr-rs). Only compiled when the
/// `hardware` feature is enabled (`cargo build --features hardware`) -
/// it needs libusb to link against and an actual dongle to open, neither
/// of which should be a requirement for the default build.
#[cfg(feature = "hardware")]
mod hardware {
    use super::*;
    use anyhow::anyhow;

    pub struct RtlSdrTuner {
        sdr: rtl_sdr_rs::RtlSdr,
        sample_rate: u32,
    }

    unsafe impl Send for RtlSdrTuner {}

    impl RtlSdrTuner {
        /// Opens the first RTL-SDR device (index 0), sets auto gain,
        /// disables the bias-tee (most dongles don't need it, and it can
        /// draw **more power** than some laptop USB ports like to give), and
        /// configures a capture rate matching what `dsp::WbfmDemodulator`
        /// expects.
        pub fn open() -> Result<Self> {
            let mut sdr = rtl_sdr_rs::RtlSdr::open(rtl_sdr_rs::DeviceId::Index(0))
                .map_err(|e| anyhow!("couldn't open RTL-SDR device: {e:?}"))?;
            sdr.set_tuner_gain(rtl_sdr_rs::TunerGain::Auto)
                .map_err(|e| anyhow!("couldn't set auto gain: {e:?}"))?;
            sdr.set_bias_tee(false)
                .map_err(|e| anyhow!("couldn't configure bias-tee: {e:?}"))?;

            let sample_rate = 1_024_000;
            sdr.set_sample_rate(sample_rate)
                .map_err(|e| anyhow!("couldn't set sample rate: {e:?}"))?;
            sdr.reset_buffer()
                .map_err(|e| anyhow!("couldn't reset USB buffer: {e:?}"))?;

            Ok(RtlSdrTuner { sdr, sample_rate })
        }
    }

    impl FmTuner for RtlSdrTuner {
        fn set_frequency_khz(&mut self, khz: u32) -> Result<()> {
            let hz = khz.clamp(FM_BAND_MIN_KHZ, FM_BAND_MAX_KHZ) * 1000;
            self.sdr
                .set_center_freq(hz)
                .map_err(|e| anyhow!("couldn't tune: {e:?}"))
        }

        fn sample_rate(&self) -> f32 {
            self.sample_rate as f32
        }

        fn read_iq(&mut self, n: usize) -> Vec<Complex32> {
            let mut buf = vec![0u8; n * 2];
            let read = match self.sdr.read_sync(&mut buf) {
                Ok(bytes) => bytes,
                Err(_) => return Vec::new(),
            };
            buf[..read]
                .chunks_exact(2)
                .map(|pair| {
                    let i = (pair[0] as f32 - 127.5) / 127.5;
                    let q = (pair[1] as f32 - 127.5) / 127.5;
                    Complex32::new(i, q)
                })
                .collect()
        }

        fn is_hardware(&self) -> bool {
            true
        }
    }
}

#[cfg(feature = "hardware")]
pub use hardware::RtlSdrTuner;

/// Picks the best tuner available: real hardware if the `hardware` feature
/// was compiled in *and* a dongle actually opens successfully, falling
/// back to the simulated tuner otherwise (including if a dongle isn't
/// plugged in). This is the one call site `PlayerHandle` needs -- it never
/// has to know which backend it got.
///
/// Returns the tuner plus an optional human-readable note about why it
/// fell back, so the UI can tell the user honestly what's happening.
pub fn open_best_available_tuner() -> (Box<dyn FmTuner>, Option<String>) {
    #[cfg(feature = "hardware")]
    {
        match hardware::RtlSdrTuner::open() {
            Ok(tuner) => return (Box::new(tuner), None),
            Err(e) => {
                return (
                    Box::new(SimulatedTuner::new()),
                    Some(format!("RTL-SDR not available ({e}) -- using simulated tuner.")),
                )
            }
        }
    }
    #[cfg(not(feature = "hardware"))]
    {
        (Box::new(SimulatedTuner::new()), None)
    }
}
