use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_TRACE_DIR: &str = ".cache/polyrhythm";
const TRACE_FILE: &str = "trace.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceLevel {
    Info,
    Warn,
    Error,
}

impl TraceLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEvent {
    pub level: TraceLevel,
    pub event: String,
    pub message: String,
}

impl TraceEvent {
    pub fn info(event: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: TraceLevel::Info,
            event: event.into(),
            message: message.into(),
        }
    }

    pub fn warn(event: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: TraceLevel::Warn,
            event: event.into(),
            message: message.into(),
        }
    }

    pub fn error(event: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: TraceLevel::Error,
            event: event.into(),
            message: message.into(),
        }
    }
}

pub fn trace_path() -> PathBuf {
    trace_dir().join(TRACE_FILE)
}

pub fn write_event(event: TraceEvent) -> io::Result<()> {
    let path = trace_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", encode_event(&event))
}

pub fn tail(limit: usize) -> io::Result<Vec<String>> {
    let path = trace_path();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let lines: Vec<String> = BufReader::new(file).lines().collect::<io::Result<_>>()?;
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..].to_vec())
}

pub fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!(
            "command -v -- {} >/dev/null 2>&1",
            shell_escape(name)
        ))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn trace_dir() -> PathBuf {
    env::var_os("POLYRHYTHM_TRACE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(DEFAULT_TRACE_DIR)))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_TRACE_DIR))
}

fn encode_event(event: &TraceEvent) -> String {
    format!(
        "{{\"ts\":{},\"level\":\"{}\",\"event\":\"{}\",\"message\":\"{}\"}}",
        unix_millis(),
        event.level.as_str(),
        escape_json(&event.event),
        escape_json(&event.message)
    )
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn escape_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn shell_escape(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_encoding_escapes_message() {
        let encoded = encode_event(&TraceEvent::info("thing", "quote \" and slash \\"));
        assert!(encoded.contains("quote \\\" and slash \\\\"));
    }

    #[test]
    fn tail_missing_trace_is_empty() {
        let dir = env::temp_dir().join(format!("polyrhythm-missing-trace-{}", unix_millis()));
        env::set_var("POLYRHYTHM_TRACE_DIR", &dir);
        assert!(tail(10).unwrap().is_empty());
        env::remove_var("POLYRHYTHM_TRACE_DIR");
    }
}
