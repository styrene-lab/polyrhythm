use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const OVERHEAD_LEFT: &str = "DrumGizmo:5-OHL";
const OVERHEAD_RIGHT: &str = "DrumGizmo:6-OHR";
const PLAYBACK_LEFT: &str = "playback_FL";
const PLAYBACK_RIGHT: &str = "playback_FR";
const TD50_OBS_MIX: &str = "TD50-OBS-Mix";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingMix {
    Balanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorPair {
    Overheads,
    PracticeRecordingBalanced,
    FullKit,
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
    vec![MonitorSetupOp::ClampMonitor {
        sink: sink.to_string(),
        volume: volume.to_string(),
    }]
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
                target: format!("{sink}:{PLAYBACK_LEFT}"),
            },
            MonitorOp {
                source: OVERHEAD_RIGHT.to_string(),
                target: format!("{sink}:{PLAYBACK_RIGHT}"),
            },
        ],
        MonitorPair::PracticeRecordingBalanced => practice_recording_balanced_plan(sink),
        MonitorPair::FullKit => full_kit_plan(sink),
    }
}

fn practice_recording_balanced_plan(sink: &str) -> Vec<MonitorOp> {
    let mut ops = plan(MonitorPair::Overheads, sink);
    ops.extend(stereo("DrumGizmo:3-Kdrum_front", sink));
    ops.extend(stereo("DrumGizmo:9-Snare_top", sink));
    ops
}

fn stereo(source: &str, sink: &str) -> Vec<MonitorOp> {
    vec![
        MonitorOp {
            source: source.to_string(),
            target: format!("{sink}:{PLAYBACK_LEFT}"),
        },
        MonitorOp {
            source: source.to_string(),
            target: format!("{sink}:{PLAYBACK_RIGHT}"),
        },
    ]
}

pub fn recording_plan(mix: RecordingMix, sink: &str) -> Vec<MonitorOp> {
    match mix {
        RecordingMix::Balanced => recording_balanced_plan(sink),
    }
}

fn recording_balanced_plan(sink: &str) -> Vec<MonitorOp> {
    let mut ops = Vec::new();
    ops.push(MonitorOp {
        source: "spotify:output_FL".to_string(),
        target: format!("{sink}:{PLAYBACK_LEFT}"),
    });
    ops.push(MonitorOp {
        source: "spotify:output_FR".to_string(),
        target: format!("{sink}:{PLAYBACK_RIGHT}"),
    });
    ops.push(MonitorOp {
        source: "DrumGizmo:0-AmbL".to_string(),
        target: format!("{sink}:{PLAYBACK_LEFT}"),
    });
    ops.push(MonitorOp {
        source: "DrumGizmo:1-AmbR".to_string(),
        target: format!("{sink}:{PLAYBACK_RIGHT}"),
    });
    for source in [
        "DrumGizmo:5-OHL",
        "DrumGizmo:3-Kdrum_front",
        "DrumGizmo:9-Snare_top",
    ] {
        ops.push(MonitorOp {
            source: source.to_string(),
            target: format!("{sink}:{PLAYBACK_LEFT}"),
        });
    }
    for source in [
        "DrumGizmo:6-OHR",
        "DrumGizmo:3-Kdrum_front",
        "DrumGizmo:9-Snare_top",
    ] {
        ops.push(MonitorOp {
            source: source.to_string(),
            target: format!("{sink}:{PLAYBACK_RIGHT}"),
        });
    }
    ops
}

pub fn default_recording_sink() -> &'static str {
    TD50_OBS_MIX
}

fn full_kit_plan(sink: &str) -> Vec<MonitorOp> {
    let mut ops = Vec::new();
    ops.push(MonitorOp {
        source: "DrumGizmo:5-OHL".to_string(),
        target: format!("{sink}:{PLAYBACK_LEFT}"),
    });
    ops.push(MonitorOp {
        source: "DrumGizmo:6-OHR".to_string(),
        target: format!("{sink}:{PLAYBACK_RIGHT}"),
    });
    for source in [
        "DrumGizmo:2-Kdrum_back",
        "DrumGizmo:3-Kdrum_front",
        "DrumGizmo:8-Snare_bottom",
        "DrumGizmo:9-Snare_top",
    ] {
        ops.extend(stereo(source, sink));
    }
    ops.push(MonitorOp {
        source: "DrumGizmo:4-Hihat".to_string(),
        target: format!("{sink}:{PLAYBACK_LEFT}"),
    });
    ops.push(MonitorOp {
        source: "DrumGizmo:7-Ride".to_string(),
        target: format!("{sink}:{PLAYBACK_RIGHT}"),
    });
    ops.push(MonitorOp {
        source: "DrumGizmo:10-Tom1".to_string(),
        target: format!("{sink}:{PLAYBACK_LEFT}"),
    });
    ops.extend(stereo("DrumGizmo:11-Tom2", sink));
    ops.push(MonitorOp {
        source: "DrumGizmo:12-Tom3".to_string(),
        target: format!("{sink}:{PLAYBACK_RIGHT}"),
    });
    ops
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
    fn setup_plan_clamps_sink_for_direct_monitor_probe() {
        let ops = setup_plan(DEFAULT_SINK, "50%");
        assert_eq!(ops.len(), 1);
        assert_eq!(
            describe_setup_op(&ops[0]),
            format!("clamp monitor {DEFAULT_SINK} to 50%")
        );
    }

    #[test]
    fn overhead_plan_routes_direct_to_sink() {
        let ops = plan(MonitorPair::Overheads, DEFAULT_SINK);
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].source, "DrumGizmo:5-OHL");
        assert_eq!(ops[0].target, format!("{DEFAULT_SINK}:playback_FL"));
        assert_eq!(ops[1].source, "DrumGizmo:6-OHR");
        assert_eq!(ops[1].target, format!("{DEFAULT_SINK}:playback_FR"));
    }

    #[test]
    fn practice_recording_balanced_plan_routes_overheads_with_kick_and_snare_support() {
        let ops = plan(MonitorPair::PracticeRecordingBalanced, DEFAULT_SINK);
        assert_eq!(ops.len(), 6);
        assert_eq!(ops[0].source, "DrumGizmo:5-OHL");
        assert_eq!(ops[1].source, "DrumGizmo:6-OHR");
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:3-Kdrum_front"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:9-Snare_top"));
        assert!(ops.iter().all(|op| op.target.starts_with(DEFAULT_SINK)));
    }

    #[test]
    fn recording_balanced_plan_routes_spotify_and_drum_support_to_obs_mix() {
        let ops = recording_plan(RecordingMix::Balanced, TD50_OBS_MIX);
        assert_eq!(ops.len(), 10);
        assert!(ops.iter().any(|op| op.source == "spotify:output_FL"));
        assert!(ops.iter().any(|op| op.source == "spotify:output_FR"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:0-AmbL"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:1-AmbR"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:3-Kdrum_front"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:9-Snare_top"));
        assert!(ops.iter().all(|op| op.target.starts_with(TD50_OBS_MIX)));
    }

    #[test]
    fn full_kit_plan_routes_close_mics_at_low_volume() {
        let ops = plan(MonitorPair::FullKit, DEFAULT_SINK);
        assert!(ops.len() > 2);
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:2-Kdrum_back"));
        assert!(ops.iter().any(|op| op.source == "DrumGizmo:9-Snare_top"));
        assert!(ops.iter().all(|op| op.target.starts_with(DEFAULT_SINK)));
    }
}
