pub mod caching;
pub mod fm;
pub mod fm_audio_src;
pub mod internet;

use crate::dsp::WbfmDemodulator;
use anyhow::Result;
use fm_audio_src::FmAudioSource;
use rodio::{OutputStream, OutputStreamHandle, Sink};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

#[derive(Debug, Clone, PartialEq)]
pub enum NowPlaying {
    Nothing,
    Internet { name: String, url: String },
    Fm { khz: u32, hardware: bool },
}

/// Owns the audio output device and coordinates whichever source (internet
/// stream or FM tuner) is currently feeding it. Only one plays at a time -
/// starting a new one tears down whatever was running before.
pub struct PlayerHandle {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sink: Arc<Sink>,
    pub now_playing: NowPlaying,
    fm_stop: Option<Arc<AtomicBool>>,
    pub last_error: Option<String>,
}

const FM_AUDIO_RATE: u32 = 48_000;

impl PlayerHandle {
    pub fn new(initial_volume: f32) -> Result<Self> {
        let (stream, stream_handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&stream_handle)?;
        sink.set_volume(initial_volume);
        Ok(PlayerHandle {
            _stream: stream,
            stream_handle,
            sink: Arc::new(sink),
            now_playing: NowPlaying::Nothing,
            fm_stop: None,
            last_error: None,
        })
    }

    pub fn set_volume(&self, v: f32) {
        self.sink.set_volume(v.clamp(0.0, 1.0));
    }

    pub fn volume(&self) -> f32 {
        self.sink.volume()
    }

    fn reset_sink(&mut self) -> Result<()> {
        if let Some(flag) = self.fm_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }
        let volume = self.sink.volume();
        let new_sink = Sink::try_new(&self.stream_handle)?;
        new_sink.set_volume(volume);
        self.sink = Arc::new(new_sink);
        Ok(())
    }

    pub fn play_internet(&mut self, name: String, url: String) -> Result<()> {
        self.reset_sink()?;
        self.last_error = None;
        let handle = internet::SinkHandle(self.sink.clone());
        internet::spawn_stream(url.clone(), handle);
        self.now_playing = NowPlaying::Internet { name, url };
        Ok(())
    }

    pub fn play_fm(&mut self, khz: u32) -> Result<()> {
        self.reset_sink()?;
        self.last_error = None;

        let stop_flag = Arc::new(AtomicBool::new(false));
        self.fm_stop = Some(stop_flag.clone());

        let (tx, rx) = mpsc::sync_channel::<f32>(FM_AUDIO_RATE as usize);
        let sink = self.sink.clone();
        let audio_source = FmAudioSource::new(rx, FM_AUDIO_RATE);
        sink.append(audio_source);

        let (mut tuner, fallback_note) = fm::open_best_available_tuner();
        self.last_error = fallback_note;
        let is_hardware = tuner.is_hardware();
        let _ = tuner.set_frequency_khz(khz);

        thread::spawn(move || {
            let mut demod =
                WbfmDemodulator::new(tuner.sample_rate(), FM_AUDIO_RATE as f32, 75_000.0);

            const CHUNK: usize = 4096;
            while !stop_flag.load(Ordering::Relaxed) {
                let iq = tuner.read_iq(CHUNK);
                if iq.is_empty() {
                    thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                let audio = demod.process(&iq);
                for sample in audio {
                    if tx.send(sample).is_err() {
                        return;
                    }
                }
            }
        });

        self.now_playing = NowPlaying::Fm {
            khz,
            hardware: is_hardware,
        };
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.reset_sink()?;
        self.now_playing = NowPlaying::Nothing;
        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        !matches!(self.now_playing, NowPlaying::Nothing)
    }
}
