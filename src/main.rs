mod claude;
mod codex;
mod common;
mod opencode;

use std::env;
use std::process::ExitCode;

use common::{SessionEntry, Source, TResult, sort_entries};

enum SessionSelector {
    Index(usize),
    Id(String),
}

const USAGE: &str = "\
Usage: ocs [SOURCE] <COMMAND>

Sources (optional, default: all):
\toc, opencode\t\tOnly OpenCode sessions
\tcc, claude\t\tOnly Claude Code sessions
\tcx, codex\t\tOnly Codex sessions

Commands:
\tls [-l|--long]\t\tList sessions, oldest first (newest last)
\trm <N>\t\t\tDelete the session at list number N
\trm --id <SESSION_ID>\tDelete a session by id

Examples:
\tocs ls\t\t\tEvery source
\tocs oc\t\t\tSame as `ocs oc ls`
\tocs cc ls -l\t\tLong listing of Claude Code sessions
\tocs cx rm 2\t\tDelete Codex session #2

Note: `rm` works where the vendor ships a delete interface; Claude Code
has no official per-session delete, so `ocs cc rm` refuses to guess.

Options:
\t-h, --help\t\tPrint this help";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> TResult<()> {
    let mut rest = args;
    let mut source = None;

    if let Some(first) = rest.first() {
        if first == "-h" || first == "--help" {
            println!("{USAGE}");
            return Ok(());
        }
        if let Some(parsed) = Source::parse(first) {
            source = Some(parsed);
            rest = &rest[1..];
        }
    }

    match rest.first().map(String::as_str) {
        Some("ls") => list(source, &rest[1..]),
        Some("rm") => remove(source, &rest[1..]),
        Some(flag) if flag.starts_with('-') => list(source, rest),
        None if source.is_some() => list(source, &[]),
        _ => usage(),
    }
}

fn usage() -> TResult<()> {
    eprintln!("{USAGE}");
    Err("invalid usage".into())
}

fn list(source: Option<Source>, rest: &[String]) -> TResult<()> {
    let is_long = rest.iter().any(|arg| arg == "-l" || arg == "--long");

    let mut entries = load(source)?;
    sort_entries(&mut entries);

    for (index, entry) in entries.iter().enumerate() {
        if is_long {
            println!(
                "{}\t[{}]\t{} -> {}",
                index,
                entry.source.tag(),
                common::fmt_time(entry.created),
                common::fmt_time(entry.updated),
            );
            println!("\t{}", entry.title);
            println!("\t{}", entry.id);
            println!("\t{}", entry.cwd);
            if let Some(path) = &entry.path {
                println!("\t{}", path.display());
            }
            println!();
        } else {
            println!(
                "{}\t[{}]\t{} -> {}\t{}",
                index,
                entry.source.tag(),
                common::fmt_time(entry.created),
                common::fmt_time(entry.updated),
                common::collapse_truncate(&entry.title, 80),
            );
        }
    }
    Ok(())
}

/// Load sessions for one source, or every source for the merged view.
///
/// In merged mode a broken source only costs that source: a warning is
/// printed and the other listings still work.
fn load(source: Option<Source>) -> TResult<Vec<SessionEntry>> {
    match source {
        Some(Source::Opencode) => opencode::list(),
        Some(Source::Claude) => claude::list(),
        Some(Source::Codex) => codex::list(),
        None => {
            let mut entries = Vec::new();
            for (source, result) in [
                (Source::Opencode, opencode::list()),
                (Source::Claude, claude::list()),
                (Source::Codex, codex::list()),
            ] {
                match result {
                    Ok(mut list) => entries.append(&mut list),
                    Err(error) => eprintln!("warning: no {} sessions: {error}", source.name()),
                }
            }
            Ok(entries)
        }
    }
}

fn parse_selector(rest: &[String]) -> TResult<SessionSelector> {
    match rest.first().map(String::as_str) {
        Some("--id") => {
            let id = rest.get(1).ok_or("missing session id")?;
            Ok(SessionSelector::Id(id.clone()))
        }
        Some(number) => {
            let index: usize = number
                .parse()
                .map_err(|_| format!("not a number: {number}"))?;
            Ok(SessionSelector::Index(index))
        }
        _ => Err("missing argument: <N> or --id <SESSION_ID>".into()),
    }
}

fn resolve(source: Option<Source>, selector: SessionSelector) -> TResult<SessionEntry> {
    match selector {
        SessionSelector::Index(index) => {
            let mut entries = load(source)?;
            sort_entries(&mut entries);
            if index >= entries.len() {
                return Err(format!("no session #{index}").into());
            }
            Ok(entries.remove(index))
        }
        SessionSelector::Id(id) => {
            let entries = load(source)?;
            if let Some(entry) = entries.into_iter().find(|entry| entry.id == id) {
                return Ok(entry);
            }
            // The opencode API can look up any session id directly, including
            // ones that are not part of the top-level listing.
            if (source.is_none() || source == Some(Source::Opencode))
                && let Ok(entry) = opencode::find(&id)
            {
                return Ok(entry);
            }
            Err(format!("no session with id {id}").into())
        }
    }
}

fn remove(source: Option<Source>, rest: &[String]) -> TResult<()> {
    let selector = parse_selector(rest)?;
    let target = resolve(source, selector)?;

    if target.source == Source::Claude {
        return Err(
            "claude code has no official per-session delete interface; refusing to delete transcript files by hand"
                .into(),
        );
    }

    println!("  id:      {}", target.id);
    println!("  title:   {}", target.title);
    println!("  dir:     {}", target.cwd);
    println!("  updated: {}", common::fmt_time(target.updated));
    if let Some(path) = &target.path {
        println!("  path:    {}", path.display());
    }

    if !common::confirm("Delete this session?")? {
        println!("cancelled");
        return Ok(());
    }

    match target.source {
        Source::Opencode => opencode::delete(&target.id)?,
        Source::Codex => codex::delete(&target.id)?,
        Source::Claude => unreachable!("claude refused above"),
    }
    println!("deleted {}", target.id);
    Ok(())
}
