use crate::key::Key;
use amazing_marvin_light::{AmazingMarvinClient, AmazingMarvinCredentials};
use anyhow::{anyhow, Result};
use beeminder::{types::CreateDatapoint, BeeminderClient};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use time::OffsetDateTime;

#[derive(Deserialize)]
pub struct CategorySyncConfig {
    pub uri: Key,
    pub username: Key,
    pub password: Key,
    pub database_name: Key,
    pub category: String,
    pub goal_name: String,
}

fn task_to_datapoint(task: &HashMap<String, Value>) -> Result<CreateDatapoint> {
    let id = task
        .get("_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Task missing _id field"))?;

    let title = task
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled task");

    let done_at = task
        .get("doneAt")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("Task missing doneAt field"))
        .and_then(|millis| {
            let seconds = i64::try_from(millis / 1000)
                .map_err(|_| anyhow!("Timestamp too large: {}", millis))?;
            OffsetDateTime::from_unix_timestamp(seconds)
                .map_err(|_| anyhow!("Invalid doneAt timestamp: {}", millis))
        })?;

    let daystamp = format!(
        "{:04}{:02}{:02}",
        done_at.year(),
        done_at.month() as u8,
        done_at.day()
    );

    Ok(CreateDatapoint {
        value: 1.0,
        timestamp: Some(done_at),
        daystamp: Some(daystamp),
        comment: Some(title.to_string()),
        requestid: Some(id.to_string()),
    })
}

pub async fn category_sync(config: &CategorySyncConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("📋 category-sync");

    let uri = config.uri.get_value()?;
    let username = config.username.get_value()?;
    let password = config.password.get_value()?;
    let database_name = config.database_name.get_value()?;

    let credentials = AmazingMarvinCredentials {
        uri,
        username,
        password,
        database_name,
    };

    let marvin_client = AmazingMarvinClient::new(&credentials);
    let goal = &config.goal_name;

    let done_tasks = marvin_client
        .find_recently_completed_tasks_in_category(&config.category)
        .await?;

    let existing_dps = beeminder
        .get_datapoints(goal, Some("timestamp"), None)
        .await?;

    let existing_ids: HashSet<_> = existing_dps
        .iter()
        .filter_map(|dp| dp.requestid.as_deref())
        .collect();

    let new_tasks: Vec<_> = done_tasks
        .into_iter()
        .filter(|task| {
            task.get("_id")
                .and_then(|v| v.as_str())
                .is_some_and(|id| !existing_ids.contains(id))
        })
        .collect();

    for task in new_tasks.into_iter().rev() {
        let dp = task_to_datapoint(&task)?;
        beeminder.create_datapoint(goal, &dp).await?;
        if let Some(comment) = dp.comment.as_ref() {
            println!("  🆕 Created Amazing Marvin datapoint: {comment}");
        }
    }

    Ok(())
}
