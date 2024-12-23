use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request error: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("Date format error: {0}")]
    DateFormatError(#[from] time::error::Format),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventData {
    pub app: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub duration: f64,
    pub data: EventData,
}

pub struct AwClient {
    client: Client,
    url: String,
}

impl AwClient {
    #[must_use]
    pub fn new(url: Option<String>) -> Self {
        let url = url.unwrap_or("http://localhost:5600/".to_string());
        Self {
            client: Client::new(),
            url,
        }
    }

    /// Fetches events from a bucket within a specified time range.
    ///
    /// # Arguments
    /// * `bucket` - The bucket ID to fetch events from
    /// * `start` - Start time of the range
    /// * `end` - End time of the range
    ///
    /// # Returns
    /// A Result containing either a Vec of Events or a reqwest Error
    ///
    /// # Errors
    /// Returns error if the HTTP request fails or if response cannot be parsed as JSON
    pub async fn get_events(
        &self,
        bucket: &str,
        start: &OffsetDateTime,
        end: &OffsetDateTime,
    ) -> Result<Vec<Event>, Error> {
        let url = format!("{}/api/0/buckets/{}/events", self.url, bucket);

        let start = start.format(&Rfc3339).map_err(Error::DateFormatError)?;
        let end = end.format(&Rfc3339).map_err(Error::DateFormatError)?;

        let response = self
            .client
            .get(&url)
            .query(&[
                ("start", start),
                ("end", end),
            ])
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    /// Sends an event heartbeat to a bucket. Events with identical data within the pulsetime
    /// window will be merged to save storage space.
    ///
    /// # Arguments
    /// * `bucket` - The bucket ID to send the heartbeat to
    /// * `event` - The event to send
    /// * `pulsetime` - Time window (in seconds) during which subsequent events with identical data will be merged
    ///
    /// # Returns
    /// A Result containing either unit () or a reqwest Error
    /// # Errors
    /// Returns error if the HTTP request fails or if the event cannot be serialized to JSON
    pub async fn heartbeat(
        &self,
        bucketname: &str,
        event: &Event,
        pulsetime: f64,
    ) -> Result<(), reqwest::Error> {
        let url = format!(
            "{}/api/0/buckets/{}/heartbeat?pulsetime={}",
            self.url, bucketname, pulsetime
        );
        self.client.post(url).json(&event).send().await?;
        Ok(())
    }
}

#[must_use]
pub fn sum_duration_by_title(events: &[Event]) -> std::collections::HashMap<String, f64> {
    events
        .iter()
        .fold(std::collections::HashMap::new(), |mut acc, event| {
            *acc.entry(event.data.title.clone()).or_default() += event.duration;
            acc
        })
}
