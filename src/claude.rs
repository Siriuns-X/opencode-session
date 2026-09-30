use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::common::{self, SessionEntry, Source, TResult};

/// How far into a transcript we look for metadata and a usable title.
const MAX_SCAN_LINES: usize = 150;

fn config_dir() -> TResult<PathBuf> {
    if let Some(dir) = env::var_os("CLAUDE_CONFIG_DIR")
        && !dir.is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    Ok(common::home_dir()?.join(".claude"))
}

/// List top-level Claude Code sessions by scanning transcript files.
///
/// Claude Code has no official listing API. Transcripts live in
/// `<CLAUDE_CONFIG_DIR>/projects/<project>/<session-id>.jsonl`, where every
/// line is one event. The on-disk format is internal and may change between
/// releases, so every field lookup here is best-effort with fallbacks.
pub fn list() -> TResult<Vec<SessionEntry>> {
    let projects = config_dir()?.join("projects");
    if !projects.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for project in fs::read_dir(&projects)? {
        let Ok(project) = project else { continue };
        if !project.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files {
            let Ok(file) = file else { continue };
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // Subagent transcripts live next to session transcripts.
            if stem.starts_with("agent-") {
                continue;
            }
            if let Some(entry) = parse_session(&path) {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

fn parse_session(path: &Path) -> Option<SessionEntry> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut parsed_any = false;
    let mut created = None;
    let mut cwd = String::new();
    let mut session_id: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut first_user: Option<String> = None;

    for line in reader.lines().take(MAX_SCAN_LINES) {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        parsed_any = true;

        if created.is_none() {
            created = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(common::parse_rfc3339);
        }
        if cwd.is_empty()
            && let Some(dir) = value.get("cwd").and_then(Value::as_str)
        {
            cwd = dir.to_string();
        }
        if session_id.is_none() {
            session_id = value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if value.get("type").and_then(Value::as_str) == Some("summary")
            && let Some(text) = value.get("summary").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            summary = Some(text.to_string());
        }
        if first_user.is_none()
            && value.get("type").and_then(Value::as_str) == Some("user")
            && value.get("isSidechain").and_then(Value::as_bool) != Some(true)
            && value.get("isMeta").and_then(Value::as_bool) != Some(true)
            && let Some(text) = user_text(&value)
        {
            let text = text.trim();
            // Meta entries such as `<command-name>` are not real prompts.
            if !text.is_empty() && !text.starts_with('<') {
                first_user = Some(text.to_string());
            }
        }
    }

    if !parsed_any {
        return None;
    }

    let id = session_id.unwrap_or_else(|| {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    if id.is_empty() {
        return None;
    }

    let title = summary.or(first_user).unwrap_or_else(|| id.clone());
    Some(SessionEntry {
        source: Source::Claude,
        id,
        title: common::collapse_truncate(&title, 300),
        cwd,
        created,
        updated: common::file_mtime(path).or(created),
        path: Some(path.to_path_buf()),
    })
}

/// Extract plain text from a `type: "user"` entry, ignoring tool results.
fn user_text(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    let content = message.get("content")?;
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        }
        _ => None,
    }
}
