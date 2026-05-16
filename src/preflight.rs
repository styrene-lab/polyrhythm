use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::process::PROCESS_NAMES;
use crate::td50_mapper::{mapped_notes_from_midimap, DRS_EMITTED_NOTES};
use crate::trace::command_exists;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

impl CheckResult {
    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Ok,
            detail: detail.into(),
        }
    }

    fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightConfig {
    pub required_commands: Vec<String>,
    pub mapper: PathBuf,
    pub kit: PathBuf,
    pub midimap: PathBuf,
    pub midi_device_name: String,
    pub monitor_sinks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    pub checks: Vec<CheckResult>,
    pub midi_client: Option<u32>,
}

impl PreflightConfig {
    pub fn drs_default(home: &Path, repo: &Path) -> Self {
        Self {
            required_commands: vec![
                "drumgizmo".to_string(),
                "pw-link".to_string(),
                "aconnect".to_string(),
                "pactl".to_string(),
                "timeout".to_string(),
            ],
            mapper: home
                .join(".local")
                .join("bin")
                .join("td50-drumgizmo-hihat-mapper"),
            kit: home
                .join(".local")
                .join("share")
                .join("drumgizmo")
                .join("kits")
                .join("DRSKit")
                .join("DRSKit_full.xml"),
            midimap: repo.join("assets/drumgizmo/DRSKit/Midimap_td50.xml"),
            midi_device_name: "U2MIDI Pro".to_string(),
            monitor_sinks: vec!["alsa_output.pci-0000_0e_00.4.analog-stereo".to_string()],
        }
    }
}

pub fn run(config: &PreflightConfig) -> PreflightReport {
    let mut checks = Vec::new();

    for command in &config.required_commands {
        if command_exists(command) {
            checks.push(CheckResult::ok(
                format!("command:{command}"),
                "found in PATH",
            ));
        } else {
            checks.push(CheckResult::fail(
                format!("command:{command}"),
                "not found in PATH",
            ));
        }
    }

    checks.push(executable_check("mapper", &config.mapper));
    checks.push(file_check("kit", &config.kit));
    checks.push(file_check("midimap", &config.midimap));
    checks.push(midimap_coverage_check(&config.midimap));

    let midi_client = detect_midi_client(&config.midi_device_name, Duration::from_secs(2));
    match midi_client {
        Some(client) => checks.push(CheckResult::ok(
            "midi-client",
            format!("{} client {client}", config.midi_device_name),
        )),
        None => checks.push(CheckResult::fail(
            "midi-client",
            format!(
                "{} not visible via bounded aconnect",
                config.midi_device_name
            ),
        )),
    }

    match list_sinks(Duration::from_secs(2)) {
        Ok(sinks) => {
            for sink in &config.monitor_sinks {
                if sinks.iter().any(|line| line.contains(sink)) {
                    checks.push(CheckResult::ok("monitor-sink", sink));
                } else {
                    checks.push(CheckResult::fail(
                        "monitor-sink",
                        format!("missing: {sink}"),
                    ));
                }
            }
        }
        Err(err) => checks.push(CheckResult::fail("monitor-sink", err)),
    }

    checks.extend(forbidden_process_checks());

    PreflightReport {
        checks,
        midi_client,
    }
}

pub fn has_failures(checks: &[CheckResult]) -> bool {
    checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Fail))
}

pub fn detect_midi_client(device_name: &str, timeout: Duration) -> Option<u32> {
    let output = Command::new("timeout")
        .arg(format_duration(timeout))
        .arg("aconnect")
        .arg("-l")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_aconnect_client(&String::from_utf8_lossy(&output.stdout), device_name)
}

pub fn parse_aconnect_client(output: &str, device_name: &str) -> Option<u32> {
    for line in output.lines() {
        let line = line.trim_start();
        if !line.starts_with("client ") || !line.contains(device_name) {
            continue;
        }
        let rest = line.strip_prefix("client ")?;
        let client = rest.split(':').next()?;
        if let Ok(client) = client.trim().parse::<u32>() {
            return Some(client);
        }
    }
    None
}

fn file_check(name: &str, path: &Path) -> CheckResult {
    if path.exists() {
        CheckResult::ok(name, path.display().to_string())
    } else {
        CheckResult::fail(name, format!("missing: {}", path.display()))
    }
}

fn executable_check(name: &str, path: &Path) -> CheckResult {
    let Ok(meta) = fs::metadata(path) else {
        return CheckResult::fail(name, format!("missing: {}", path.display()));
    };
    if meta.permissions().mode() & 0o111 != 0 {
        CheckResult::ok(name, format!("executable: {}", path.display()))
    } else {
        CheckResult::fail(name, format!("not executable: {}", path.display()))
    }
}

fn midimap_coverage_check(path: &Path) -> CheckResult {
    let Ok(xml) = std::fs::read_to_string(path) else {
        return CheckResult::fail(
            "midimap-coverage",
            format!("cannot read: {}", path.display()),
        );
    };
    let mapped = mapped_notes_from_midimap(&xml);
    let missing: Vec<u8> = DRS_EMITTED_NOTES
        .iter()
        .copied()
        .filter(|note| !mapped.contains(note))
        .collect();
    if missing.is_empty() {
        CheckResult::ok("midimap-coverage", "all emitted DRS notes are mapped")
    } else {
        CheckResult::fail("midimap-coverage", format!("missing notes: {missing:?}"))
    }
}

fn list_sinks(timeout: Duration) -> Result<Vec<String>, String> {
    let output = Command::new("timeout")
        .arg(format_duration(timeout))
        .arg("pactl")
        .arg("list")
        .arg("short")
        .arg("sinks")
        .output()
        .map_err(|err| format!("failed to run pactl: {err}"))?;
    if !output.status.success() {
        return Err(format!("pactl failed with status {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(ToString::to_string)
        .collect())
}

fn forbidden_process_checks() -> Vec<CheckResult> {
    PROCESS_NAMES
        .iter()
        .filter(|name| **name == "sfizz_jack")
        .map(|name| {
            if process_running(name) {
                CheckResult::fail("forbidden-process", format!("{name} is running"))
            } else {
                CheckResult::ok("forbidden-process", format!("{name} is not running"))
            }
        })
        .collect()
}

fn process_running(name: &str) -> bool {
    Command::new("pgrep")
        .arg("-x")
        .arg(name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn format_duration(duration: Duration) -> String {
    format!("{}s", duration.as_secs().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_failures() {
        let checks = vec![CheckResult::ok("a", "ok"), CheckResult::fail("b", "bad")];
        assert!(has_failures(&checks));
    }

    #[test]
    fn parses_aconnect_client() {
        let output =
            "client 0: 'System' [type=kernel]\nclient 32: 'U2MIDI Pro' [type=kernel,card=4]\n";
        assert_eq!(parse_aconnect_client(output, "U2MIDI Pro"), Some(32));
    }
}
