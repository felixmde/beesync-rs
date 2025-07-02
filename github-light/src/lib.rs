use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Date formatting failed: {0}")]
    DateFormat(#[from] time::error::Format),

    #[error("GitHub API error ({status}): {message}")]
    Api { status: u16, message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub repository: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchCommit>,
}

#[derive(Debug, Deserialize)]
struct SearchCommit {
    sha: String,
    commit: CommitDetails,
    repository: Repository,
}

#[derive(Debug, Deserialize)]
struct CommitDetails {
    message: String,
}

#[derive(Debug, Deserialize)]
struct Repository {
    full_name: String,
}

/// A lightweight GitHub API client for fetching commit data.
///
/// This client provides methods to search for commits by author and date range,
/// with optional authentication using a GitHub personal access token.
pub struct GitHubClient {
    client: Client,
    token: Option<String>,
}

impl GitHubClient {
    #[must_use]
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            token,
        }
    }

    /// Fetches all commits by a user since the specified date.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the date format is invalid,
    /// or the GitHub API returns an error response.
    pub async fn get_commits(
        &self,
        username: &str,
        since: &OffsetDateTime,
    ) -> Result<Vec<Commit>, Error> {
        const PER_PAGE: usize = 100;
        let mut all_commits = Vec::new();
        let mut page = 1;

        loop {
            let commits = self
                .get_commits_page(username, since, page, PER_PAGE)
                .await?;

            let is_last_page = commits.len() < PER_PAGE;
            all_commits.extend(commits);

            if is_last_page {
                break;
            }

            page += 1;
        }

        Ok(all_commits)
    }

    async fn get_commits_page(
        &self,
        username: &str,
        since: &OffsetDateTime,
        page: u32,
        per_page: usize,
    ) -> Result<Vec<Commit>, Error> {
        let since_str = since.format(&Rfc3339)?;
        let query = format!("author:{username} committer-date:>{since_str}");

        let mut request = self
            .client
            .get("https://api.github.com/search/commits")
            .query(&[
                ("q", query.as_str()),
                ("sort", "committer-date"),
                ("order", "desc"),
                ("page", &page.to_string()),
                ("per_page", &per_page.to_string()),
            ])
            .header("Accept", "application/vnd.github.cloak-preview")
            .header("User-Agent", "github-light/0.1.0");

        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("token {token}"));
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_owned());
            return Err(Error::Api { status, message });
        }

        let search_response: SearchResponse = response.json().await?;

        let commits = search_response
            .items
            .into_iter()
            .map(|item| Commit {
                sha: item.sha,
                message: item.commit.message,
                repository: item.repository.full_name,
            })
            .collect();

        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[tokio::test]
    async fn test_github_commits() {
        let client = GitHubClient::new(None);
        let since = datetime!(2025-01-01 0:00 UTC);

        let result = client.get_commits("felixmde", &since).await;

        match result {
            Ok(commits) => {
                println!("Found {} commits", commits.len());
                for (i, commit) in commits.iter().enumerate().take(5) {
                    println!(
                        "{}. [{}] {} - {}",
                        i + 1,
                        commit.repository,
                        &commit.sha[..8],
                        commit.message.lines().next().unwrap_or("").trim()
                    );
                }
            }
            Err(e) => {
                eprintln!("Error fetching commits: {e}");
            }
        }
    }
}
