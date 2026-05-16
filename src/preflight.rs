use std::path::{Path, PathBuf};

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
}

impl PreflightConfig {
    pub fn drs_default(home: &Path, repo: &Path) -> Self {
        Self {
            required_commands: vec![
                "drumgizmo".to_string(),
                "pw-link".to_string(),
                "aconnect".to_string(),
                "pactl".to_string(),
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
        }
    }
}

pub fn run(config: &PreflightConfig) -> Vec<CheckResult> {
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

    checks.push(file_check("mapper", &config.mapper));
    checks.push(file_check("kit", &config.kit));
    checks.push(file_check("midimap", &config.midimap));
    checks.push(midimap_coverage_check(&config.midimap));
    checks
}

pub fn has_failures(checks: &[CheckResult]) -> bool {
    checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Fail))
}

fn file_check(name: &str, path: &Path) -> CheckResult {
    if path.exists() {
        CheckResult::ok(name, path.display().to_string())
    } else {
        CheckResult::fail(name, format!("missing: {}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_failures() {
        let checks = vec![CheckResult::ok("a", "ok"), CheckResult::fail("b", "bad")];
        assert!(has_failures(&checks));
    }
}
