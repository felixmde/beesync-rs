use anyhow::{bail, Context, Result};
use beeminder::{
    types::{CreateDatapoint, DatapointFull, UpdateDatapoint},
    BeeminderClient,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use time::{macros::format_description, Date, Duration};

fn default_reconcile_days() -> i64 {
    7
}

fn default_prefill_horizon_days() -> i64 {
    7
}

#[derive(Debug, Deserialize)]
pub struct DaylioConfig {
    pub source: String,
    #[serde(default = "default_reconcile_days")]
    pub reconcile_days: i64,
    #[serde(default = "default_prefill_horizon_days")]
    pub prefill_horizon_days: i64,
    #[serde(default)]
    pub apply: bool,
    pub mappings: Vec<DaylioMapping>,
}

#[derive(Debug, Deserialize)]
pub struct DaylioMapping {
    pub activity: String,
    pub beeminder_goal: String,
    pub present_value: f64,
    pub absent_value: f64,
    pub prefill_value: f64,
}

#[derive(Debug)]
struct DaylioDay {
    date: Date,
    activities: HashSet<String>,
}

#[derive(Debug)]
struct ExistingPoint {
    id: String,
    value: Option<f64>,
    comment: Option<String>,
    requestid: Option<String>,
    system: bool,
}

#[derive(Debug)]
struct Target {
    goal: String,
    date: Date,
    value: f64,
    comment: String,
    requestid: String,
    existing: Vec<ExistingPoint>,
}

/// True when `source` is a glob pattern rather than a plain path.
fn is_pattern(source: &str) -> bool {
    source.contains(['*', '?', '['])
}

impl DaylioConfig {
    /// Resolves `source` to a concrete file.
    ///
    /// Daylio stamps its exports with the export date, so a pattern such as
    /// `daylio_export_*.csv` picks up the newest export without renaming it.
    /// A `source` without wildcards is used verbatim.
    fn resolve_source(&self) -> Result<PathBuf> {
        if !is_pattern(&self.source) {
            return Ok(PathBuf::from(&self.source));
        }

        let matches = glob::glob(&self.source)
            .with_context(|| format!("invalid Daylio source pattern: {}", self.source))?;

        let mut newest: Option<(SystemTime, PathBuf)> = None;
        for entry in matches {
            let path = entry
                .with_context(|| format!("expanding Daylio source pattern: {}", self.source))?;
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            // Ties break on the path so the choice stays deterministic.
            let better = match &newest {
                Some((best_modified, best_path)) => (modified, &path) > (*best_modified, best_path),
                None => true,
            };
            if better {
                newest = Some((modified, path));
            }
        }

        match newest {
            Some((_, path)) => Ok(path),
            None => bail!("no file matches Daylio source pattern: {}", self.source),
        }
    }

    fn validate(&self, source: &Path) -> Result<()> {
        if source.extension().and_then(|value| value.to_str()) != Some("csv") {
            bail!("Daylio source must have a .csv extension")
        }
        let metadata = fs::metadata(source)
            .with_context(|| format!("reading Daylio source metadata at {}", source.display()))?;
        if !metadata.is_file() {
            bail!("Daylio source is not a regular file: {}", source.display())
        }
        if self.reconcile_days < 1 {
            bail!("daylio.reconcile_days must be at least 1")
        }
        if self.prefill_horizon_days < 0 {
            bail!("daylio.prefill_horizon_days cannot be negative")
        }
        if self.mappings.is_empty() {
            bail!("daylio.mappings must contain at least one mapping")
        }

        let mut goals = HashSet::new();
        for mapping in &self.mappings {
            if mapping.activity.trim().is_empty() || mapping.beeminder_goal.trim().is_empty() {
                bail!("Daylio activity and Beeminder goal names cannot be empty")
            }
            if !goals.insert(mapping.beeminder_goal.trim()) {
                bail!(
                    "duplicate Daylio mapping for Beeminder goal '{}'",
                    mapping.beeminder_goal
                )
            }
            if [
                mapping.present_value,
                mapping.absent_value,
                mapping.prefill_value,
            ]
            .iter()
            .any(|value| !value.is_finite())
            {
                bail!("Daylio mapping values must be finite")
            }
        }
        Ok(())
    }
}

fn normalized(value: &str) -> String {
    value.trim().to_lowercase()
}

fn parse_csv(path: &Path) -> Result<Vec<DaylioDay>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("opening Daylio CSV at {}", path.display()))?;
    let headers = reader.headers()?.clone();
    let names: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if index == 0 {
                name.trim_start_matches('\u{feff}').to_string()
            } else {
                name.to_string()
            }
        })
        .collect();

    let column = |required: &str| -> Result<usize> {
        let matches: Vec<usize> = names
            .iter()
            .enumerate()
            .filter_map(|(index, name)| (name == required).then_some(index))
            .collect();
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => bail!("Daylio CSV is missing required '{required}' column"),
            _ => bail!("Daylio CSV has duplicate '{required}' columns"),
        }
    };
    let date_column = column("full_date")?;
    let activities_column = column("activities")?;
    let format = format_description!("[year]-[month]-[day]");
    let mut days: HashMap<Date, HashSet<String>> = HashMap::new();

    for (row, record) in reader.records().enumerate() {
        let record = record.with_context(|| format!("parsing Daylio CSV row {}", row + 2))?;
        let date_text = record
            .get(date_column)
            .context("Daylio CSV row is missing full_date")?;
        let date = Date::parse(date_text, format)
            .with_context(|| format!("invalid Daylio date '{date_text}' on row {}", row + 2))?;
        let activities = record
            .get(activities_column)
            .context("Daylio CSV row is missing activities")?;
        let day = days.entry(date).or_default();
        for activity in activities.split(" | ").map(normalized) {
            if !activity.is_empty() {
                day.insert(activity);
            }
        }
    }

    let mut days: Vec<DaylioDay> = days
        .into_iter()
        .map(|(date, activities)| DaylioDay { date, activities })
        .collect();
    days.sort_by_key(|day| day.date);
    if days.is_empty() {
        bail!("Daylio CSV contains no entries")
    }
    Ok(days)
}

