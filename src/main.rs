use anyhow::{anyhow, Result};
use aw_client_light::AwClient;
use beeminder::BeeminderClient;
use config::Config;
use focusmate::FocusmateClient;
mod clean_tube_sync;
mod config;
mod focusmate_sync;

fn get_key(env_var_name: &str) -> Result<String> {
    std::env::var(env_var_name).map_or_else(|_| Err(anyhow!("{env_var_name} not found")), Ok)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let aw_client = AwClient::new(None);
    let bee_key = get_key(&config.beeminder_api_key_env)?;
    let bee_client = BeeminderClient::new(bee_key).with_username(config.beeminder_username);
    let fm_key = get_key(&config.focusmate_api_key_env)?;
    let fm_client = FocusmateClient::new(fm_key);

    clean_tube_sync::clean_tube_sync(&config.clean_tube_sync, &bee_client, &aw_client).await?;
    focusmate_sync::focusmate_sync(&config.focusmate_sync, &fm_client, &bee_client).await?;
    Ok(())
}
