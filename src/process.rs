use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Zombie,
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidStatus {
    pub label: &'static str,
    pub pidfile: PathBuf,
    pub pid: Option<u32>,
    pub state: ProcessState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopOptions {
    pub dry_run: bool,
    pub force: bool,
}

pub const PIDFILES: &[(&str, &str)] = &[
    ("filter", "filter.pid"),
    ("sfizz", "sfizz.pid"),
    ("drumgizmo", "drumgizmo.pid"),
    ("hihat mapper", "hihat-mapper.pid"),
];

pub const PROCESS_NAMES: &[&str] = &[
    "sfizz_jack",
    "drumgizmo",
    "td50-midi-filter",
    "td50-drumgizmo-hihat-mapper",
    "td50-drumgizmo-hihat-mapper-rs",
    "pw-link",
];

pub fn pid_statuses(cache: &Path) -> Vec<PidStatus> {
    PIDFILES
        .iter()
        .map(|(label, file)| {
            let pidfile = cache.join(file);
            let pid = read_pid(&pidfile).ok().flatten();
            let state = pid.map(process_state).unwrap_or(ProcessState::Dead);
            PidStatus {
                label,
                pidfile,
                pid,
                state,
            }
        })
        .collect()
}

pub fn stop(cache: &Path, options: StopOptions) -> io::Result<Vec<String>> {
    let mut actions = Vec::new();

    for status in pid_statuses(cache) {
        if let Some(pid) = status.pid {
            actions.push(format!("kill pid {pid} from {}", status.pidfile.display()));
            if !options.dry_run {
                let _ = signal_pid(pid, "TERM");
            }
        }
        if status.pidfile.exists() {
            actions.push(format!("remove pidfile {}", status.pidfile.display()));
            if !options.dry_run {
                let _ = fs::remove_file(&status.pidfile);
            }
        }
    }

    for name in PROCESS_NAMES {
        actions.push(format!("pkill -x {name}"));
        if !options.dry_run {
            let _ = pkill(name, false);
        }
    }

    if options.force {
        for name in PROCESS_NAMES {
            actions.push(format!("pkill -9 -x {name}"));
            if !options.dry_run {
                let _ = pkill(name, true);
            }
        }
    }

    Ok(actions)
}

pub fn read_pid(path: &Path) -> io::Result<Option<u32>> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(raw.trim().parse::<u32>().ok()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn process_state(pid: u32) -> ProcessState {
    let status = PathBuf::from("/proc").join(pid.to_string()).join("status");
    let Ok(raw) = fs::read_to_string(status) else {
        return ProcessState::Dead;
    };

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("State:") {
            return if rest.split_whitespace().next() == Some("Z") {
                ProcessState::Zombie
            } else {
                ProcessState::Running
            };
        }
    }

    ProcessState::Running
}

fn signal_pid(pid: u32, signal: &str) -> io::Result<()> {
    Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
}

fn pkill(name: &str, force: bool) -> io::Result<()> {
    let mut command = Command::new("pkill");
    if force {
        command.arg("-9");
    }
    command
        .arg("-x")
        .arg(name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn read_pid_missing_is_none() {
        let path = PathBuf::from("/definitely/missing/polyrhythm.pid");
        assert_eq!(read_pid(&path).unwrap(), None);
    }

    #[test]
    fn read_pid_parses_numeric_pid() {
        let dir = std::env::temp_dir().join(format!(
            "polyrhythm-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.pid");
        fs::write(&path, "1234\n").unwrap();
        assert_eq!(read_pid(&path).unwrap(), Some(1234));
        fs::remove_dir_all(dir).unwrap();
    }
}
