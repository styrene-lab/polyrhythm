use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorSafety {
    pub sinks: Vec<String>,
    pub restore_volume: String,
    pub safety_volume: String,
    pub mute: bool,
}

impl MonitorSafety {
    pub fn new(sinks: Vec<String>, restore_volume: String) -> Self {
        Self {
            sinks,
            restore_volume,
            safety_volume: "5%".to_string(),
            mute: false,
        }
    }
}

pub fn apply_safety(config: &MonitorSafety) -> Vec<String> {
    let mut actions = Vec::new();
    for sink in &config.sinks {
        let volume = set_sink_volume(sink, &config.safety_volume);
        actions.push(format!(
            "safety volume {sink} -> {}: {volume}",
            config.safety_volume
        ));
        if config.mute {
            let mute = set_sink_mute(sink, true);
            actions.push(format!("safety mute {sink}: {mute}"));
        }
    }
    actions
}

pub fn restore(config: &MonitorSafety) -> Vec<String> {
    let mut actions = Vec::new();
    for sink in &config.sinks {
        if config.mute {
            let mute = set_sink_mute(sink, false);
            actions.push(format!("restore unmute {sink}: {mute}"));
        }
        let volume = set_sink_volume(sink, &config.restore_volume);
        actions.push(format!(
            "restore volume {sink} -> {}: {volume}",
            config.restore_volume
        ));
    }
    actions
}

fn set_sink_volume(sink: &str, volume: &str) -> &'static str {
    command_status(
        Command::new("pactl")
            .arg("set-sink-volume")
            .arg(sink)
            .arg(volume),
    )
}

fn set_sink_mute(sink: &str, mute: bool) -> &'static str {
    command_status(
        Command::new("pactl")
            .arg("set-sink-mute")
            .arg(sink)
            .arg(if mute { "1" } else { "0" }),
    )
}

fn command_status(command: &mut Command) -> &'static str {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| if status.success() { "ok" } else { "failed" })
        .unwrap_or("failed")
}
