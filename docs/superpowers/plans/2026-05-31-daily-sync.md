# daily_sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `daily_sync` module that reads the `daily` app's SQLite checkout data and *sets* (upserts) one Beeminder datapoint per checked-out day for each configured habit pair.

**Architecture:** A new `src/daily_sync.rs` module following the existing `{service}_sync.rs` convention. It reads `daily.db` read-only via `rusqlite`, resolves configured pair names to `(pair_id, good_side)`, computes a 1.0/0.0 value per checked-out day per pair, and idempotently upserts datapoints (create / update-if-different / skip-if-equal). Pure decision functions are unit-tested; the SQLite read is tested against an in-memory DB.

**Tech Stack:** Rust, `rusqlite` (bundled), `beeminder` crate (`create_datapoint` / `update_datapoint` / `get_datapoints`), `time`, `serde`, `anyhow`.

**Reference spec:** `docs/superpowers/specs/2026-05-31-daily-sync-design.md`

---

## File Structure

- **Create** `src/daily_sync.rs` — the whole module: config structs, pure helpers, DB read layer, orchestrator, tests.
- **Modify** `src/main.rs` — add `mod daily_sync;` and a dispatch block.
- **Modify** `src/config.rs` — add `daily: Option<DailyConfig>` field + import.
- **Modify** `Cargo.toml` — add `rusqlite`.

Key types/functions defined in `daily_sync.rs` (referenced across tasks — names are fixed here):
- `DailyConfig { db_path: String, lookback_days: i64, pairs: Vec<DailyPairMapping> }`
- `DailyPairMapping { pair: String, goal: String }`
- `fn default_lookback_days() -> i64` → `3`
- `enum UpsertAction { Skip, Create, Update(String) }`
- `fn pair_value(side: Option<&str>, good_side: &str) -> f64`
- `fn daystamp_from_date(date: &str) -> String`
- `fn decide_upsert(existing: Option<(&str, f64)>, value: f64) -> UpsertAction`
- `struct PairMapping { pair_id: i64, good_side: String, positive_name: String, goal: String }`
- `struct CheckoutEntry { id: i64, entry_date: String, mood: Option<i64>, sides: HashMap<i64, String> }`
- `fn open_db(path: &str) -> Result<Connection>`
- `fn resolve_mappings(conn: &Connection, pairs: &[DailyPairMapping]) -> Result<Vec<PairMapping>>`
- `fn load_checkout_days(conn: &Connection, pair_ids: &[i64], start_date: &str) -> Result<Vec<CheckoutEntry>>`
- `async fn daily_sync(config: &DailyConfig, beeminder: &BeeminderClient) -> Result<()>`

---

## Task 1: Add the rusqlite dependency

**Files:**
- Modify: `Cargo.toml:19` (end of `[dependencies]`)

- [ ] **Step 1: Add the dependency**

Add this line at the end of the `[dependencies]` block in `Cargo.toml`:

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

