use crate::config::{Config, Station, StationKind};
use crate::source::{fm::FM_BAND_MAX_KHZ, fm::FM_BAND_MIN_KHZ, NowPlaying, PlayerHandle};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    None,
    FmFrequency,
    NewStationName,
    NewStationUrl { name: String },
}

pub struct App {
    pub config: Config,
    pub player: PlayerHandle,
    pub stations: Vec<Station>,
    pub selected: usize,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub status: String,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Result<Self> {
        let config = Config::load().unwrap_or_default();
        let player = PlayerHandle::new(config.volume)?;
        let stations = config.stations.clone();
        Ok(App {
            config,
            player,
            stations,
            selected: 0,
            input_mode: InputMode::None,
            input_buffer: String::new(),
            status: "Welcome to radiofm.rs \u{1F63C}  - press ? for help".to_string(),
            should_quit: false,
        })
    }

    fn sync_config_and_save(&mut self) {
        self.config.stations = self.stations.clone();
        self.config.volume = self.player.volume();
        let _ = self.config.save();
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.input_mode != InputMode::None {
            self.handle_input_mode_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Enter => self.play_selected(),
            KeyCode::Char('s') => {
                let _ = self.player.stop();
                self.status = "Stopped.".to_string();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust_volume(0.05),
            KeyCode::Char('-') | KeyCode::Char('_') => self.adjust_volume(-0.05),
            KeyCode::Left => self.nudge_fm(-100),
            KeyCode::Right => self.nudge_fm(100),
            KeyCode::Char('f') => {
                self.input_mode = InputMode::FmFrequency;
                self.input_buffer.clear();
                self.status = "Type an FM frequency in MHz (e.g. 101.1) and press Enter."
                    .to_string();
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::NewStationName;
                self.input_buffer.clear();
                self.status = "New internet station -- type a name and press Enter.".to_string();
            }
            KeyCode::Char('d') => self.delete_selected(),
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.adjust_volume(0.03),
            MouseEventKind::ScrollDown => self.adjust_volume(-0.03),
            _ => {}
        }
    }

    fn handle_input_mode_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::None;
                self.input_buffer.clear();
                self.status = "Cancelled.".to_string();
            }
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => self.input_buffer.push(c),
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        match std::mem::replace(&mut self.input_mode, InputMode::None) {
            InputMode::FmFrequency => {
                let text = self.input_buffer.trim();
                match text.parse::<f32>() {
                    Ok(mhz) => {
                        let khz = (mhz * 1000.0).round() as u32;
                        let khz = khz.clamp(FM_BAND_MIN_KHZ, FM_BAND_MAX_KHZ);
                        self.config.last_fm_khz = khz;
                        match self.player.play_fm(khz) {
                            Ok(()) => {
                                self.status = self.player.last_error.clone().unwrap_or_else(|| {
                                    format!(
                                        "Tuned to {:.1} MHz.",
                                        khz as f32 / 1000.0
                                    )
                                })
                            }
                            Err(e) => self.status = format!("Couldn't tune: {e}"),
                        }
                    }
                    Err(_) => {
                        self.status = "That didn't look like a frequency, e.g. try 101.1"
                            .to_string()
                    }
                }
            }
            InputMode::NewStationName => {
                let name = self.input_buffer.trim().to_string();
                if name.is_empty() {
                    self.status = "Name can't be empty.".to_string();
                } else {
                    self.input_mode = InputMode::NewStationUrl { name };
                    self.input_buffer.clear();
                    self.status = "Now type the stream URL and press Enter.".to_string();
                    return;
                }
            }
            InputMode::NewStationUrl { name } => {
                let url = self.input_buffer.trim().to_string();
                if url.is_empty() {
                    self.status = "URL can't be empty.".to_string();
                } else {
                    self.stations.push(Station {
                        name: name.clone(),
                        kind: StationKind::Internet(url),
                    });
                    self.selected = self.stations.len() - 1;
                    self.status = format!("Added \"{name}\" to your stations.");
                    self.sync_config_and_save();
                }
            }
            InputMode::None => {}
        }
        self.input_buffer.clear();
    }

    fn move_selection(&mut self, delta: i32) {
        if self.stations.is_empty() {
            return;
        }
        let len = self.stations.len() as i32;
        let mut idx = self.selected as i32 + delta;
        idx = idx.rem_euclid(len);
        self.selected = idx as usize;
    }

    fn play_selected(&mut self) {
        let Some(station) = self.stations.get(self.selected).cloned() else {
            return;
        };
        let result = match &station.kind {
            StationKind::Internet(url) => {
                self.player.play_internet(station.name.clone(), url.clone())
            }
            StationKind::Fm(khz) => {
                self.config.last_fm_khz = *khz;
                self.player.play_fm(*khz)
            }
        };
        match result {
            Ok(()) => {
                self.status = self
                    .player
                    .last_error
                    .clone()
                    .unwrap_or_else(|| format!("Now playing: {}", station.name))
            }
            Err(e) => self.status = format!("Playback error: {e}"),
        }
    }

    fn delete_selected(&mut self) {
        if self.stations.is_empty() {
            return;
        }
        let removed = self.stations.remove(self.selected);
        if self.selected >= self.stations.len() && self.selected > 0 {
            self.selected -= 1;
        }
        self.status = format!("Removed \"{}\".", removed.name);
        self.sync_config_and_save();
    }

    fn adjust_volume(&mut self, delta: f32) {
        let v = (self.player.volume() + delta).clamp(0.0, 1.0);
        self.player.set_volume(v);
        self.config.volume = v;
    }

    /// Fine-tune the currently playing FM frequency by `delta_khz`, or if
    /// nothing's playing, just move the last-tuned frequency for next time.
    fn nudge_fm(&mut self, delta_khz: i32) {
        let current = match self.player.now_playing {
            NowPlaying::Fm { khz, .. } => khz,
            _ => self.config.last_fm_khz,
        };
        let new_khz = (current as i32 + delta_khz).clamp(
            FM_BAND_MIN_KHZ as i32,
            FM_BAND_MAX_KHZ as i32,
        ) as u32;
        self.config.last_fm_khz = new_khz;
        if matches!(self.player.now_playing, NowPlaying::Fm { .. }) {
            if let Err(e) = self.player.play_fm(new_khz) {
                self.status = format!("Couldn't retune: {e}");
            } else {
                self.status = self.player.last_error.clone().unwrap_or_else(|| {
                    format!("Tuned to {:.1} MHz.", new_khz as f32 / 1000.0)
                });
            }
        }
    }

    pub fn on_exit(&mut self) {
        self.sync_config_and_save();
    }
}
