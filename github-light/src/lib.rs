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
    pub committer_date: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
struct UserRepository {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct RepoCommit {
    sha: String,
    commit: RepoCommitDetails,
}

#[derive(Debug, Deserialize)]
struct RepoCommitDetails {
    message: String,
    committer: RepoCommitter,
}

#[derive(Debug, Deserialize)]
struct RepoCommitter {
    date: String,
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

    /// Fetches all repository names for a user.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the GitHub API returns an error response.
    pub async fn get_user_repositories(&self, username: &str) -> Result<Vec<String>, Error> {
        let mut request = self
            .client
            .get(format!("https://api.github.com/users/{username}/repos"))
            .query(&[("per_page", "100")])
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

        let repositories: Vec<UserRepository> = response.json().await?;
        Ok(repositories
            .into_iter()
            .map(|repo| repo.full_name)
            .collect())
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
        let repositories = self.get_user_repositories(username).await?;
        let mut all_commits = Vec::new();

        for repo in repositories {
            let commits = self.get_repository_commits(&repo, username, since).await?;
            all_commits.extend(commits);
        }

        Ok(all_commits)
    }

    /// Fetches commits for a specific repository since the specified date.
    async fn get_repository_commits(
        &self,
        repo: &str,
        username: &str,
        since: &OffsetDateTime,
    ) -> Result<Vec<Commit>, Error> {
        let since_str = since.format(&Rfc3339)?;

        let mut request = self
            .client
            .get(format!("https://api.github.com/repos/{repo}/commits"))
            .query(&[
                ("since", since_str.as_str()),
                ("author", username),
                ("per_page", "100"),
            ])
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

        let repo_commits: Vec<RepoCommit> = response.json().await?;

        let commits = repo_commits
            .into_iter()
            .map(|item| -> Result<Commit, Error> {
                let committer_date = OffsetDateTime::parse(&item.commit.committer.date, &Rfc3339)
                    .map_err(|_| Error::Api {
                    status: 500,
                    message: "Failed to parse commit date".to_string(),
                })?;

                Ok(Commit {
                    sha: item.sha,
                    message: item.commit.message,
                    repository: repo.to_string(),
                    committer_date,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(commits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[tokio::test]
    async fn test_github_user_repositories() {
        let client = GitHubClient::new(None);

        let result = client.get_user_repositories("felixmde").await;

        match result {
            Ok(repositories) => {
                println!("Found {} repositories", repositories.len());
                for (i, repo) in repositories.iter().enumerate().take(10) {
                    println!("{}. {}", i + 1, repo);
                }
                assert!(
                    !repositories.is_empty(),
                    "Should find at least one repository"
                );
            }
            Err(e) => {
                eprintln!("Error fetching repositories: {e}");
                panic!("Test failed with error: {e}");
            }
        }
    }

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
