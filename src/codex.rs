use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

use crate::common::{self, SessionEntry, Source, TResult};

const PAGE_LIMIT: u32 = 200;
const MAX_PAGES: usize = 20;
const MAX_UNMATCHED_MESSAGES: usize = 1000;

/// List Codex sessions through the official app-server protocol.
///
/// Codex has no one-shot JSON listing command, so we speak JSON-RPC to
/// `codex app-server --stdio`: `initialize` handshake, then `thread/list`,
/// following `nextCursor` until the list is exhausted.
pub fn list() -> TResult<Vec<SessionEntry>> {
    let mut server = AppServer::start()?;
    server.request(
        "initialize",
        json!({
            "clientInfo": {
                "name": "ocs",
                "title": "ocs session manager",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )?;
    server.notify("initialized")?;

    let mut entries = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut params = json!({ "limit": PAGE_LIMIT });
        if let Some(cursor) = &cursor {
            params["cursor"] = Value::String(cursor.clone());
        }
        let result = server.request("thread/list", params)?;
        if let Some(data) = result.get("data").and_then(Value::as_array) {
            entries.extend(data.iter().filter_map(to_entry));
        }
        cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    Ok(entries)
}

/// Permanently delete one session through the official CLI (`codex delete`).
///
/// The caller already confirmed, hence `--force`. Codex requires a UUID with
/// `--force`, and app-server thread ids are UUIDv7.
pub fn delete(id: &str) -> TResult<()> {
    let output = Command::new("codex")
        .args(["delete", id, "--force"])
        .output()
        .map_err(|e| format!("failed to run `codex delete`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(format!("codex delete failed: {detail}").into());
    }
    Ok(())
}

/// Map one `Thread` from the app-server into our listing shape.
/// Subagent threads (with a parent) are skipped, like opencode children.
fn to_entry(thread: &Value) -> Option<SessionEntry> {
    if thread
        .get("parentThreadId")
        .and_then(Value::as_str)
        .is_some()
    {
        return None;
    }
    let id = thread.get("id").and_then(Value::as_str)?.to_string();
    let name = thread
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let preview = thread.get("preview").and_then(Value::as_str).unwrap_or("");
    let title = if name.is_empty() { preview } else { name };
    Some(SessionEntry {
        source: Source::Codex,
        id,
        title: common::collapse_truncate(title, 300),
        cwd: thread
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created: thread
            .get("createdAt")
            .and_then(Value::as_i64)
            .and_then(common::ts_from_epoch_seconds),
        updated: thread
            .get("updatedAt")
            .and_then(Value::as_i64)
            .and_then(common::ts_from_epoch_seconds),
        path: thread
            .get("path")
            .and_then(Value::as_str)
            .map(PathBuf::from),
    })
}

/// Minimal JSON-RPC-over-stdio client for `codex app-server`.
///
/// The protocol skips the `"jsonrpc": "2.0"` field and uses
/// `{"id", "method", "params"}` requests with `{"id", "result" | "error"}`
/// responses (see codex-rs `app-server-protocol`).
struct AppServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl AppServer {
    fn start() -> TResult<Self> {
        let mut child = Command::new("codex")
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start `codex app-server`: {e}"))?;
        let stdin = child.stdin.take().ok_or("cannot open app-server stdin")?;
        let stdout = child.stdout.take().ok_or("cannot open app-server stdout")?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn send(&mut self, message: &Value) -> TResult<()> {
        writeln!(self.stdin, "{message}")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> TResult<Value> {
        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            return Err("codex app-server closed the connection".into());
        }
        let value = serde_json::from_str(line.trim())
            .map_err(|e| format!("invalid JSON from codex app-server: {e}"))?;
        Ok(value)
    }

    fn request(&mut self, method: &str, params: Value) -> TResult<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "id": id, "method": method, "params": params }))?;
        for _ in 0..MAX_UNMATCHED_MESSAGES {
            let message = self.read_message()?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("codex app-server `{method}` failed: {text}").into());
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(format!("codex app-server never answered `{method}`").into())
    }

    fn notify(&mut self, method: &str) -> TResult<()> {
        self.send(&json!({ "method": method }))
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