fn date_range(start: Date, end: Date) -> Vec<Date> {
    let mut dates = Vec::new();
    let mut date = start;
    while date <= end {
        dates.push(date);
        date += Duration::days(1);
    }
    dates
}

fn daystamp(date: Date) -> String {
    format!(
        "{:04}{:02}{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

fn target_dates(
    config: &DaylioConfig,
    days: &[DaylioDay],
    today: Date,
) -> Result<(Vec<Date>, Vec<Date>)> {
    let latest = days.last().context("Daylio CSV contains no entries")?.date;
    if latest > today {
        bail!("latest Daylio date {latest} is in the future")
    }
    if (today - latest).whole_days() > config.reconcile_days {
        bail!("latest Daylio date {latest} is too stale for the reconciliation window")
    }

    let reconcile_start = latest - Duration::days(config.reconcile_days - 1);
    let reconcile = date_range(reconcile_start, latest);
    let available: HashSet<Date> = days.iter().map(|day| day.date).collect();
    let missing: Vec<String> = reconcile
        .iter()
        .filter(|date| !available.contains(date))
        .map(ToString::to_string)
        .collect();
    if !missing.is_empty() {
        bail!(
            "Daylio reconciliation window has missing dates: {}",
            missing.join(", ")
        )
    }

    let prefill_start = latest + Duration::days(1);
    let prefill_end = today + Duration::days(config.prefill_horizon_days);
    let prefill = if prefill_start <= prefill_end {
        date_range(prefill_start, prefill_end)
    } else {
        Vec::new()
    };
    Ok((reconcile, prefill))
}

fn point(point: DatapointFull) -> ExistingPoint {
    ExistingPoint {
        id: point.id,
        value: point.value,
        comment: point.comment,
        requestid: point.requestid,
        system: point.is_dummy.unwrap_or(false) || point.is_initial.unwrap_or(false),
    }
}

fn plan(
    config: &DaylioConfig,
    days: &[DaylioDay],
    reconcile: &[Date],
    prefill: &[Date],
    mut existing: HashMap<String, HashMap<String, Vec<ExistingPoint>>>,
) -> Result<Vec<Target>> {
    let activities: HashMap<Date, &HashSet<String>> =
        days.iter().map(|day| (day.date, &day.activities)).collect();
    let mut targets = Vec::new();
    for mapping in &config.mappings {
        let goal = mapping.beeminder_goal.trim();
        for (date, value, state) in reconcile
            .iter()
            .map(|date| {
                let present = activities[date].contains(&normalized(&mapping.activity));
                (
                    *date,
                    if present {
                        mapping.present_value
                    } else {
                        mapping.absent_value
                    },
                    if present {
                        "present (authoritative)"
                    } else {
                        "absent (authoritative)"
                    },
                )
            })
            .chain(
                prefill
                    .iter()
                    .map(|date| (*date, mapping.prefill_value, "optimistic prefill")),
            )
        {
            let stamp = daystamp(date);
            let points = existing
                .entry(goal.to_string())
                .or_default()
                .remove(&stamp)
                .unwrap_or_default();
            if points.iter().any(|point| point.system) {
                bail!(
                    "system datapoint found on {} {date}; refusing the entire Daylio plan",
                    goal
                )
            }
            let requestid = format!("beesync-daylio-v1:{stamp}");
            if points
                .iter()
                .filter(|point| point.requestid.as_deref() == Some(&requestid))
                .count()
                > 1
            {
                bail!(
                    "multiple canonical Daylio datapoints found on {} {date}",
                    goal
                )
            }
            targets.push(Target {
                goal: goal.to_string(),
                date,
                value,
                comment: format!("beesync/daylio: {} {state}", mapping.activity.trim()),
                requestid,
                existing: points,
            });
        }
    }
    Ok(targets)
}

fn same_value(actual: Option<f64>, expected: f64) -> bool {
    actual.is_some_and(|actual| (actual - expected).abs() < 1e-9)
}

fn target_is_unchanged(target: &Target) -> bool {
    target.existing.len() == 1
        && target.existing[0].requestid.as_deref() == Some(&target.requestid)
        && same_value(target.existing[0].value, target.value)
        && target.existing[0].comment.as_deref() == Some(&target.comment)
}

fn target_action(target: &Target) -> &'static str {
    if target_is_unchanged(target) {
        return "✅ keep";
    }

    let canonical = target
        .existing
        .iter()
        .any(|point| point.requestid.as_deref() == Some(&target.requestid));
    match (canonical, target.existing.len()) {
        (true, 1) => "✏️ update",
        (true, _) => "🧹 update+prune",
        (false, 0) => "➕ create",
        (false, _) => "♻️ replace",
    }
}

fn existing_values(target: &Target) -> String {
    match target.existing.as_slice() {
        [] => "-".to_string(),
        points => points
            .iter()
            .map(|point| {
                point
                    .value
                    .map_or_else(|| "?".to_string(), |value| value.to_string())
            })
            .collect::<Vec<_>>()
            .join(","),
    }
}

fn format_preview_table<'a>(targets: impl IntoIterator<Item = &'a Target>) -> String {
    let headers = [
        "goal".to_string(),
        "date".to_string(),
        "target".to_string(),
        "existing".to_string(),
        "action".to_string(),
    ];
    let rows: Vec<[String; 5]> = targets
        .into_iter()
        .map(|target| {
            [
                target.goal.clone(),
                target.date.to_string(),
                target.value.to_string(),
                existing_values(target),
                target_action(target).to_string(),
            ]
        })
        .collect();

    let widths = (0..5)
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .chain(std::iter::once(headers[column].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let render = |row: &[String; 5]| {
        format!(
            "  {:<goal_width$} | {:<date_width$} | {:>target_width$} | {:>existing_width$} | {}\n",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            goal_width = widths[0],
            date_width = widths[1],
            target_width = widths[2],
            existing_width = widths[3],
        )
    };
    let separator = [
        "-".repeat(widths[0]),
        "-".repeat(widths[1]),
        "-".repeat(widths[2]),
        "-".repeat(widths[3]),
        "-".repeat(widths[4]),
    ];

    let mut output = render(&headers);
    output.push_str(&format!(
        "  {} | {} | {} | {} | {}\n",
        separator[0], separator[1], separator[2], separator[3], separator[4]
    ));
    for row in &rows {
        output.push_str(&render(row));
    }
    output
}

fn pluralized(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

fn format_run_summary(rows: usize, latest: Date, apply: bool) -> String {
    let (emoji, mode) = if apply {
        ("⚡", "APPLY")
    } else {
        ("🔍", "preview")
    };
    format!("  {emoji} {mode} · source: {rows} rows, latest {latest}")
}

fn format_apply_plan(targets: &[Target]) -> String {
    let mutations: Vec<&Target> = targets
        .iter()
        .filter(|target| !target_is_unchanged(target))
        .collect();
    let unchanged = targets.len() - mutations.len();

    if mutations.is_empty() {
        let goals = targets
            .iter()
            .map(|target| target.goal.as_str())
            .collect::<HashSet<_>>()
            .len();
        return format!(
            "  ✅ already in sync — {} checked across {}\n",
            pluralized(targets.len(), "datapoint", "datapoints"),
            pluralized(goals, "goal", "goals")
        );
    }

    let mut output = format_preview_table(mutations.iter().copied());
    output.push_str(&format!(
        "  {}{}\n",
        pluralized(mutations.len(), "change", "changes"),
        if unchanged == 0 {
            String::new()
        } else {
            format!(
                "; {} hidden",
                pluralized(unchanged, "unchanged target", "unchanged targets")
            )
        }
    ));
    output
}

async fn apply_target(client: &BeeminderClient, target: &Target) -> Result<()> {
    let canonical = target
        .existing
        .iter()
        .find(|point| point.requestid.as_deref() == Some(&target.requestid));
    let keeper_id = if let Some(canonical) = canonical {
        if !same_value(canonical.value, target.value)
            || canonical.comment.as_deref() != Some(&target.comment)
        {
            let update = UpdateDatapoint::new(canonical.id.clone())
                .with_value(target.value)
                .with_comment(&target.comment);
            client.update_datapoint(&target.goal, &update).await?;
        }
        canonical.id.clone()
    } else {
        let created = client
            .create_datapoint(
                &target.goal,
                &CreateDatapoint {
                    value: target.value,
                    timestamp: None,
                    daystamp: Some(daystamp(target.date)),
                    comment: Some(target.comment.clone()),
                    requestid: Some(target.requestid.clone()),
                },
            )
            .await?;
        created.id
    };

    for extra in target.existing.iter().filter(|point| point.id != keeper_id) {
        client.delete_datapoint(&target.goal, &extra.id).await?;
    }

    let stamp = daystamp(target.date);
    let current: Vec<DatapointFull> = client
        .get_datapoints_full(&target.goal, None, None, None, None)
        .await?
        .into_iter()
        .filter(|point| point.daystamp == stamp)
        .collect();
    if current.len() != 1
        || current[0].id != keeper_id
        || current[0].requestid.as_deref() != Some(&target.requestid)
        || !same_value(current[0].value, target.value)
        || current[0].is_dummy.unwrap_or(false)
        || current[0].is_initial.unwrap_or(false)
    {
        bail!(
            "post-write verification failed for {} {}",
            target.goal,
            target.date
        )
    }
    Ok(())
}

pub async fn daylio_sync(
    config: &DaylioConfig,
    client: &BeeminderClient,
    today: Date,
) -> Result<()> {
    println!("📔 daylio-sync");
    let source = config.resolve_source()?;
    config.validate(&source)?;
    if is_pattern(&config.source) {
        println!("  📄 {}", source.display());
    }
    let days = parse_csv(&source)?;
    let (reconcile, prefill) = target_dates(config, &days, today)?;

    let mut snapshots = HashMap::new();
    for mapping in &config.mappings {
        let goal = mapping.beeminder_goal.trim().to_string();
        if snapshots.contains_key(&goal) {
            continue;
        }
        let mut by_day: HashMap<String, Vec<ExistingPoint>> = HashMap::new();
        for datapoint in client
            .get_datapoints_full(&goal, None, None, None, None)
            .await
            .with_context(|| format!("fetching all datapoints for {goal}"))?
        {
            by_day
                .entry(datapoint.daystamp.clone())
                .or_default()
                .push(point(datapoint));
        }
        snapshots.insert(goal, by_day);
    }
    let targets = plan(config, &days, &reconcile, &prefill, snapshots)?;

    println!(
        "{}",
        format_run_summary(days.len(), days.last().unwrap().date, config.apply)
    );

    if !config.apply {
        print!("{}", format_preview_table(&targets));
        println!("  preview complete; set daylio.apply = true to apply all listed mutations");
        return Ok(());
    }

    print!("{}", format_apply_plan(&targets));
    let mutations: Vec<&Target> = targets
        .iter()
        .filter(|target| !target_is_unchanged(target))
        .collect();
    if mutations.is_empty() {
        return Ok(());
    }
    for target in &mutations {
        apply_target(client, target)
            .await
            .with_context(|| format!("applying {} {}", target.goal, target.date))?;
    }
    println!(
        "  ✅ applied and verified {}",
        pluralized(mutations.len(), "change", "changes")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(value: &str) -> Date {
        Date::parse(value, format_description!("[year]-[month]-[day]")).unwrap()
    }

    #[test]
    fn parses_real_report_shape_and_unions_rows() {
        let days = parse_csv(Path::new("tests/fixtures/daylio-report.csv")).unwrap();
        assert_eq!(days.len(), 2);
        assert!(days[0].activities.contains("reading"));
        assert!(days[0].activities.contains("stretching"));
    }

    #[test]
    fn reconciliation_and_prefill_ranges_are_calendar_dates() {
        let days: Vec<DaylioDay> = (0..7)
            .map(|offset| DaylioDay {
                date: date("2026-07-29") + Duration::days(offset),
                activities: HashSet::new(),
            })
            .collect();
        let config = DaylioConfig {
            source: "tests/fixtures/daylio-report.csv".into(),
            reconcile_days: 7,
            prefill_horizon_days: 7,
            apply: false,
            mappings: vec![],
        };
        let (reconcile, prefill) = target_dates(&config, &days, date("2026-08-06")).unwrap();
        assert_eq!(reconcile.len(), 7);
        assert_eq!(prefill.first(), Some(&date("2026-08-05")));
        assert_eq!(prefill.last(), Some(&date("2026-08-13")));
        assert_eq!(daystamp(date("2026-08-04")), "20260804");
    }

    #[test]
    fn missing_reconciliation_day_is_rejected() {
        let days = vec![
            DaylioDay {
                date: date("2026-08-02"),
                activities: HashSet::new(),
            },
            DaylioDay {
                date: date("2026-08-04"),
                activities: HashSet::new(),
            },
        ];
        let config = DaylioConfig {
            source: "tests/fixtures/daylio-report.csv".into(),
            reconcile_days: 3,
            prefill_horizon_days: 7,
            apply: false,
            mappings: vec![],
        };
        assert!(target_dates(&config, &days, date("2026-08-04"))
            .unwrap_err()
            .to_string()
            .contains("2026-08-03"));
    }

    #[test]
    fn plan_uses_trimmed_goal_for_existing_snapshot() {
        let target_date = date("2026-08-04");
        let days = vec![DaylioDay {
            date: target_date,
            activities: HashSet::new(),
        }];
        let config = DaylioConfig {
            source: "tests/fixtures/daylio-report.csv".into(),
            reconcile_days: 1,
            prefill_horizon_days: 0,
            apply: false,
            mappings: vec![DaylioMapping {
                activity: "Reading".into(),
                beeminder_goal: " goal ".into(),
                present_value: 1.0,
                absent_value: 0.0,
                prefill_value: 1.0,
            }],
        };
        let point = ExistingPoint {
            id: "existing".into(),
            value: Some(0.0),
            comment: None,
            requestid: None,
            system: false,
        };
        let existing = HashMap::from([(
            "goal".into(),
            HashMap::from([(daystamp(target_date), vec![point])]),
        )]);

        let targets = plan(&config, &days, &[target_date], &[], existing).unwrap();

        assert_eq!(targets[0].goal, "goal");
        assert_eq!(targets[0].existing.len(), 1);
        assert_eq!(targets[0].existing[0].id, "existing");
    }

    #[test]
    fn preview_table_aligns_columns_and_separates_header() {
        let targets = vec![
            Target {
                goal: "free".into(),
                date: date("2026-08-07"),
                value: 1.0,
                comment: "beesync/daylio: non-user present".into(),
                requestid: "canonical".into(),
                existing: vec![ExistingPoint {
                    id: "existing".into(),
                    value: Some(1.0),
                    comment: Some("beesync/daylio: non-user present".into()),
                    requestid: Some("canonical".into()),
                    system: false,
                }],
            },
            Target {
                goal: "free".into(),
                date: date("2026-08-10"),
                value: 1.0,
                comment: "beesync/daylio: non-user present".into(),
                requestid: "new".into(),
                existing: vec![],
            },
        ];

        assert_eq!(
            format_preview_table(&targets),
            concat!(
                "  goal | date       | target | existing | action\n",
                "  ---- | ---------- | ------ | -------- | --------\n",
                "  free | 2026-08-07 |      1 |        1 | ✅ keep\n",
                "  free | 2026-08-10 |      1 |        - | ➕ create\n"
            )
        );
    }

    #[test]
    fn preview_table_shows_existing_values_instead_of_point_counts() {
        let target = Target {
            goal: "clean-twitch".into(),
            date: date("2026-08-08"),
            value: 0.0,
            comment: "beesync/daylio: non-user absent (authoritative)".into(),
            requestid: "canonical".into(),
            existing: vec![ExistingPoint {
                id: "existing".into(),
                value: Some(0.0),
                comment: None,
                requestid: None,
                system: false,
            }],
        };

        let table = format_preview_table(&[target]);

        assert!(table.contains("clean-twitch | 2026-08-08 |      0 |        0 | ♻️ replace"));
    }

    #[test]
    fn apply_plan_hides_unchanged_targets() {
        let targets = vec![
            Target {
                goal: "free".into(),
                date: date("2026-08-07"),
                value: 1.0,
                comment: "beesync/daylio: non-user present".into(),
                requestid: "canonical".into(),
                existing: vec![ExistingPoint {
                    id: "existing".into(),
                    value: Some(1.0),
                    comment: Some("beesync/daylio: non-user present".into()),
                    requestid: Some("canonical".into()),
                    system: false,
                }],
            },
            Target {
                goal: "free".into(),
                date: date("2026-08-10"),
                value: 1.0,
                comment: "beesync/daylio: non-user present".into(),
                requestid: "new".into(),
                existing: vec![],
            },
        ];

        let output = format_apply_plan(&targets);

        assert!(!output.contains("2026-08-07"));
        assert!(output.contains("2026-08-10"));
        assert!(output.contains("1 change; 1 unchanged target hidden"));
    }

    #[test]
    fn apply_plan_collapses_when_everything_is_unchanged() {
        let targets = vec![Target {
            goal: "free".into(),
            date: date("2026-08-07"),
            value: 1.0,
            comment: "beesync/daylio: non-user present".into(),
            requestid: "canonical".into(),
            existing: vec![ExistingPoint {
                id: "existing".into(),
                value: Some(1.0),
                comment: Some("beesync/daylio: non-user present".into()),
                requestid: Some("canonical".into()),
                system: false,
            }],
        }];

        assert_eq!(
            format_apply_plan(&targets),
            "  ✅ already in sync — 1 datapoint checked across 1 goal\n"
        );
    }

    #[test]
    fn run_summary_combines_mode_and_source_with_a_mode_emoji() {
        assert_eq!(
            format_run_summary(694, date("2026-08-08"), true),
            "  ⚡ APPLY · source: 694 rows, latest 2026-08-08"
        );
        assert_eq!(
            format_run_summary(694, date("2026-08-08"), false),
            "  🔍 preview · source: 694 rows, latest 2026-08-08"
        );
    }

    fn config_with_source(source: &str) -> DaylioConfig {
        DaylioConfig {
            source: source.into(),
            reconcile_days: 7,
            prefill_horizon_days: 7,
            apply: false,
            mappings: vec![],
        }
    }

    /// Creates an empty directory under the system temp dir, unique to `label`.
    fn scratch_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beesync-daylio-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn source_without_wildcard_is_used_verbatim() {
        let config = config_with_source("/home/user/daylio_export.csv");
        assert_eq!(
            config.resolve_source().unwrap(),
            PathBuf::from("/home/user/daylio_export.csv")
        );
    }

    #[test]
    fn wildcard_source_resolves_to_the_most_recent_export() {
        let dir = scratch_dir("newest");
        // Written first, so it is the older file despite sorting last by name.
        fs::write(dir.join("export_b.csv"), "").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("export_a.csv"), "").unwrap();

        let config = config_with_source(dir.join("export_*.csv").to_str().unwrap());
        assert_eq!(config.resolve_source().unwrap(), dir.join("export_a.csv"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wildcard_source_without_matches_is_an_error() {
        let dir = scratch_dir("empty");
        let config = config_with_source(dir.join("export_*.csv").to_str().unwrap());
        let error = config.resolve_source().unwrap_err().to_string();
        assert!(error.contains("no file matches"), "{error}");

        fs::remove_dir_all(&dir).unwrap();
    }
}
