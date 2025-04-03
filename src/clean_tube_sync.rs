use anyhow::Result;
use aw_client_light::AwClient;
use beeminder::{types::CreateDatapoint, BeeminderClient};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use time::{Duration, OffsetDateTime};

#[derive(Deserialize)]
pub struct CleanTubeConfig {
    pub activity_watch_base_url: String,
    pub window_bucket: String,
    pub goal_name: String,
    pub lookback_days: i64,
    pub min_video_duration_seconds: f64,
    pub max_datapoints: u64,
}

async fn get_seen_titles(aw: &AwClient, config: &CleanTubeConfig) -> Result<Vec<String>> {
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
    config: &CleanTubeConfig,
) -> Result<HashSet<String>> {
    let datapoints = beeminder
        .get_datapoints(&config.goal_name, None, Some(config.max_datapoints))
        .await?;

    Ok(datapoints
        .into_iter()
        .map(|dp| dp.comment.unwrap_or_default())
        .collect())
}

pub async fn clean_tube_sync(config: &CleanTubeConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("🚇 clean-tube-sync");
    let aw = AwClient::new(Some(config.activity_watch_base_url.clone()));
    let logged_titles = get_logged_titles(beeminder, config).await?;
    let seen_titles = get_seen_titles(&aw, config).await?;

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
