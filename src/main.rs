use chrono::{DateTime, Local};
use serde::Deserialize;
use std::env;
use std::io::{self, Write};
use std::process::{Command, ExitCode};

#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

enum SessionSelector {
    Index(usize),
    Id(String),
}

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

fn fmt_ms_time(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

type TResult<T> = Result<T, Box<dyn std::error::Error>>;

fn parse_target(rest: &[String]) -> TResult<SessionSelector> {
    match rest.first().map(String::as_str) {
        Some("--id") => {
            let id = rest.get(1).ok_or("missing session id")?;
            Ok(SessionSelector::Id(id.clone()))
        }
        Some(n) => {
            let index: usize = n.parse().map_err(|_| format!("not a number: {n}"))?;
            Ok(SessionSelector::Index(index))
        }
        _ => Err("missing argument: <N> or --id <SESSION_ID>".into()),
    }
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
    let json = run_opencode(&["api", "get", &path])?;
    let response: SessionResponse = serde_json::from_str(&json)?;
    Ok(response.data)
}

fn confirm(prompt: &str) -> TResult<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}

fn delete_session(id: &str) -> TResult<()> {
    run_opencode(&["session", "delete", id])?;
    Ok(())
}

fn resolve_session(target: SessionSelector) -> TResult<Session> {
    match target {
        SessionSelector::Id(id) => fetch_session_by_id(&id),
        SessionSelector::Index(n) => {
            let mut sessions = fetch_sessions()?;
            if n >= sessions.len() {
                return Err(format!("no session #{n}").into());
            }
            Ok(sessions.remove(n))
        }
    }
}

const USAGE: &str = "\
Usage: ocs <COMMAND>
\nCommand:
\tls [-l|--long]\t\tList top-level sessions
\trm <N>\t\t\tDelete the session at list number N
\trm --id <SESSION_ID>\tDelete a session by id
\nOptions:
\t-h, --help\t\tPrint this help";

fn main() -> ExitCode {
    reset_sigpipe();
    let args: Vec<String> = env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("ls") => list_sessions(&args[1..]),
        Some("rm") => remove_session(&args[1..]),
        Some("-h" | "--help") => help(),
        _ => usage(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> TResult<()> {
    eprintln!("{USAGE}");
    Err("invalid usage".into())
}

fn help() -> TResult<()> {
    println!("{USAGE}");
    Ok(())
}

fn list_sessions(rest: &[String]) -> TResult<()> {
    let is_long = rest.iter().any(|a| a == "-l" || a == "--long");

    let roots = fetch_sessions()?;

    for (i, s) in roots.iter().enumerate() {
        if !is_long {
            println!(
                "{}\t{} -> {}\t{}",
                i,
                fmt_ms_time(s.time.created),
                fmt_ms_time(s.time.updated),
                s.title
            );
        } else {
            println!(
                "{}\t{} -> {}\n\t{}\n\t{}\n\t{}\n",
                i,
                fmt_ms_time(s.time.created),
                fmt_ms_time(s.time.updated),
                s.title,
                s.id,
                s.location.directory,
            )
        }
    }
    Ok(())
}

fn remove_session(rest: &[String]) -> TResult<()> {
    let target = parse_target(rest)?;
    let session = resolve_session(target)?;

    println!("  id:      {}", session.id);
    println!("  title:   {}", session.title);
    println!("  dir:     {}", session.location.directory);
    println!("  updated: {}", fmt_ms_time(session.time.updated));

    if !confirm("Delete this session?")? {
        println!("cancelled");
        return Ok(());
    }
    delete_session(&session.id)?;
    println!("deleted {}", session.id);
    Ok(())
}
