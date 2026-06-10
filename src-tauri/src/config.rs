use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// Japanese meeting: transcribe in Japanese only
    Ja,
    /// English meeting: transcribe in English + translate to Japanese
    EnJa,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub direction: Direction,
    pub ollama_model: Option<String>,
    pub mic_enabled: bool,
    pub system_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            direction: Direction::Ja,
            ollama_model: None,
            mic_enabled: true,
            system_enabled: true,
        }
    }
}

impl Config {
    /// Language code passed to whisper
    pub fn whisper_language(&self) -> &'static str {
        match self.direction {
            Direction::Ja => "ja",
            Direction::EnJa => "en",
        }
    }
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

pub fn load(app: &AppHandle) -> Config {
    config_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, config: &Config) -> Result<(), String> {
    let path = config_path(app)?;
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
