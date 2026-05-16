use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const DEFAULT_SAFETY_BUS: &str = "TD50-Safety-Bus";
const OVERHEAD_LEFT: &str = "DrumGizmo:5-OHL";
const OVERHEAD_RIGHT: &str = "DrumGizmo:6-OHR";
const PLAYBACK_LEFT: &str = "playback_FL";
const PLAYBACK_RIGHT: &str = "playback_FR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorPair {
    Overheads,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorAction {
    Link,
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorOp {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorSetupOp {
    EnsureSafetyBus { name: String, volume: String },
    ClampMonitor { sink: String, volume: String },
}

pub fn setup_plan(sink: &str, volume: &str) -> Vec<MonitorSetupOp> {
    vec![
        MonitorSetupOp::EnsureSafetyBus {
            name: DEFAULT_SAFETY_BUS.to_string(),
            volume: volume.to_string(),
        },
        MonitorSetupOp::ClampMonitor {
            sink: sink.to_string(),
            volume: volume.to_string(),
        },
    ]
}

pub fn describe_setup_op(op: &MonitorSetupOp) -> String {
    match op {
        MonitorSetupOp::EnsureSafetyBus { name, volume } => {
            format!("ensure safety bus {name} exists at {volume}")
        }
        MonitorSetupOp::ClampMonitor { sink, volume } => {
            format!("clamp monitor {sink} to {volume}")
        }
    }
}

pub fn execute_setup(ops: &[MonitorSetupOp]) -> Vec<String> {
    let mut results = Vec::new();
    for op in ops {
        let result = match op {
            MonitorSetupOp::EnsureSafetyBus { name, volume } => ensure_safety_bus(name, volume),
            MonitorSetupOp::ClampMonitor { sink, volume } => clamp_monitor(sink, volume),
        };
        let failed = result.starts_with("failed:");
        results.push(result);
        if failed {
            break;
        }
    }
    results
}

pub fn setup_failed(results: &[String]) -> bool {
    results.iter().any(|result| result.starts_with("failed:"))
}

pub fn plan(pair: MonitorPair, sink: &str) -> Vec<MonitorOp> {
    match pair {
        MonitorPair::Overheads => vec![
            MonitorOp {
                source: OVERHEAD_LEFT.to_string(),
                target: format!("{DEFAULT_SAFETY_BUS}:{PLAYBACK_LEFT}"),
            },
            MonitorOp {
                source: OVERHEAD_RIGHT.to_string(),
                target: format!("{DEFAULT_SAFETY_BUS}:{PLAYBACK_RIGHT}"),
            },
            MonitorOp {
                source: format!("{DEFAULT_SAFETY_BUS}.monitor:capture_FL"),
                target: format!("{sink}:{PLAYBACK_LEFT}"),
            },
            MonitorOp {
                source: format!("{DEFAULT_SAFETY_BUS}.monitor:capture_FR"),
                target: format!("{sink}:{PLAYBACK_RIGHT}"),
            },
        ],
    }
}

pub fn execute(action: MonitorAction, ops: &[MonitorOp]) -> Vec<String> {
    ops.iter()
        .map(|op| {
            let mut command = Command::new("timeout");
            command.arg("1s").arg("pw-link");
            if matches!(action, MonitorAction::Clear) {
                command.arg("-d");
            }
            let status = command
                .arg(&op.source)
                .arg(&op.target)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let verb = match action {
                MonitorAction::Link => "link",
                MonitorAction::Clear => "clear",
            };
            match status {
                Ok(status) if status.success() => {
                    format!("ok: {verb} {} -> {}", op.source, op.target)
                }
                Ok(status) if matches!(action, MonitorAction::Clear) => {
                    format!("skipped: {verb} {} -> {} ({status})", op.source, op.target)
                }
                Ok(status) => format!("failed: {verb} {} -> {} ({status})", op.source, op.target),
                Err(err) => format!("failed: {verb} {} -> {} ({err})", op.source, op.target),
            }
        })
        .collect()
}

fn ensure_safety_bus(name: &str, volume: &str) -> String {
    if !sink_present(name) {
        let status = Command::new("pactl")
            .arg("load-module")
            .arg("module-null-sink")
            .arg(format!("sink_name={name}"))
            .arg("channels=2")
            .arg("channel_map=front-left,front-right")
            .arg(format!("sink_properties=device.description={name}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return format!(
                    "failed: ensure safety bus {name}: pactl load-module exited with {status}"
                );
            }
            Err(err) => return format!("failed: ensure safety bus {name}: {err}"),
        }
        if !wait_for_sink_present(name, Duration::from_secs(2)) {
            return format!("failed: ensure safety bus {name}: sink did not appear");
        }
    }
    match clamp_monitor(name, volume).strip_prefix("ok: clamp monitor ") {
        Some(_) => format!("ok: ensure safety bus {name} exists at {volume}"),
        None => format!("failed: ensure safety bus {name} exists at {volume}"),
    }
}

fn clamp_monitor(sink: &str, volume: &str) -> String {
    let vol = Command::new("pactl")
        .arg("set-sink-volume")
        .arg(sink)
        .arg(volume)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let mute = Command::new("pactl")
        .arg("set-sink-mute")
        .arg(sink)
        .arg("0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if vol.map(|status| status.success()).unwrap_or(false)
        && mute.map(|status| status.success()).unwrap_or(false)
    {
        format!("ok: clamp monitor {sink} to {volume}")
    } else {
        format!("failed: clamp monitor {sink} to {volume}")
    }
}

fn sink_present(name: &str) -> bool {
    Command::new("pactl")
        .args(["list", "short", "sinks"])
        .stdin(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(name))
        .unwrap_or(false)
}

fn wait_for_sink_present(name: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if sink_present(name) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

pub fn default_sink() -> &'static str {
    DEFAULT_SINK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_plan_ensures_bus_before_clamping_sink() {
        let ops = setup_plan(DEFAULT_SINK, "5%");
        assert_eq!(
            describe_setup_op(&ops[0]),
            "ensure safety bus TD50-Safety-Bus exists at 5%"
        );
        assert_eq!(
            describe_setup_op(&ops[1]),
            format!("clamp monitor {DEFAULT_SINK} to 5%")
        );
    }

    #[test]
    fn overhead_plan_routes_through_safety_bus() {
        let ops = plan(MonitorPair::Overheads, DEFAULT_SINK);
        assert_eq!(ops.len(), 4);
        assert_eq!(ops[0].source, "DrumGizmo:5-OHL");
        assert_eq!(ops[0].target, "TD50-Safety-Bus:playback_FL");
        assert_eq!(ops[1].source, "DrumGizmo:6-OHR");
        assert_eq!(ops[1].target, "TD50-Safety-Bus:playback_FR");
        assert_eq!(ops[2].source, "TD50-Safety-Bus.monitor:capture_FL");
        assert_eq!(ops[2].target, format!("{DEFAULT_SINK}:playback_FL"));
        assert_eq!(ops[3].source, "TD50-Safety-Bus.monitor:capture_FR");
        assert_eq!(ops[3].target, format!("{DEFAULT_SINK}:playback_FR"));
    }
}
