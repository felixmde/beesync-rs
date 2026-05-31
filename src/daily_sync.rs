use anyhow::Result;
use beeminder::{types::CreateDatapoint, types::UpdateDatapoint, BeeminderClient};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use std::collections::HashMap;
use time::{Duration, OffsetDateTime, UtcOffset};

fn default_lookback_days() -> i64 {
    3
}

#[derive(Deserialize)]
pub struct DailyPairMapping {
    pub pair: String,
    pub goal: String,
}

#[derive(Deserialize)]
pub struct DailyConfig {
    pub db_path: String,
    #[serde(default = "default_lookback_days")]
    pub lookback_days: i64,
    pub pairs: Vec<DailyPairMapping>,
}

#[derive(Debug, PartialEq)]
enum UpsertAction {
    Skip,
    Create,
    Update(String),
}

fn pair_value(side: Option<&str>, good_side: &str) -> f64 {
    match side {
        Some(s) if s == good_side => 1.0,
        _ => 0.0,
    }
}

fn daystamp_from_date(date: &str) -> String {
    date.replace('-', "")
}

fn decide_upsert(existing: Option<(&str, f64)>, value: f64) -> UpsertAction {
    match existing {
        None => UpsertAction::Create,
        Some((_, v)) if (v - value).abs() < 0.01 => UpsertAction::Skip,
        Some((id, _)) => UpsertAction::Update(id.to_string()),
    }
}

pub async fn daily_sync(_config: &DailyConfig, _beeminder: &BeeminderClient) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_value_good_side_reached_is_one() {
        assert_eq!(pair_value(Some("positive"), "positive"), 1.0);
        assert_eq!(pair_value(Some("negative"), "negative"), 1.0);
    }

    #[test]
    fn pair_value_wrong_side_is_zero() {
        assert_eq!(pair_value(Some("negative"), "positive"), 0.0);
    }

    #[test]
    fn pair_value_unresolved_is_zero() {
        assert_eq!(pair_value(None, "positive"), 0.0);
    }

    #[test]
    fn daystamp_strips_dashes() {
        assert_eq!(daystamp_from_date("2026-05-30"), "20260530");
    }

    #[test]
    fn decide_upsert_creates_when_absent() {
        assert_eq!(decide_upsert(None, 1.0), UpsertAction::Create);
    }

    #[test]
    fn decide_upsert_skips_when_equal() {
        assert_eq!(decide_upsert(Some(("abc", 1.0)), 1.0), UpsertAction::Skip);
    }

    #[test]
    fn decide_upsert_updates_when_different() {
        assert_eq!(
            decide_upsert(Some(("abc", 0.0)), 1.0),
            UpsertAction::Update("abc".to_string())
        );
    }
}
