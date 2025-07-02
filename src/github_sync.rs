use crate::key::Key;
use anyhow::Result;
use beeminder::{types::CreateDatapoint, BeeminderClient};
use github_light::{Commit, GitHubClient};
use serde::Deserialize;
use std::collections::HashSet;
use time::OffsetDateTime;

#[derive(Deserialize)]
pub struct GitHubConfig {
    pub key: Option<Key>,
    pub goal_name: String,
    pub username: String,
}

fn commit_to_datapoint(commit: &Commit) -> CreateDatapoint {
    let daystamp = format!(
        "{:04}{:02}{:02}",
        commit.committer_date.year(),
        commit.committer_date.month() as u8,
        commit.committer_date.day()
    );

    let first_line = commit.message.lines().next().unwrap_or("").trim();
    let comment = format!("{}: {}", commit.repository, first_line);

    CreateDatapoint {
        value: 1.0,
        timestamp: Some(commit.committer_date),
        daystamp: Some(daystamp),
        comment: Some(comment),
        requestid: Some(commit.sha.clone()),
    }
}

pub async fn github_sync(config: &GitHubConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("🐙 github-sync");

    let token = match &config.key {
        Some(key) => Some(key.get_value()?),
        None => None,
    };
    let github = GitHubClient::new(token);

    let goal = &config.goal_name;
    let most_recent_github_dp = beeminder
        .get_datapoints(goal, Some("timestamp"), Some(1))
        .await?;

    let start = match most_recent_github_dp.first() {
        Some(dp) if dp.value != 0.0 => dp.timestamp,
        _ => OffsetDateTime::UNIX_EPOCH,
    };

    let commits = github.get_commits(&config.username, &start).await?;

    let existing_dps = beeminder
        .get_datapoints(goal, Some("timestamp"), Some(commits.len() as u64))
        .await?;

    let existing_shas: HashSet<_> = existing_dps
        .iter()
        .filter_map(|dp| dp.requestid.as_ref())
        .collect();

    let new_commits: Vec<_> = commits
        .into_iter()
        .filter(|commit| !existing_shas.contains(&commit.sha))
        .rev()
        .collect();

    for commit in new_commits {
        let dp = commit_to_datapoint(&commit);
        beeminder.create_datapoint(goal, &dp).await?;

        if let Some(comment) = dp.comment.as_ref() {
            println!("  🆕 Created GitHub datapoint: {comment}");
        }
    }

    Ok(())
}
