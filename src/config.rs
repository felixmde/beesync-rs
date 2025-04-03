use crate::clean_tube_sync::CleanTubeConfig;
use crate::clean_view_sync::CleanViewConfig;
use crate::fatebook_sync::FatebookConfig;
use crate::focusmate_sync::FocusmateConfig;
use crate::key::Key;
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub beeminder_key: Key,
    pub beeminder_username: String,
    pub clean_tube: Option<CleanTubeConfig>,
    pub clean_view: Option<CleanViewConfig>,
    pub focusmate: Option<FocusmateConfig>,
    pub fatebook: Option<FatebookConfig>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_string());
        let config_str = std::fs::read_to_string(config_path)?;
        let config: Self = toml::from_str(&config_str)?;
        Ok(config)
    }
}
