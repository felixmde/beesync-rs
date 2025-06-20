use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("JSON serialization/deserialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },
}

#[derive(Clone, Debug)]
pub struct AmazingMarvinCredentials {
    uri: String,
    username: String,
    password: String,
    database_name: String,
}

pub struct AmazingMarvinClient {
    client: Client,
    credentials: AmazingMarvinCredentials,
}

impl AmazingMarvinClient {
    #[must_use]
    pub fn new(credentials: &AmazingMarvinCredentials) -> Self {
        Self {
            client: Client::new(),
            credentials: credentials.clone(),
        }
    }

    pub async fn find_docs(&self, selector: &Value) -> Result<Vec<HashMap<String, Value>>, Error> {
        let url = format!(
            "{}/{}/_find",
            self.credentials.uri.trim_end_matches('/'),
            self.credentials.database_name
        );

        let query_body = serde_json::json!({
            "selector": selector
        });

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.credentials.username, Some(&self.credentials.password))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&query_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::Api { status, message });
        }

        let response_body: Value = response.json().await?;
        let docs = response_body
            .get("docs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|doc| {
                        if let Value::Object(map) = doc {
                            let mut result = HashMap::new();
                            for (key, value) in map {
                                result.insert(key.clone(), value.clone());
                            }
                            Some(result)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(docs)
    }

    pub async fn get_category_id_by_title(&self, title: &str) -> Result<String, Error> {
        let selector = serde_json::json!({
            "db": "Categories",
            "type": "category",
            "title": title
        });

        let docs = self.find_docs(&selector).await?;

        match docs.len() {
            0 => Err(Error::Api {
                status: 404,
                message: format!("No category found with title '{}'", title),
            }),
            1 => {
                let category = docs.into_iter().next().unwrap();
                category
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| Error::Api {
                        status: 400,
                        message: format!("Category '{}' does not have a valid _id", title),
                    })
            }
            count => Err(Error::Api {
                status: 409,
                message: format!(
                    "Found {} categories with title '{}', expected exactly one",
                    count, title
                ),
            }),
        }
    }

    pub async fn find_tasks_in_category(
        &self,
        category_title: &str,
    ) -> Result<Vec<HashMap<String, Value>>, Error> {
        let category_id = self.get_category_id_by_title(category_title).await?;
        let selector = serde_json::json!({
            "db": "Tasks",
            "parentId": category_id,
            "$or": [
                {"done": false},
                {"done": {"$exists": false}}
            ]
        });

        self.find_docs(&selector).await
    }

    pub async fn find_recently_completed_tasks_in_category(
        &self,
        category_title: &str,
    ) -> Result<Vec<HashMap<String, Value>>, Error> {
        let category_id = self.get_category_id_by_title(category_title).await?;

        // Calculate timestamp for two weeks ago
        let two_weeks_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - (14 * 24 * 60 * 60 * 1000); // 14 days in milliseconds

        let selector = serde_json::json!({
            "db": "Tasks",
            "parentId": category_id,
            "done": true,
            "doneAt": {
                "$gte": two_weeks_ago
            }
        });
        self.find_docs(&selector).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tokio;

    fn get_test_credentials() -> AmazingMarvinCredentials {
        let get_keyring = |key: &str| -> String {
            let username = std::env::var("USER").expect("$USER must be set for keyring command");
            let output = Command::new("keyring")
                .args(&["get", key, &username])
                .output()
                .expect("Failed to execute keyring command");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };

        AmazingMarvinCredentials {
            uri: get_keyring("am-uri"),
            username: get_keyring("am-username"),
            password: get_keyring("am-password"),
            database_name: get_keyring("am-database"),
        }
    }

    #[tokio::test]
    async fn test_find_docs_with_selector() {
        let credentials = get_test_credentials();
        let client = AmazingMarvinClient::new(&credentials);

        let selector_json = serde_json::json!({
            "db": "Categories",
            "type": "project",
            "$or": [
                {"done": false},
                {"done": {"$exists": false}}
            ]
        });

        let result = client.find_docs(&selector_json).await;
        assert!(result.is_ok(), "Expected Ok result, got: {:?}", result);
    }

    #[tokio::test]
    async fn test_find_tasks_in_must_do_category() {
        let credentials = get_test_credentials();
        let client = AmazingMarvinClient::new(&credentials);

        let result = client.find_tasks_in_category("Must Do").await;
        assert!(
            result.is_ok(),
            "Expected Ok for category 'Must Do', got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_find_recently_completed_tasks_in_must_do_category() {
        let credentials = get_test_credentials();
        let client = AmazingMarvinClient::new(&credentials);

        let result = client
            .find_recently_completed_tasks_in_category("Must Do")
            .await;
        assert!(
            result.is_ok(),
            "Expected Ok for category 'Must Do', got: {:?}",
            result
        );
    }
}
