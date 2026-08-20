use anyhow::{bail, Context, Result};
use reqwest::{header::RETRY_AFTER, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

const CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const APP_URL: &str = "https://github.com/felixmde/beesync-rs";
const APP_NAME: &str = "beesync";

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    role: String,
    content: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message>,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    error: Option<ApiError>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    code: Option<Value>,
    message: Option<String>,
    #[serde(default)]
    metadata: Value,
}

pub struct LlmClient {
    client: Client,
    api_key: String,
    model: String,
}

impl LlmClient {
    pub fn new(api_key: String, model: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .context("failed to create the OpenRouter HTTP client")?;

        Ok(Self {
            client,
            api_key,
            model,
        })
    }

    pub async fn chat(&self, prompt: &str) -> Result<String> {
        let request = ChatRequest {
            model: &self.model,
            messages: vec![Message {
                role: "user".to_string(),
                content: Some(prompt.to_string()),
            }],
        };

        let response = self
            .client
            .post(CHAT_COMPLETIONS_URL)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", APP_URL)
            .header("X-Title", APP_NAME)
            .json(&request)
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to send OpenRouter chat request to {CHAT_COMPLETIONS_URL} for model `{}`; check network connectivity and the request timeout",
                    self.model
                )
            })?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.text().await.with_context(|| {
            format!(
                "failed to read the OpenRouter HTTP {status} response body for model `{}`",
                self.model
            )
        })?;

        parse_chat_response(status, retry_after.as_deref(), &body)
    }
}

fn parse_chat_response(
    status: StatusCode,
    retry_after: Option<&str>,
    body: &str,
) -> Result<String> {
    if body.trim().is_empty() {
        bail!(
            "OpenRouter returned an empty response body (HTTP {status}); check the API key, model, and provider availability"
        );
    }

    let response: ChatResponse = serde_json::from_str(body).map_err(|source| {
        let summary = truncate(body, 500);
        anyhow::anyhow!(
            "OpenRouter returned invalid JSON (HTTP {status}): {source}; response body: {summary:?}"
        )
    })?;

    if let Some(error) = response.error {
        return Err(format_api_error(status, retry_after, error));
    }

    if !status.is_success() {
        let summary = truncate(body, 500);
        bail!(
            "OpenRouter request failed with HTTP {status} and no structured API error; response body: {summary:?}"
        );
    }

    let choice = response.choices.into_iter().next().context(format!(
        "OpenRouter returned no completion choices (HTTP {status}); verify the model supports chat completions and that a provider is available"
    ))?;
    let content = choice.message.content.context(format!(
        "OpenRouter returned a completion choice with no text content (HTTP {status}); verify the selected model returned a text response"
    ))?;

    if content.trim().is_empty() {
        bail!(
            "OpenRouter returned an empty completion (HTTP {status}); verify the selected model returned text"
        );
    }

    Ok(content)
}

fn format_api_error(
    status: StatusCode,
    retry_after: Option<&str>,
    error: ApiError,
) -> anyhow::Error {
    let code = error
        .code
        .map(|code| match code {
            Value::String(code) => code,
            other => other.to_string(),
        })
        .unwrap_or_else(|| "unknown".to_string());
    let message = error
        .message
        .unwrap_or_else(|| "no message provided".to_string());
    let error_type = error
        .metadata
        .get("error_type")
        .and_then(Value::as_str)
        .map(|value| format!(", type {value}"))
        .unwrap_or_default();
    let retry = retry_after
        .map(|value| format!("; Retry-After: {value}"))
        .unwrap_or_default();

    anyhow::anyhow!(
        "OpenRouter request failed (HTTP {status}, API code {code}{error_type}): {message}{retry}"
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_successful_completion() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"no"}}],"error":null}"#;

        let result = parse_chat_response(StatusCode::OK, None, body).unwrap();

        assert_eq!(result, "no");
    }

    #[test]
    fn reports_structured_api_errors_with_retry_details() {
        let body = r#"{"choices":[],"error":{"code":429,"message":"Rate limit exceeded","metadata":{"error_type":"rate_limit_exceeded"}}}"#;

        let error = parse_chat_response(StatusCode::TOO_MANY_REQUESTS, Some("60"), body)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter request failed (HTTP 429 Too Many Requests, API code 429, type rate_limit_exceeded): Rate limit exceeded; Retry-After: 60"
        );
    }

    #[test]
    fn reports_missing_completion_content() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null}}],"error":null}"#;

        let error = parse_chat_response(StatusCode::OK, None, body)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter returned a completion choice with no text content (HTTP 200 OK); verify the selected model returned a text response"
        );
    }

    #[test]
    fn reports_missing_completion_choices() {
        let error = parse_chat_response(StatusCode::OK, None, r#"{"choices":[]}"#)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter returned no completion choices (HTTP 200 OK); verify the model supports chat completions and that a provider is available"
        );
    }

    #[test]
    fn reports_blank_completion_content() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"  "}}]}"#;

        let error = parse_chat_response(StatusCode::OK, None, body)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter returned an empty completion (HTTP 200 OK); verify the selected model returned text"
        );
    }

    #[test]
    fn reports_empty_response_body() {
        let error = parse_chat_response(StatusCode::BAD_GATEWAY, None, "\n")
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter returned an empty response body (HTTP 502 Bad Gateway); check the API key, model, and provider availability"
        );
    }

    #[test]
    fn reports_invalid_json_with_status_and_body() {
        let error = parse_chat_response(StatusCode::BAD_GATEWAY, None, "upstream unavailable")
            .unwrap_err()
            .to_string();

        assert!(error.starts_with("OpenRouter returned invalid JSON (HTTP 502 Bad Gateway):"));
        assert!(error.contains("response body: \"upstream unavailable\""));
    }

    #[test]
    fn reports_http_errors_without_structured_details() {
        let error = parse_chat_response(StatusCode::SERVICE_UNAVAILABLE, None, "{}")
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "OpenRouter request failed with HTTP 503 Service Unavailable and no structured API error; response body: \"{}\""
        );
    }
}
