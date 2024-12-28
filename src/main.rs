use anyhow::{anyhow, Result};
use aw_client_light::AwClient;
use beeminder::BeeminderClient;
use config::Config;
use focusmate::FocusmateClient;
mod clean_tube_sync;
mod config;
mod focusmate_sync;

fn get_key(env_var_name: &str) -> Result<String> {
    match std::env::var(env_var_name) {
        Ok(var) => Ok(var),
        Err(_) => Err(anyhow!("{env_var_name} not found")),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let aw_client = AwClient::new(None);
    let beeminder_key = get_key(&config.beeminder_api_key_env)?;
    let beeminder_client =
        BeeminderClient::new(beeminder_key).with_username(config.beeminder_username);
    let focusmate_key = get_key(&config.focusmate_api_key_env)?;
    let fm_client = FocusmateClient::new(focusmate_key);

    clean_tube_sync::clean_tube_sync(&config.clean_tube_sync, &beeminder_client, &aw_client)
        .await?;
    focusmate_sync::focusmate_sync(&config.focusmate_sync, &fm_client, &beeminder_client).await?;

    Ok(())
}
