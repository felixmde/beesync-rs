use crate::key::Key;
use anyhow::Result;
use beeminder::{types::CreateDatapoint, BeeminderClient};
use github_light::{Commit, GitHubClient};
use serde::Deserialize;
use std::collections::HashSet;
use time::{Duration, OffsetDateTime};

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
        .get_datapoints(goal, Some("timestamp"), Some(1), None, None)
        .await?;

    let start = match most_recent_github_dp.first() {
        Some(dp) if dp.value != 0.0 => dp.timestamp - Duration::days(2),
        _ => OffsetDateTime::UNIX_EPOCH,
    };

    let commits = github.get_commits(&config.username, &start).await?;
    let existing_shas = existing_requestids(beeminder, goal, start).await?;

    let new_commits: Vec<_> = commits
        .into_iter()
        .filter(|commit| !existing_shas.contains(&commit.sha))
        .rev()
        .collect();

    let mut failures = 0;

    for commit in new_commits {
        let dp = commit_to_datapoint(&commit);
        let comment = dp.comment.clone().unwrap_or_else(|| commit.sha.clone());

        match beeminder.create_datapoint(goal, &dp).await {
            Ok(_) => println!("  🆕 Created GitHub datapoint: {comment}"),
            // Beeminder rejects a repeat POST of an unchanged requestid, so an
            // already-present datapoint means this commit is synced.
            Err(e) if is_duplicate_request(&e) => println!("  ⏭️  Already synced: {comment}"),
            Err(e) => {
                failures += 1;
                eprintln!("  ⚠️  Failed to sync {comment}: {e}");
            }
        }
    }

    if failures > 0 {
        anyhow::bail!("{failures} commit(s) could not be synced");
    }

    Ok(())
}

/// Collects the request IDs (commit SHAs) of datapoints already on the goal,
/// reaching back at least as far as `start`.
///
/// The window is driven by `start` rather than by the number of commits found:
/// a datapoint whose commit no longer appears in GitHub's listing (rebased or
/// amended away) would otherwise push real commits out of view and make them
/// look unsynced.
async fn existing_requestids(
    beeminder: &BeeminderClient,
    goal: &str,
    start: OffsetDateTime,
) -> Result<HashSet<String>> {
    let mut count = 100;

    loop {
        // Sorted by timestamp descending, so the last entry is the oldest.
        let datapoints = beeminder
            .get_datapoints(goal, Some("timestamp"), Some(count), None, None)
            .await?;

        let exhausted = (datapoints.len() as u64) < count;
        let covers_window = datapoints.last().is_some_and(|dp| dp.timestamp < start);

        if exhausted || covers_window {
            return Ok(datapoints
                .into_iter()
                .filter_map(|dp| dp.requestid)
                .collect());
        }

        count *= 2;
    }
}

fn is_duplicate_request(error: &beeminder::Error) -> bool {
    matches!(
        error,
        beeminder::Error::HttpStatus { status: 422, body, .. } if body.contains("Duplicate request")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_status(status: u16, body: &str) -> beeminder::Error {
        beeminder::Error::HttpStatus {
            status,
            reason: "Unprocessable Entity".to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn duplicate_request_is_recognized() {
        assert!(is_duplicate_request(&http_status(
            422,
            r#"{"errors":"Duplicate request"}"#
        )));
    }

    #[test]
    fn other_errors_are_not_duplicates() {
        assert!(!is_duplicate_request(&http_status(
            422,
            r#"{"errors":{"value":["is not a number"]}}"#
        )));
        assert!(!is_duplicate_request(&http_status(404, "Not found")));
    }
}
