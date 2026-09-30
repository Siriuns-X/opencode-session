use serde::Deserialize;
use std::process::Command;

use crate::common::{self, SessionEntry, Source, TResult};

#[derive(Deserialize)]
struct SessionsResponse {
    data: Vec<Session>,
}

#[derive(Deserialize)]
struct SessionResponse {
    data: Session,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    title: String,
    #[serde(rename = "parentID", default)]
    parent_id: Option<String>,
    location: Location,
    time: Timestamps,
}

#[derive(Deserialize)]
struct Location {
    directory: String,
}

#[derive(Deserialize)]
struct Timestamps {
    created: i64,
    updated: i64,
}

fn run_opencode(args: &[&str]) -> TResult<String> {
    let output = Command::new("opencode").args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "opencode {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim(),
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn fetch_sessions() -> TResult<Vec<Session>> {
    let json = run_opencode(&["api", "get", "/api/session"])?;
    let response: SessionsResponse = serde_json::from_str(&json)?;
    Ok(response
        .data
        .into_iter()
        .filter(|s| s.parent_id.is_none())
        .collect())
}

fn fetch_session_by_id(id: &str) -> TResult<Session> {
    let path = format!("/api/session/{id}");
    let json = run_opencode(&["api", "get", path.as_str()])?;
    let response: SessionResponse = serde_json::from_str(&json)?;
    Ok(response.data)
}

fn to_entry(session: &Session) -> SessionEntry {
    SessionEntry {
        source: Source::Opencode,
        id: session.id.clone(),
        title: common::collapse_truncate(&session.title, 300),
        cwd: session.location.directory.clone(),
        created: common::ts_from_epoch_millis(session.time.created),
        updated: common::ts_from_epoch_millis(session.time.updated),
        path: None,
    }
}

pub fn list() -> TResult<Vec<SessionEntry>> {
    Ok(fetch_sessions()?.iter().map(to_entry).collect())
}

pub fn find(id: &str) -> TResult<SessionEntry> {
    Ok(to_entry(&fetch_session_by_id(id)?))
}

pub fn delete(id: &str) -> TResult<()> {
    run_opencode(&["session", "delete", id])?;
    Ok(())
}
