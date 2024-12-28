use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub beeminder_api_key_env: String,
    pub focusmate_api_key_env: String,
    pub beeminder_username: String,
    pub clean_tube_sync: CleanTubeSync,
    pub focusmate_sync: FocusmateSync,
}

#[derive(Deserialize)]
pub struct CleanTubeSync {
    pub activity_watch_base_url: String,
    pub window_bucket: String,
    pub goal_name: String,
    pub lookback_days: i64,
    pub min_video_duration_seconds: f64,
    pub max_datapoints: u64,
}

#[derive(Deserialize)]
pub struct FocusmateSync {
    pub goal_name: String,
    pub auto_tags: Vec<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_string());
        let config_str = std::fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
