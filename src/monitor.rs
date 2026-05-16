use std::process::{Command, Stdio};

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

pub fn default_sink() -> &'static str {
    DEFAULT_SINK
}

#[cfg(test)]
mod tests {
    use super::*;

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
