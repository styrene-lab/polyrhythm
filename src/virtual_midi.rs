use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::process::{stop, StopOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualMidiConfig {
    pub cache_dir: PathBuf,
    pub midi_client: u32,
    pub midi_port: u8,
    pub mapper_bin: PathBuf,
    pub velocity_curve: String,
    pub client_name: String,
    pub port_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualMidiOp {
    StopExisting,
    StartMapper {
        command: Vec<String>,
        pidfile: PathBuf,
        log: PathBuf,
    },
    AdvertiseOutput {
        client_name: String,
        port_name: String,
    },
    CheckJackMidiBridge {
        client_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualMidiResult {
    pub description: String,
    pub required: bool,
    pub status: VirtualMidiStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualMidiStatus {
    Ok,
    Failed(String),
    Skipped(String),
}

pub fn plan(config: &VirtualMidiConfig) -> Vec<VirtualMidiOp> {
    vec![
        VirtualMidiOp::StopExisting,
        VirtualMidiOp::StartMapper {
            command: mapper_command(config),
            pidfile: config.cache_dir.join("hihat-mapper.pid"),
            log: config.cache_dir.join("hihat-mapper.log"),
        },
        VirtualMidiOp::AdvertiseOutput {
            client_name: config.client_name.clone(),
            port_name: config.port_name.clone(),
        },
        VirtualMidiOp::CheckJackMidiBridge {
            client_name: config.client_name.clone(),
        },
    ]
}

pub fn execute(config: &VirtualMidiConfig, ops: &[VirtualMidiOp]) -> Vec<VirtualMidiResult> {
    let mut results = Vec::new();
    let _ = fs::create_dir_all(&config.cache_dir);
    let mut blocked_by_required_failure: Option<String> = None;
    for op in ops {
        let description = describe(op);
        let status = if let Some(reason) = &blocked_by_required_failure {
            VirtualMidiStatus::Skipped(format!("blocked by earlier required failure: {reason}"))
        } else {
            match op {
                VirtualMidiOp::StopExisting => stop(
                    &config.cache_dir,
                    StopOptions {
                        dry_run: false,
                        force: true,
                    },
                )
                .map(|_| VirtualMidiStatus::Ok)
                .unwrap_or_else(|err| VirtualMidiStatus::Failed(err.to_string())),
                VirtualMidiOp::StartMapper {
                    command,
                    pidfile,
                    log,
                } => start_process(command, pidfile, log, Duration::from_secs(1)),
                VirtualMidiOp::AdvertiseOutput { .. } => VirtualMidiStatus::Ok,
                VirtualMidiOp::CheckJackMidiBridge { client_name } => {
                    check_jack_midi_bridge(client_name, Duration::from_secs(2))
                }
            }
        };
        if is_required(op) {
            if let VirtualMidiStatus::Failed(reason) = &status {
                blocked_by_required_failure = Some(reason.clone());
            }
        }
        results.push(VirtualMidiResult {
            description,
            required: is_required(op),
            status,
        });
    }
    results
}

pub fn describe(op: &VirtualMidiOp) -> String {
    match op {
        VirtualMidiOp::StopExisting => "stop existing TD-50 clients".to_string(),
        VirtualMidiOp::StartMapper {
            command,
            pidfile,
            log,
        } => format!(
            "start virtual MIDI mapper: {} >{} pidfile={}",
            command.join(" "),
            log.display(),
            pidfile.display()
        ),
        VirtualMidiOp::AdvertiseOutput {
            client_name,
            port_name,
        } => format!("Ardour input: connect MIDI track to {client_name}:{port_name}"),
        VirtualMidiOp::CheckJackMidiBridge { client_name } => {
            format!("JACK/PipeWire MIDI bridge: expect a capture port containing {client_name}")
        }
    }
}

pub fn is_required(op: &VirtualMidiOp) -> bool {
    matches!(
        op,
        VirtualMidiOp::StopExisting | VirtualMidiOp::StartMapper { .. }
    )
}

fn mapper_command(config: &VirtualMidiConfig) -> Vec<String> {
    vec![
        format!("TD50_VELOCITY_CURVE={}", config.velocity_curve),
        config.mapper_bin.display().to_string(),
        config.midi_client.to_string(),
        config.midi_port.to_string(),
    ]
}

fn start_process(
    command: &[String],
    pidfile: &PathBuf,
    log: &PathBuf,
    wait: Duration,
) -> VirtualMidiStatus {
    if command.len() < 2 {
        return VirtualMidiStatus::Failed("expected env assignment and mapper command".to_string());
    }
    let Some(parent) = log.parent() else {
        return VirtualMidiStatus::Failed("log has no parent".to_string());
    };
    if let Err(err) = fs::create_dir_all(parent) {
        return VirtualMidiStatus::Failed(err.to_string());
    }
    let log_file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log)
    {
        Ok(file) => file,
        Err(err) => return VirtualMidiStatus::Failed(err.to_string()),
    };
    let stderr = match log_file.try_clone() {
        Ok(file) => file,
        Err(err) => return VirtualMidiStatus::Failed(err.to_string()),
    };
    let Some((env_key, env_value)) = command[0].split_once('=') else {
        return VirtualMidiStatus::Failed(
            "first mapper command item must be env assignment".to_string(),
        );
    };
    let child = Command::new(&command[1])
        .args(&command[2..])
        .env(env_key, env_value)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr))
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => return VirtualMidiStatus::Failed(err.to_string()),
    };
    if let Some(parent) = pidfile.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = fs::write(pidfile, format!("{}\n", child.id())) {
        return VirtualMidiStatus::Failed(err.to_string());
    }
    thread::sleep(wait);
    match child.try_wait() {
        Ok(None) => VirtualMidiStatus::Ok,
        Ok(Some(status)) => VirtualMidiStatus::Failed(format!("process exited early: {status}")),
        Err(err) => VirtualMidiStatus::Failed(err.to_string()),
    }
}

fn check_jack_midi_bridge(client_name: &str, timeout: Duration) -> VirtualMidiStatus {
    let output = Command::new("timeout")
        .arg(format!("{}s", timeout.as_secs().max(1)))
        .arg("pw-link")
        .arg("-io")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => return VirtualMidiStatus::Skipped(format!("pw-link unavailable: {err}")),
    };
    if !output.status.success() {
        return VirtualMidiStatus::Skipped(format!("pw-link exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let candidates = jack_midi_bridge_candidates(&stdout, client_name);
    if candidates.is_empty() {
        VirtualMidiStatus::Skipped(format!(
            "no PipeWire/JACK MIDI bridge port found for {client_name}"
        ))
    } else {
        VirtualMidiStatus::Ok
    }
}

pub fn jack_midi_bridge_candidates(output: &str, client_name: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| {
            line.contains("Midi-Bridge") && line.contains(client_name) && line.contains("(capture)")
        })
        .map(|line| line.trim().to_string())
        .collect()
}

pub fn write_plan(
    path: &PathBuf,
    config: &VirtualMidiConfig,
    ops: &[VirtualMidiOp],
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let ops = ops
        .iter()
        .map(|op| format!("\"{}\"", escape_json(&describe(op))))
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        path,
        format!(
            "{{\"mode\":\"virtual-midi\",\"midi_client\":{},\"midi_port\":{},\"output\":\"{}:{}\",\"ops\":[{}]}}\n",
            config.midi_client,
            config.midi_port,
            escape_json(&config.client_name),
            escape_json(&config.port_name),
            ops
        ),
    )
}

fn escape_json(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_midi_plan_starts_mapper_without_drumgizmo_or_audio_links() {
        let config = VirtualMidiConfig {
            cache_dir: PathBuf::from("/tmp/cache"),
            midi_client: 36,
            midi_port: 0,
            mapper_bin: PathBuf::from("mapper"),
            velocity_curve: "linear".to_string(),
            client_name: "Polyrhythm Canonical Out".to_string(),
            port_name: "out".to_string(),
        };
        let ops = plan(&config);
        assert_eq!(ops.len(), 4);
        let descriptions = ops.iter().map(describe).collect::<Vec<_>>().join("\n");
        assert!(descriptions.contains("start virtual MIDI mapper"));
        assert!(descriptions.contains("Ardour input"));
        assert!(descriptions.contains("JACK/PipeWire MIDI bridge"));
        assert!(!descriptions.contains("DrumGizmo"));
        assert!(!descriptions.contains("pw-link "));
    }

    #[test]
    fn finds_pipewire_midi_bridge_capture_for_mapper() {
        let output = "Midi-Bridge:U2MIDI Pro MIDI 1 (capture)\nMidi-Bridge:Polyrhythm Canonical Outout (capture)\nMidi-Bridge:Polyrhythm Canonical Outout (playback)\n";
        let candidates = jack_midi_bridge_candidates(output, "Polyrhythm Canonical Out");
        assert_eq!(
            candidates,
            vec!["Midi-Bridge:Polyrhythm Canonical Outout (capture)".to_string()]
        );
    }
}
