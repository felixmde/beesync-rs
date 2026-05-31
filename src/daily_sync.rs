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

struct PairMapping {
    pair_id: i64,
    good_side: String,
    positive_name: String,
    goal: String,
}

struct CheckoutEntry {
    id: i64,
    entry_date: String,
    mood: Option<i64>,
    sides: HashMap<i64, String>,
}

fn open_db(path: &str) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(conn)
}

fn resolve_mappings(conn: &Connection, pairs: &[DailyPairMapping]) -> Result<Vec<PairMapping>> {
    let mut out = Vec::new();
    for p in pairs {
        let row = conn
            .query_row(
                "SELECT id, good_side FROM pair WHERE positive_name = ?1",
                [&p.pair],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        match row {
            Some((id, good_side)) => out.push(PairMapping {
                pair_id: id,
                good_side,
                positive_name: p.pair.clone(),
                goal: p.goal.clone(),
            }),
            None => println!("  ⚠️  No pair named '{}' in daily.db; skipping.", p.pair),
        }
    }
    Ok(out)
}

fn load_checkout_days(
    conn: &Connection,
    pair_ids: &[i64],
    start_date: &str,
) -> Result<Vec<CheckoutEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, entry_date, mood FROM entry WHERE entry_date >= ?1 ORDER BY entry_date",
    )?;
    let rows = stmt.query_map([start_date], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;

    let mut entries: Vec<CheckoutEntry> = Vec::new();
    for row in rows {
        let (id, entry_date, mood) = row?;
        entries.push(CheckoutEntry {
            id,
            entry_date,
            mood,
            sides: HashMap::new(),
        });
    }

    if pair_ids.is_empty() {
        return Ok(entries);
    }

    // pair_ids are i64 values resolved from our own config, so formatting them
    // directly into the IN clause is safe (no user-controlled strings).
    let id_list = pair_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT entry_id, pair_id, side FROM entry_pair WHERE pair_id IN ({id_list})");
    let mut stmt2 = conn.prepare(&sql)?;
    let side_rows = stmt2.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let mut sides_by_entry: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for row in side_rows {
        let (entry_id, pair_id, side) = row?;
        sides_by_entry.entry(entry_id).or_default().push((pair_id, side));
    }

    for e in &mut entries {
        if let Some(v) = sides_by_entry.get(&e.id) {
            for (pair_id, side) in v {
                e.sides.insert(*pair_id, side.clone());
            }
        }
    }

    Ok(entries)
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

    fn seed_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE pair (id INTEGER PRIMARY KEY, positive_name TEXT NOT NULL,
                 good_side TEXT NOT NULL);
             CREATE TABLE entry (id INTEGER PRIMARY KEY, entry_date TEXT NOT NULL,
                 mood INTEGER);
             CREATE TABLE entry_pair (entry_id INTEGER, pair_id INTEGER, side TEXT,
                 PRIMARY KEY (entry_id, pair_id));
             INSERT INTO pair (id, positive_name, good_side) VALUES
                 (4, 'No Twitch', 'positive'), (2, 'No TV', 'positive');
             INSERT INTO entry (id, entry_date, mood) VALUES
                 (1, '2026-05-28', 5),
                 (2, '2026-05-29', NULL),
                 (3, '2026-05-30', 4);
             INSERT INTO entry_pair (entry_id, pair_id, side) VALUES
                 (1, 4, 'positive'),
                 (1, 2, 'negative'),
                 (3, 2, 'positive');
            ",
        )
        .unwrap();
        conn
    }

    #[test]
    fn resolve_mappings_resolves_known_skips_unknown() {
        let conn = seed_db();
        let cfg = vec![
            DailyPairMapping { pair: "No Twitch".into(), goal: "no-twitch".into() },
            DailyPairMapping { pair: "Nonexistent".into(), goal: "nope".into() },
        ];
        let resolved = resolve_mappings(&conn, &cfg).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].pair_id, 4);
        assert_eq!(resolved[0].good_side, "positive");
        assert_eq!(resolved[0].goal, "no-twitch");
    }

    #[test]
    fn load_checkout_days_filters_window_and_collects_sides() {
        let conn = seed_db();
        let entries = load_checkout_days(&conn, &[4, 2], "2026-05-29").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry_date, "2026-05-29");
        assert_eq!(entries[0].mood, None);
        let day30 = &entries[1];
        assert_eq!(day30.entry_date, "2026-05-30");
        assert_eq!(day30.mood, Some(4));
        assert_eq!(day30.sides.get(&2), Some(&"positive".to_string()));
        assert_eq!(day30.sides.get(&4), None);
    }
}
