use reqwest::{Client, Error};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub title: String,
    #[serde(rename = "resolveBy")]
    #[serde(with = "time::serde::rfc3339")]
    pub resolve_by: OffsetDateTime,
    #[serde(rename = "createdAt")]
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub comment: Option<String>,
    #[serde(rename = "profileId")]
    pub profile_id: Option<String>,
    #[serde(rename = "type")]
    pub question_type: String,
    pub resolved: bool,
    #[serde(rename = "pingedForResolution")]
    pub pinged_for_resolution: bool,
    pub resolution: Option<String>,
    #[serde(rename = "resolvedAt")]
    #[serde(with = "time::serde::rfc3339::option")]
    pub resolved_at: Option<OffsetDateTime>,
    pub notes: Option<String>,
    #[serde(rename = "hideForecastsUntil")]
    pub hide_forecasts_until: Option<String>,
    #[serde(rename = "hideForecastsUntilPrediction")]
    pub hide_forecasts_until_prediction: bool,
    #[serde(rename = "userId")]
    pub user_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QuestionsResponse {
    pub items: Vec<Question>,
}

#[derive(Debug, Default, Serialize)]
pub struct GetQuestionsConfig {
    pub resolved: Option<bool>,
    pub unresolved: Option<bool>,
    pub ready_to_resolve: Option<bool>,
    pub resolving_soon: Option<bool>,
    pub limit: Option<i32>,
    pub search_string: Option<String>,
    pub show_all_public: Option<bool>,
}

pub struct FatebookClient {
    client: Client,
    url: String,
    api_key: String,
}

impl FatebookClient {
    #[must_use]
    pub fn new(api_key: String, url: Option<String>) -> Self {
        let url = url.unwrap_or("https://fatebook.io/api".to_string());
        Self {
            client: Client::new(),
            url,
            api_key,
        }
    }

    /// Retrieves a list of questions based on the provided configuration.
    ///
    /// If no config is provided, returns up to 10000 questions.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The API request fails
    /// - Response parsing fails
    pub async fn get_questions(
        &self,
        config: Option<GetQuestionsConfig>,
    ) -> Result<Vec<Question>, Error> {
        let url = format!("{}/v0/getQuestions", self.url);
        let mut request = self.client.get(&url).query(&[("apiKey", &self.api_key)]);

        request = match config {
            Some(config) => request.query(&config),
            None => request.query(&[("limit", "10000")]),
        };

        let response: QuestionsResponse = request.send().await?.json().await?;
        Ok(response.items)
    }

    /// Retrieves a specific question by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The API request fails
    /// - Response parsing fails
    /// - The question ID is not found
    pub async fn get_question(&self, question_id: &str) -> Result<Question, Error> {
        let url = format!("{}/v0/getQuestion", self.url);
        let response: Question = self
            .client
            .get(&url)
            .query(&[
                ("apiKey", &self.api_key),
                ("questionId", &question_id.to_string()),
            ])
            .send()
            .await?
            .json()
            .await?;
        Ok(response)
    }
}
