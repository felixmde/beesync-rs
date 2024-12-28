use crate::config::FocusmateSync;
use anyhow::{anyhow, Result};
use beeminder::{types::CreateDatapoint, BeeminderClient};
use focusmate::{FocusmateClient, Session};
use std::collections::HashSet;
use time::{Duration, OffsetDateTime};

fn find_matching_tags(tags: &[String], comment: &str) -> Vec<String> {
    tags.iter()
        .filter(|tag| comment.contains(&format!("#{tag}")))
        .cloned()
        .collect()
}

fn get_session_title(session: &Session) -> Result<String> {
    let Some(me) = session.users.first() else {
        return Err(anyhow!("Could not get me profile."));
    };

    let session_title = match &me.session_title {
        Some(session_title) => session_title,
        None => "",
    };

    Ok(session_title.to_string())
}

async fn session_to_datapoint(
    focusmate: &FocusmateClient,
    session: &Session,
) -> Result<CreateDatapoint> {
    let daystamp = format!(
        "{:04}{:02}{:02}",
        session.start_time.year(),
        session.start_time.month() as u8,
        session.start_time.day()
    );

    let formatted_time = format!(
        "{}, {:02}:{:02} (UTC)",
        session.start_time.weekday(),
        session.start_time.hour(),
        session.start_time.minute()
    );

    let session_title = get_session_title(session)?;
    let partner = match session.get_partner_profile(focusmate).await {
        Ok(partner) => partner.name,
        Err(_) => "unknown partner".to_string(),
    };
    let comment = format!(
        "{}, {} with {} for {} mins",
        formatted_time,
        session_title,
        partner,
        session.duration / 60000, // milliseconds to minutes
    );
    let dp = CreateDatapoint {
        value: 1.0,
        timestamp: Some(session.start_time),
        daystamp: Some(daystamp),
        comment: Some(comment),
        requestid: None,
    };

    Ok(dp)
}

pub async fn focusmate_sync(
    config: &FocusmateSync,
    focusmate: &FocusmateClient,
    beeminder: &BeeminderClient,
) -> Result<()> {
    println!("🤝 focusmate-sync");
    let goal = &config.goal_name;
    let most_recent_focusmate_dp = beeminder
        .get_datapoints(goal, Some("timestamp"), Some(1))
        .await?;
    let start = match most_recent_focusmate_dp.first() {
        Some(dp) if dp.value != 0.0 => dp.timestamp,
        _ => OffsetDateTime::UNIX_EPOCH,
    };
    let end = OffsetDateTime::now_utc() + Duration::days(1);
    let fm_sessions = focusmate.get_sessions(&start, &end).await?;

    // Get enough datapoints to check for duplicates
    let existing_dps = beeminder
        .get_datapoints(goal, Some("timestamp"), Some(fm_sessions.len() as u64))
        .await?;

    let existing_timestamps: HashSet<_> = existing_dps.iter().map(|dp| dp.timestamp).collect();

    let new_sessions: Vec<_> = fm_sessions
        .into_iter()
        .filter(focusmate::Session::completed)
        .filter(|session| !existing_timestamps.contains(&session.start_time))
        .collect();

    for session in new_sessions {
        let dp = session_to_datapoint(focusmate, &session).await?;
        beeminder.create_datapoint(goal, &dp).await?;
        assert!(dp.comment.is_some());
        if let Some(comment) = dp.comment.as_ref() {
            println!("  🆕 Created Focusmate datapoint: {comment}");

            let matching_tags = find_matching_tags(&config.auto_tags, comment);
            for tag in matching_tags {
                beeminder.create_datapoint(&tag, &dp).await?;
                println!("    📌 Created additional datapoint for goal: {tag}");
            }
        }
    }

    Ok(())
}
