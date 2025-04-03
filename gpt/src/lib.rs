use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

pub struct GptClient {
    client: Client,
    api_key: String,
    model: String,
}

impl GptClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    pub async fn chat(&self, prompt: &str) -> Result<String> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let response = self.client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;

        Ok(response.choices[0].message.content.clone())
    }
}
