use crate::key::Key;
use anyhow::Result;
use beeminder::{types::CreateDatapoint, BeeminderClient};
use fatebook::FatebookClient;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct FatebookConfig {
    pub key: Key,
}

fn question_to_datapoint(question: &fatebook::Question) -> CreateDatapoint {
    let daystamp = format!(
        "{:04}{:02}{:02}",
        question.created_at.year(),
        question.created_at.month() as u8,
        question.created_at.day()
    );

    CreateDatapoint {
        value: 1.0,
        timestamp: Some(question.created_at),
        daystamp: Some(daystamp),
        comment: Some(question.title.to_string()),
        requestid: Some(question.id.clone()),
    }
}

pub async fn fatebook_sync(config: &FatebookConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("📚 fatebook-sync");
    let goal = "fatebook";

    let key = config.key.get_value()?;
    let fatebook = FatebookClient::new(key, None);

    let questions = fatebook.get_questions(None).await?;
    let existing_dps = beeminder
        .get_datapoints(goal, Some("timestamp"), Some(questions.len() as u64))
        .await?;

    let existing_ids: HashSet<_> = existing_dps
        .iter()
        .filter_map(|dp| dp.requestid.as_ref())
        .collect();

    let new_questions: Vec<_> = questions
        .into_iter()
        .filter(|q| !existing_ids.contains(&q.id))
        .collect();

    for question in new_questions.into_iter().rev() {
        let dp = question_to_datapoint(&question);
        beeminder.create_datapoint(goal, &dp).await?;

        if let Some(comment) = dp.comment.as_ref() {
            println!("  🆕 Created Fatebook datapoint: {comment}");
        }
    }

    Ok(())
}
