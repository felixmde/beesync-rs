# daily_sync Design

**Date:** 2026-05-31
**Status:** Approved (pending implementation plan)

## Goal

Add a `daily_sync` module to beesync-rs that *sets* the value of specific
Beeminder goals for specific days based on the nightly "checkout" data in the
`daily` app (local project at `~/dev/daily`).

Unlike the existing count-style modules (which append `value = 1.0` per item),
`daily_sync` is value-setting and idempotent per day: each tracked habit pair
maps to a Beeminder goal, and each checked-out day gets exactly one datapoint
whose value reflects whether the habit's "good side" was reached.

## Source data model (daily)

`daily.db` is a SQLite database. Relevant tables:

- `entry` — one row per calendar day. `entry_date` (DATE, unique), `mood`
  (INTEGER 1–5, nullable), `note`, timestamps.
- `pair` — a habit pair. `id`, `positive_name`, `negative_name`, `good_side`
  (`"positive"` | `"negative"` — which side is the "good" outcome).
- `entry_pair` — many-to-many resolutions. `(entry_id, pair_id, side)` where
  `side` ∈ {`"positive"`, `"negative"`} records which side was reached that day.
  A pair that was not swiped that day has **no** `entry_pair` row.

A "good day" for a pair = `entry_pair.side == pair.good_side`.

## Configuration

New `[daily]` section in `config.toml`:

```toml
[daily]
db_path = "/home/felixm/dev/daily/daily.db"   # read-only SQLite path
lookback_days = 3                              # optional, default 3

[[daily.pairs]]
pair = "No Twitch"      # matches pair.positive_name in daily.db
goal = "no-twitch"      # Beeminder goal slug

[[daily.pairs]]
pair = "No news"
goal = "no-news"
```

New structs in `config.rs`:

```rust
pub struct DailyConfig {
    pub db_path: String,
    #[serde(default = "default_lookback_days")] // returns 3
    pub lookback_days: i64,
    pub pairs: Vec<DailyPairMapping>,
}

pub struct DailyPairMapping {
    pub pair: String,   // pair.positive_name
    pub goal: String,   // Beeminder goal slug
}
```

Wired into `Config` as `pub daily: Option<DailyConfig>` and dispatched from
`main.rs` via the existing `run_sync` helper:
`run_sync(|| daily_sync(daily_config, &beeminder)).await`.

## Module: `daily_sync.rs`

Signature, matching the existing convention:

```rust
pub async fn daily_sync(config: &DailyConfig, beeminder: &BeeminderClient) -> Result<()>
```

### Flow

1. Open `config.db_path` with `rusqlite` in read-only mode. The daily server
   does **not** need to be running.
2. For each configured mapping, resolve `pair` (positive_name) →
   `(pair_id, good_side)` from the `pair` table. If a name has no match, **log a
   warning and skip** that mapping (a rename in daily surfaces as a log line,
   not a crash).
3. Compute the window: `[today - lookback_days, today]` inclusive.
4. Query `entry` rows in the window. Left-join `entry_pair` filtered to the
   tracked pair_ids so that, per day per tracked pair, we know the resolved
   `side` (or that there was none).
5. For each `(goal, day)` pair, compute the value (below) and upsert.

### Value computation (pure function, unit-testable)

For a given day and tracked pair:

- If **no `entry` row** exists for the day → produce nothing; leave any existing
  Beeminder datapoint untouched.
- If the entry exists but `mood IS NULL` (partial / quick-logged day) → **skip
  with a warning**; do not touch Beeminder.
- Otherwise (a real checkout):
  - `entry_pair.side == good_side` → **1.0**
  - good side not reached, **or** no `entry_pair` row for that pair → **0.0**

Expose the side-comparison as a pure function,
`fn pair_value(side: Option<&str>, good_side: &str) -> f64`, so the core rule is
tested without a DB or network.

### Upsert / dedup

`requestid = daystamp` (e.g. `"20260530"`), scoped per goal (each goal has its
own datapoints). Beeminder treats `requestid` as an idempotency key.

Per goal:

1. Fetch existing datapoints (`get_datapoints`) and build a
   `daystamp → datapoint` map.
2. For each computed `(daystamp, value)`:
   - No existing datapoint → `create_datapoint`.
   - Existing datapoint, **same** value → skip (no API call).
   - Existing datapoint, **different** value → `update_datapoint` with the new
     value (cleaner than the delete+recreate used in `clean_view_sync`).

`CreateDatapoint` fields:
- `value`: computed value.
- `timestamp` / `daystamp`: derived from `entry_date`.
- `comment`: e.g. `"daily checkout: No Twitch ✓"` (value 1) / `"… ✗"` (value 0).
- `requestid`: the daystamp.

## Error handling

- Per-pair name mismatches: warn and skip, do not abort the module.
- The module is isolated by `run_sync`, so any error it returns is reported per
  module without stopping other syncs.
- DB opened read-only; missing `db_path` returns an error from the module only.

## Testing

- `pair_value` is a pure function — unit tests cover: good side reached,
  good side not reached, unresolved pair.
- SQLite query layer: test against a temp `daily.db` seeded with a couple of
  entries (including a mood-null partial day and a day with no checkout) to
  verify skip/untouched behavior.

## Out of scope (YAGNI)

- Mood → goal sync, composite "good day" scores, and pushing notes — explicitly
  not part of this iteration (per brainstorming).
- Mapping by `pair_id` instead of name — chose readability of names; renames are
  handled by the warn-and-skip path.