(`bundled` compiles SQLite in-tree, so there's no system-library requirement and the in-memory test DB works everywhere.)

- [ ] **Step 2: Verify it resolves and builds**

Run: `cargo build`
Expected: builds successfully (downloads/compiles `rusqlite` and `libsqlite3-sys`).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add rusqlite dependency for daily_sync"
```

---

## Task 2: Create daily_sync module with config structs and pure helpers (TDD)

**Files:**
- Create: `src/daily_sync.rs`
- Modify: `src/main.rs:1-11` (add `mod daily_sync;`)
- Test: inline `#[cfg(test)] mod tests` in `src/daily_sync.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/daily_sync.rs` with the config structs, helper stubs, and tests:

```rust
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
```

Note: `CreateDatapoint`, `UpdateDatapoint`, `OpenFlags`, `OptionalExtension`, `HashMap`, `Duration`, `OffsetDateTime`, `UtcOffset`, `Connection` are imported now but used in later tasks. To avoid `unused_import` warnings failing nothing (warnings don't fail the build), this is acceptable; they are all consumed by Task 4 and Task 5.

- [ ] **Step 2: Register the module**

In `src/main.rs`, add the module declaration alphabetically among the existing `mod` lines (after `mod config;` on line 7):

```rust
mod daily_sync;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test daily_sync`
Expected: 7 tests pass (`pair_value_*`, `daystamp_strips_dashes`, `decide_upsert_*`).

- [ ] **Step 4: Commit**

```bash
git add src/daily_sync.rs src/main.rs
git commit -m "Add daily_sync module skeleton with pure helpers and tests"
```

---

## Task 3: Wire DailyConfig into config loading and main dispatch

**Files:**
- Modify: `src/config.rs:1-21`
- Modify: `src/main.rs:50-54`

- [ ] **Step 1: Import and add the config field**

In `src/config.rs`, add the import after line 2 (`use crate::clean_view_sync::CleanViewConfig;`):

```rust
use crate::daily_sync::DailyConfig;
```

Then add a field to the `Config` struct (after the `clean_view` field, line 17):

```rust
    pub daily: Option<DailyConfig>,
```

- [ ] **Step 2: Add the dispatch block in main.rs**

In `src/main.rs`, add this block after the `github` block (after line 52), before `Ok(())`:

```rust
    if let Some(daily_config) = config.daily {
        run_sync(|| daily_sync::daily_sync(&daily_config, &bee_client)).await;
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles successfully (the stub `daily_sync` returns `Ok(())`).

- [ ] **Step 4: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "Wire daily config into loading and dispatch"
```

---

## Task 4: Implement the SQLite read layer (TDD)

**Files:**
- Modify: `src/daily_sync.rs` (add structs + three functions + tests)

- [ ] **Step 1: Write the failing test**

Add these tests to the `#[cfg(test)] mod tests` block in `src/daily_sync.rs` (inside the existing module, after the last test):

```rust
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
                 (1, '2026-05-28', 5),   -- full checkout
                 (2, '2026-05-29', NULL),-- partial day (no mood)
                 (3, '2026-05-30', 4);   -- full checkout, pair 4 unresolved
             INSERT INTO entry_pair (entry_id, pair_id, side) VALUES
                 (1, 4, 'positive'),  -- good side reached
                 (1, 2, 'negative'),  -- wrong side
                 (3, 2, 'positive');  -- entry 3 only resolved pair 2
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
        // 2026-05-28 is before the window start, so excluded.
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry_date, "2026-05-29");
        assert_eq!(entries[0].mood, None);
        let day30 = &entries[1];
        assert_eq!(day30.entry_date, "2026-05-30");
        assert_eq!(day30.mood, Some(4));
        assert_eq!(day30.sides.get(&2), Some(&"positive".to_string()));
        assert_eq!(day30.sides.get(&4), None); // pair 4 unresolved on the 30th
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test daily_sync`
Expected: FAIL to compile — `resolve_mappings`, `load_checkout_days`, `PairMapping`, `CheckoutEntry` not found.

- [ ] **Step 3: Implement the structs and functions**

Add to `src/daily_sync.rs` (after `decide_upsert`, before the stub `daily_sync`):

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test daily_sync`
Expected: all tests pass (7 pure + `resolve_mappings_resolves_known_skips_unknown` + `load_checkout_days_filters_window_and_collects_sides`).

- [ ] **Step 5: Commit**

```bash
git add src/daily_sync.rs
git commit -m "Add SQLite read layer for daily_sync with in-memory tests"
```

---

## Task 5: Implement the daily_sync orchestrator

**Files:**
- Modify: `src/daily_sync.rs` (replace the stub `daily_sync` body)

- [ ] **Step 1: Replace the stub orchestrator**

Replace the entire stub function:

```rust
pub async fn daily_sync(_config: &DailyConfig, _beeminder: &BeeminderClient) -> Result<()> {
    Ok(())
}
```

with the full implementation:

```rust
pub async fn daily_sync(config: &DailyConfig, beeminder: &BeeminderClient) -> Result<()> {
    println!("📔 daily-sync");

    let conn = open_db(&config.db_path)?;
    let mappings = resolve_mappings(&conn, &config.pairs)?;
    if mappings.is_empty() {
        println!("  🫙 No pairs resolved; nothing to sync.");
        return Ok(());
    }

    let offset = UtcOffset::current_local_offset()?;
    let today = OffsetDateTime::now_utc().to_offset(offset).date();
    let start = today - Duration::days(config.lookback_days);
    let start_str = format!(
        "{:04}-{:02}-{:02}",
        start.year(),
        start.month() as u8,
        start.day()
    );

    let pair_ids: Vec<i64> = mappings.iter().map(|m| m.pair_id).collect();
    let entries = load_checkout_days(&conn, &pair_ids, &start_str)?;

    // Pre-fetch existing datapoints once per distinct goal.
    let mut existing_by_goal: HashMap<String, Vec<beeminder::types::Datapoint>> = HashMap::new();
    for m in &mappings {
        if !existing_by_goal.contains_key(&m.goal) {
            let dps = beeminder
                .get_datapoints(&m.goal, None, Some(100), None, None)
                .await?;
            existing_by_goal.insert(m.goal.clone(), dps);
        }
    }

    for entry in &entries {
        if entry.mood.is_none() {
            println!(
                "  ⚠️  Skipping partial day {} (no mood / not fully checked out).",
                entry.entry_date
            );
            continue;
        }

        let daystamp = daystamp_from_date(&entry.entry_date);

        for m in &mappings {
            let side = entry.sides.get(&m.pair_id).map(String::as_str);
            let value = pair_value(side, &m.good_side);
            let mark = if value >= 0.5 { "✓" } else { "✗" };
            let comment = format!("daily: {} {mark}", m.positive_name);

            let existing = existing_by_goal
                .get(&m.goal)
                .and_then(|dps| dps.iter().find(|dp| dp.daystamp == daystamp));
            let action = decide_upsert(existing.map(|dp| (dp.id.as_str(), dp.value)), value);

            match action {
                UpsertAction::Skip => {}
                UpsertAction::Create => {
                    let dp = CreateDatapoint {
                        value,
                        timestamp: None,
                        daystamp: Some(daystamp.clone()),
                        comment: Some(comment.clone()),
                        requestid: Some(daystamp.clone()),
                    };
                    beeminder.create_datapoint(&m.goal, &dp).await?;
                    println!("  🆕 {} {daystamp} = {value} ({})", m.goal, m.positive_name);
                }
                UpsertAction::Update(id) => {
                    let update = UpdateDatapoint::new(id)
                        .with_value(value)
                        .with_comment(&comment);
                    beeminder.update_datapoint(&m.goal, &update).await?;
                    println!("  🔁 {} {daystamp} -> {value} ({})", m.goal, m.positive_name);
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles cleanly**

Run: `cargo clippy`
Expected: compiles; no errors. (All previously "unused" imports — `CreateDatapoint`, `UpdateDatapoint`, `time::*` — are now used.)

- [ ] **Step 3: Run the full test suite**

Run: `cargo test`
Expected: all daily_sync tests pass; build succeeds.

- [ ] **Step 4: Manual end-to-end verification**

Add a `[daily]` section to your local `config.toml` pointing at the real DB and a test goal, e.g.:

```toml
[daily]
db_path = "/home/felixm/dev/daily/daily.db"
lookback_days = 3

[[daily.pairs]]
pair = "No Twitch"
goal = "no-twitch"
```

Run: `cargo run`
Expected output includes the `📔 daily-sync` header, then per-day `🆕`/`🔁`/(silent skip) lines for the configured goal, and finally `  ✅ completed successfully`. Re-running immediately should produce no `🆕`/`🔁` lines (all values already correct → skipped), confirming idempotency.

If you don't have a spare Beeminder goal, verify against a throwaway goal slug or confirm the dry behavior by temporarily logging the computed `(goal, daystamp, value)` before the API calls.

- [ ] **Step 5: Commit**

```bash
git add src/daily_sync.rs
git commit -m "Implement daily_sync orchestrator with idempotent upsert"
```

---

## Final verification

- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy` — no warnings introduced
- [ ] `cargo build --release` — release build succeeds
- [ ] Manual run shows create on first run, skip on second run for unchanged days, and update when a checkout value changes
- [ ] Update `CLAUDE.md`'s sync-module list and `README.md`/`config.toml` example to mention `daily_sync` (documentation parity with existing modules)
