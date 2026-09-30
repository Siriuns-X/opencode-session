use chrono::{DateTime, Local, Utc};
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Result type shared by all modules.
pub type TResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Where a session comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Opencode,
    Claude,
    Codex,
}

impl Source {
    /// Short tag used in listings.
    pub fn tag(self) -> &'static str {
        match self {
            Source::Opencode => "oc",
            Source::Claude => "cc",
            Source::Codex => "cx",
        }
    }

    /// Human readable name used in warnings.
    pub fn name(self) -> &'static str {
        match self {
            Source::Opencode => "opencode",
            Source::Claude => "claude code",
            Source::Codex => "codex",
        }
    }

    /// Parse a source alias from the command line.
    pub fn parse(value: &str) -> Option<Source> {
        match value {
            "oc" | "opencode" => Some(Source::Opencode),
            "cc" | "claude" | "claude-code" => Some(Source::Claude),
            "cx" | "codex" => Some(Source::Codex),
            _ => None,
        }
    }
}

/// One session as shown by `ocs ls`, normalized across sources.
#[derive(Debug)]
pub struct SessionEntry {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub created: Option<DateTime<Local>>,
    pub updated: Option<DateTime<Local>>,
    /// Session file on disk, when the source stores sessions as files.
    pub path: Option<PathBuf>,
}

impl SessionEntry {
    fn sort_key(&self) -> (i64, i64) {
        (
            self.updated
                .map(|t| t.timestamp_millis())
                .unwrap_or(i64::MIN),
            self.created
                .map(|t| t.timestamp_millis())
                .unwrap_or(i64::MIN),
        )
    }
}

/// Oldest first, newest last.
pub fn sort_entries(entries: &mut [SessionEntry]) {
    entries.sort_by_key(SessionEntry::sort_key);
}

pub fn fmt_time(time: Option<DateTime<Local>>) -> String {
    time.map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// Collapse all whitespace into single spaces and truncate to `max` chars.
pub fn collapse_truncate(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let mut short: String = collapsed.chars().take(max.saturating_sub(1)).collect();
        short.push('…');
        short
    }
}

pub fn confirm(prompt: &str) -> TResult<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

pub fn home_dir() -> TResult<PathBuf> {
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home)),
        _ => Err("cannot determine the home directory: HOME is not set".into()),
    }
}

pub fn ts_from_epoch_millis(millis: i64) -> Option<DateTime<Local>> {
    DateTime::<Utc>::from_timestamp_millis(millis).map(|dt| dt.with_timezone(&Local))
}

pub fn ts_from_epoch_seconds(seconds: i64) -> Option<DateTime<Local>> {
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|dt| dt.with_timezone(&Local))
}

pub fn parse_rfc3339(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Local))
}

pub fn file_mtime(path: &Path) -> Option<DateTime<Local>> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(modified).with_timezone(&Local))
}
