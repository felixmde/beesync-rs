use anyhow::Result;
use beeminder::BeeminderClient;
use config::Config;
use time::{Date, OffsetDateTime, UtcOffset};
mod category_sync;
mod clean_tube_sync;
mod clean_view_sync;
mod config;
mod daylio_sync;
mod fatebook_sync;
mod focusmate_sync;
mod github_sync;
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

fn local_today() -> Date {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    OffsetDateTime::now_utc().to_offset(offset).date()
}

fn main() -> Result<()> {
    let today = local_today();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let config = Config::load()?;
        let bee_key = config.beeminder_key.get_value()?;
        let bee_client = BeeminderClient::new(bee_key).with_username(config.beeminder_username);

        if let Some(focusmate_config) = config.focusmate {
            run_sync(|| focusmate_sync::focusmate_sync(&focusmate_config, &bee_client)).await;
        }

        if let Some(fatebook_config) = config.fatebook {
            run_sync(|| fatebook_sync::fatebook_sync(&fatebook_config, &bee_client)).await;
        }

        if let Some(category_config) = config.category {
            run_sync(|| category_sync::category_sync(&category_config, &bee_client)).await;
        }

        if let Some(clean_tube_config) = config.clean_tube {
            run_sync(|| clean_tube_sync::clean_tube_sync(&clean_tube_config, &bee_client)).await;
        }

        if let Some(clean_view_config) = config.clean_view {
            run_sync(|| clean_view_sync::clean_view_sync(&clean_view_config, &bee_client)).await;
        }

        if let Some(github_config) = config.github {
            run_sync(|| github_sync::github_sync(&github_config, &bee_client)).await;
        }

        if let Some(daylio_config) = config.daylio {
            run_sync(|| daylio_sync::daylio_sync(&daylio_config, &bee_client, today)).await;
        }

        Ok(())
    })
}
