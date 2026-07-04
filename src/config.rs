use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Station {
    pub name: String,
    pub kind: StationKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum StationKind {
    Internet(String),
    Fm(u32),
}

impl Station {
    pub fn label(&self) -> String {
        match &self.kind {
            StationKind::Internet(url) => format!("{}  ({})", self.name, url),
            StationKind::Fm(khz) => format!("{}  ({:.1} MHz)", self.name, *khz as f32 / 1000.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub volume: f32,
    #[serde(default)]
    pub stations: Vec<Station>,
    #[serde(default = "default_last_fm_khz")]
    pub last_fm_khz: u32,
}

fn default_last_fm_khz() -> u32 {
    100_000
}

impl Default for Config {
    fn default() -> Self {
        Config {
            volume: 0.6,
            stations: default_stations(),
            last_fm_khz: default_last_fm_khz(),
        }
    }
}

/// A handful of well-known public internet radio streams so the app is
/// useful the moment a user launches it, plus a couple of FM presets as examples q(≧▽≦q)!!
fn default_stations() -> Vec<Station> {
    vec![
        Station {
            name: "SomaFM - Groove Salad".into(),
            kind: StationKind::Internet(
                "https://ice1.somafm.com/groovesalad-128-mp3".into(),
            ),
        },
        Station {
            name: "SomaFM - Drone Zone".into(),
            kind: StationKind::Internet("https://ice1.somafm.com/dronezone-128-mp3".into()),
        },
        Station {
            name: "SomaFM - Space Station".into(),
            kind: StationKind::Internet(
                "https://ice1.somafm.com/spacestation-128-mp3".into(),
            ),
        },
        Station {
            name: "Radio Paradise - Main Mix".into(),
            kind: StationKind::Internet("https://stream.radioparadise.com/mp3-128".into()),
        },
        Station {
            name: "KEXP Seattle".into(),
            kind: StationKind::Internet("https://kexp-mp3-128.streamguys1.com/kexp128.mp3".into()),
        },
        Station {
            name: "BBC Radio".into(),
            kind: StationKind::Internet("https://garfnet.org.uk/download/radio/20231029-bbc-radio-with-rewind.m3u".into()),
        },
        Station {
            name: "Local FM".into(),
            kind: StationKind::Fm(101_100),
        },
    ]
}

fn config_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("rs", "starloexoliz", "radiofm")
        .context("could not determine config directory")?;
    let dir = dirs.config_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("radiofm.toml"))
}

impl Config {
    pub fn load() -> Result<Config> {
        let path = config_path()?;
        if !path.exists() {
            let cfg = Config::default();
            cfg.save().ok();
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading config at {}", path.display()))?;
        let cfg: Config = toml::from_str(&text).unwrap_or_default();
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        let text = toml::to_string_pretty(self)?;
        fs::write(path, text)?;
        Ok(())
    }
}
