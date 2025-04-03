use crate::key::Key;
use anyhow::Result;
use aw_client_light::AwClient;
use beeminder::types::CreateDatapoint;
use beeminder::BeeminderClient;
use gpt::GptClient;
use serde::Deserialize;
use std::collections::HashSet;
use time::{Duration, OffsetDateTime, Time, UtcOffset};

#[derive(Deserialize)]
pub struct CleanViewConfig {
    pub activity_watch_base_url: String,
    pub window_bucket: String,
    pub goal_name: String,
    pub lookback_days: i64,
    pub openai_key: Key,
    pub openai_model: String,
    pub min_window_duration_seconds: f64,
    pub prompt_template: String,
}

fn get_prompt(template: &str, titles: &[String]) -> String {
    let titles_str = titles.join("\n");
    template.replace("{{titles}}", &titles_str)
}

pub async fn clean_view_sync(config: &CleanViewConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("🧹 clean-view-sync");
    let aw = AwClient::new(Some(config.activity_watch_base_url.clone()));
    let gpt = GptClient::new(config.openai_key.get_value()?, config.openai_model.clone());
    let mut data_by_day: Vec<(String, Vec<String>)> = Vec::new();

    let offset = UtcOffset::current_local_offset()?;
    let now = OffsetDateTime::now_utc().to_offset(offset);
    let end_of_day_today = now.replace_time(Time::MIDNIGHT);

    for day_offset in (0..config.lookback_days).rev() {
        let end = end_of_day_today - Duration::days(day_offset);
        let start = end - Duration::days(1);
        let events = aw.get_events(&config.window_bucket, &start, &end).await?;

        let entries: HashSet<_> = events
            .into_iter()
            .filter(|event| event.duration > config.min_window_duration_seconds)
            .filter(|event| {
                let title = &event.data.title.to_lowercase();
                title.contains("firefox") || title.contains("brave") || title.contains("chromium")
            })
            .map(|event| event.data.title)
            .collect();

        let daystamp = format!("{:04}{:02}{:02}", end.year(), end.month() as u8, end.day());
        data_by_day.push((daystamp, entries.into_iter().collect()));
    }

    let existing_datapoints = beeminder
        .get_datapoints(&config.goal_name, None, Some(50))
        .await?;

    for (daystamp, titles) in &data_by_day {
        let (comment, value) = {
            if titles.is_empty() {
                ("🫙 No titles.".to_string(), 1.0)
            } else {
                let prompt = get_prompt(&config.prompt_template, titles);
                let result = gpt.chat(&prompt).await?;

                if result.trim() == "no" {
                    ("✨ GPT approved.".to_string(), 1.0)
                } else {
                    (result.lines().nth(1).unwrap_or_default().to_string(), 0.0)
                }
            }
        };

        let mut add_new_datapoint = true;
        for dp in &existing_datapoints {
            if *daystamp == dp.daystamp && (value - dp.value).abs() > 0.01 {
                println!("  ❌ Deleting existing wrong datapoint {daystamp}.");
                beeminder
                    .delete_datapoint(&config.goal_name, &dp.id)
                    .await?;
            } else if *daystamp == dp.daystamp {
                add_new_datapoint = false;
            }
        }

        if add_new_datapoint {
            if (value - 1.0).abs() > 0.01 {
                println!("  💦 Dirty datapoint for daystamp: {daystamp}.");
            } else {
                println!("  ✨ Clean datapoint for daystamp: {daystamp}.");
            }

            let dp = CreateDatapoint {
                value,
                comment: Some(comment),
                timestamp: None,
                daystamp: Some(daystamp.clone()),
                requestid: None,
            };

            beeminder.create_datapoint(&config.goal_name, &dp).await?;
        } else {
            println!("  ✅ Existing datapoint for {daystamp} is correct.");
        }
    }
    Ok(())
}
