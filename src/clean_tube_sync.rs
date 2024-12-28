use crate::config::CleanTubeSync;
use anyhow::Result;
use aw_client_light::AwClient;
use beeminder::{types::CreateDatapoint, BeeminderClient};
use std::collections::{HashMap, HashSet};
use time::{Duration, OffsetDateTime};

async fn get_seen_titles(aw: &AwClient, config: &CleanTubeSync) -> Result<Vec<String>> {
    let end = OffsetDateTime::now_utc();
    let start = end - Duration::days(config.lookback_days);
    let events = aw.get_events(&config.window_bucket, &start, &end).await?;

    let mut video_to_time: HashMap<String, f64> = HashMap::new();
    for event in events {
        let title = &event.data.title;
        if let Some((video_title, _)) = title.split_once(" - YouTube —") {
            let video = video_title.trim().to_string();
            *video_to_time.entry(video).or_default() += event.duration;
        }
    }

    let mut titles: Vec<_> = video_to_time
        .into_iter()
        .filter(|(_, duration)| *duration > config.min_video_duration_seconds)
        .map(|(video, _)| video)
        .collect();
    titles.sort();
    Ok(titles)
}

async fn get_logged_titles(
    beeminder: &BeeminderClient,
    config: &CleanTubeSync,
) -> Result<HashSet<String>> {
    let datapoints = beeminder
        .get_datapoints(&config.goal_name, None, Some(config.max_datapoints))
        .await?;

    Ok(datapoints
        .into_iter()
        .map(|dp| dp.comment.unwrap_or_default())
        .collect())
}

pub async fn clean_tube_sync(
    config: &CleanTubeSync,
    beeminder: &BeeminderClient,
    aw: &AwClient,
) -> Result<()> {
    let logged_titles = get_logged_titles(beeminder, config).await?;
    let seen_titles = get_seen_titles(aw, config).await?;
    println!("🚇 clean-tube-sync");

    for seen in seen_titles {
        if logged_titles.contains(&seen) {
            println!("  ✅ '{seen}' already logged!");
        } else {
            println!("  🆕 '{seen}' was logged!");
            let dp = CreateDatapoint {
                value: 1.0,
                comment: Some(seen),
                timestamp: Some(OffsetDateTime::now_utc()),
                daystamp: None,
                requestid: None,
            };
            beeminder.create_datapoint(&config.goal_name, &dp).await?;
        }
    }
    Ok(())
}
