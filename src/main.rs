use anyhow::Result;
use beeminder::BeeminderClient;
use config::Config;
mod clean_tube_sync;
mod clean_view_sync;
mod config;
mod fatebook_sync;
mod focusmate_sync;
mod key;

async fn run_sync<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    match f().await {
        Ok(()) => println!("  ✅ completed successfully"),
        Err(e) => eprintln!("  ❌ failed: {e}"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let bee_key = config.beeminder_key.get_value()?;
    let bee_client = BeeminderClient::new(bee_key).with_username(config.beeminder_username);

    if let Some(focusmate_config) = config.focusmate {
        run_sync(|| focusmate_sync::focusmate_sync(&focusmate_config, &bee_client)).await;
    }

    if let Some(fatebook_config) = config.fatebook {
        run_sync(|| fatebook_sync::fatebook_sync(&fatebook_config, &bee_client)).await;
    }

    if let Some(clean_tube_config) = config.clean_tube {
        run_sync(|| clean_tube_sync::clean_tube_sync(&clean_tube_config, &bee_client)).await;
    }

    if let Some(clean_view_config) = config.clean_view {
        run_sync(|| clean_view_sync::clean_view_sync(&clean_view_config, &bee_client)).await;
    }

    Ok(())
}
