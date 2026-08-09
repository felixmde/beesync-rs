use anyhow::{bail, Context, Result};
use beeminder::{
    types::{CreateDatapoint, DatapointFull, UpdateDatapoint},
    BeeminderClient,
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
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

impl DaylioConfig {
    fn validate(&self) -> Result<()> {
        let path = Path::new(&self.source);
        if path.extension().and_then(|value| value.to_str()) != Some("csv") {
            bail!("Daylio source must have a .csv extension")
        }
        let metadata = fs::metadata(path)
            .with_context(|| format!("reading Daylio source metadata at {}", self.source))?;
        if !metadata.is_file() {
            bail!("Daylio source is not a regular file: {}", self.source)
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

fn parse_csv(path: &str) -> Result<Vec<DaylioDay>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .with_context(|| format!("opening Daylio CSV at {path}"))?;
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
    config.validate()?;
    let days = parse_csv(&config.source)?;
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
        "  source: {} rows, latest {}",
        days.len(),
        days.last().unwrap().date
    );
    println!("  mode: {}", if config.apply { "APPLY" } else { "preview" });
    println!("  goal | date | target | existing | action");
    for target in &targets {
        let canonical = target
            .existing
            .iter()
            .any(|point| point.requestid.as_deref() == Some(&target.requestid));
        let action = match (canonical, target.existing.len()) {
            (true, 1)
                if same_value(target.existing[0].value, target.value)
                    && target.existing[0].comment.as_deref() == Some(&target.comment) =>
            {
                "unchanged"
            }
            (true, 1) => "update",
            (true, _) => "update/delete extras",
            (false, 0) => "create",
            (false, _) => "create/delete extras",
        };
        println!(
            "  {} | {} | {} | {} | {action}",
            target.goal,
            target.date,
            target.value,
            target.existing.len()
        );
    }

    if !config.apply {
        println!("  preview complete; set daylio.apply = true to apply all listed mutations");
        return Ok(());
    }
    for target in &targets {
        apply_target(client, target)
            .await
            .with_context(|| format!("applying {} {}", target.goal, target.date))?;
    }
    println!("  verified {} goal/date targets", targets.len());
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
        let days = parse_csv("tests/fixtures/daylio-report.csv").unwrap();
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
}
