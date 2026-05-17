use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
pub const DEFAULT_SOURCE: &str = "alsa_input.pci-0000_0e_00.4.analog-stereo";
pub const DEFAULT_CARD: &str = "alsa_card.pci-0000_0e_00.4";
pub const DEFAULT_PORT: &str = "analog-output-lineout";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDoctorReport {
    pub checks: Vec<AudioCheck>,
}

impl AudioDoctorReport {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|check| check.ok)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverAudioOptions {
    pub execute: bool,
    pub sink: String,
    pub source: String,
    pub card: String,
    pub port: String,
    pub volume: String,
    pub restart_spotify: bool,
    pub restart_cosmic_panel: bool,
}

pub fn doctor(sink: &str, source: &str) -> AudioDoctorReport {
    let pactl_info = output("pactl", &["info"]);
    let sinks = output("pactl", &["list", "short", "sinks"]);
    let wpctl = output("wpctl", &["status"]);
    let sink_inputs_short = output("pactl", &["list", "short", "sink-inputs"]);
    let sink_inputs = output("pactl", &["list", "sink-inputs"]);

    let mut checks = vec![
        service_check("pipewire"),
        service_check("pipewire-pulse"),
        service_check("wireplumber"),
        AudioCheck {
            name: "pulse-default-sink".to_string(),
            ok: pactl_info.contains(&format!("Default Sink: {sink}")),
            detail: line_containing(&pactl_info, "Default Sink:")
                .unwrap_or_else(|| "missing Default Sink".to_string()),
        },
        AudioCheck {
            name: "pulse-default-source".to_string(),
            ok: pactl_info.contains(&format!("Default Source: {source}")),
            detail: line_containing(&pactl_info, "Default Source:")
                .unwrap_or_else(|| "missing Default Source".to_string()),
        },
        AudioCheck {
            name: "sink-present".to_string(),
            ok: sinks.contains(sink),
            detail: if sinks.contains(sink) {
                sink.to_string()
            } else {
                nonempty_or(&sinks, "no sinks reported")
            },
        },
        AudioCheck {
            name: "wpctl-onboard-visible".to_string(),
            ok: wpctl.contains("Starship/Matisse HD Audio Controller Analog Stereo")
                || wpctl.contains(sink),
            detail: if wpctl.contains("Starship/Matisse HD Audio Controller Analog Stereo")
                || wpctl.contains(sink)
            {
                "onboard analog visible".to_string()
            } else {
                "onboard analog not visible".to_string()
            },
        },
        AudioCheck {
            name: "cosmic-audio-applet".to_string(),
            ok: shell_success("pgrep -u \"$USER\" -f cosmic-applet-audio >/dev/null 2>&1"),
            detail: if shell_success("pgrep -u \"$USER\" -f cosmic-applet-audio >/dev/null 2>&1") {
                "running".to_string()
            } else {
                "not running".to_string()
            },
        },
    ];

    let spotify_running =
        shell_success("pgrep -f '/share/spotify/.spotify-wrapped|spotify' >/dev/null 2>&1");
    let spotify_stream = sink_inputs.contains("application.name = \"spotify\"")
        || sink_inputs.to_ascii_lowercase().contains("spotify");
    checks.push(AudioCheck {
        name: "spotify-stream".to_string(),
        ok: true,
        detail: if !spotify_running {
            "spotify not running".to_string()
        } else if spotify_stream {
            "spotify stream present".to_string()
        } else {
            format!(
                "warn: spotify running but no Pulse stream; this is normal when paused/stopped; sink-inputs={}",
                nonempty_or(&sink_inputs_short, "none")
            )
        },
    });

    AudioDoctorReport { checks }
}

pub fn recover(options: &RecoverAudioOptions) -> Vec<String> {
    if !options.execute {
        return vec![
            format!("would set {} volume to 1% and mute", options.sink),
            "would clear overhead monitor route and stop TD-50 clients".to_string(),
            "would restart user services: wireplumber pipewire pipewire-pulse".to_string(),
            format!(
                "would set card {} profile to output:analog-stereo+input:analog-stereo",
                options.card
            ),
            format!(
                "would set default sink={} source={}",
                options.sink, options.source
            ),
            format!(
                "would set sink port={} volume={} unmuted",
                options.port, options.volume
            ),
            format!("would restart Spotify: {}", options.restart_spotify),
            format!(
                "would restart COSMIC panel/audio applet: {}",
                options.restart_cosmic_panel
            ),
        ];
    }

    let mut actions = Vec::new();
    actions.push(run("pactl", &["set-sink-volume", &options.sink, "1%"]));
    actions.push(run("pactl", &["set-sink-mute", &options.sink, "1"]));
    actions.extend(stop_td50_clients());
    actions.push(run(
        "systemctl",
        &[
            "--user",
            "restart",
            "wireplumber",
            "pipewire",
            "pipewire-pulse",
        ],
    ));
    actions.push(wait_for_sink(&options.sink, Duration::from_secs(8)));
    actions.push(run(
        "pactl",
        &[
            "set-card-profile",
            &options.card,
            "output:analog-stereo+input:analog-stereo",
        ],
    ));
    actions.push(run("pactl", &["set-default-sink", &options.sink]));
    actions.push(run("pactl", &["set-default-source", &options.source]));
    actions.push(run(
        "pactl",
        &["set-sink-port", &options.sink, &options.port],
    ));
    actions.push(run("pactl", &["set-sink-mute", &options.sink, "0"]));
    actions.push(run(
        "pactl",
        &["set-sink-volume", &options.sink, &options.volume],
    ));

    if options.restart_spotify {
        actions.extend(restart_spotify());
    }
    if options.restart_cosmic_panel {
        actions.extend(restart_cosmic_panel());
    }
    actions
}

fn service_check(name: &str) -> AudioCheck {
    let ok = run_status("systemctl", &["--user", "is-active", "--quiet", name]);
    AudioCheck {
        name: format!("service:{name}"),
        ok,
        detail: if ok { "active" } else { "not active" }.to_string(),
    }
}

fn stop_td50_clients() -> Vec<String> {
    let polyrhythm = if run_status("sh", &["-c", "command -v polyrhythm >/dev/null 2>&1"]) {
        Some("polyrhythm")
    } else if std::path::Path::new("target/debug/polyrhythm").exists() {
        Some("target/debug/polyrhythm")
    } else {
        None
    };

    if let Some(polyrhythm) = polyrhythm {
        vec![
            run(
                polyrhythm,
                &["monitor-clear", "--pair", "overheads", "--execute"],
            ),
            run(polyrhythm, &["stop", "--force"]),
        ]
    } else {
        vec![
            run("pkill", &["-x", "drumgizmo"]),
            run("pkill", &["-x", "td50-drumgizmo-hihat-mapper"]),
            run("pkill", &["-x", "td50-drumgizmo-hihat-mapper-rs"]),
        ]
    }
}

fn restart_spotify() -> Vec<String> {
    let mut actions = vec![run(
        "pkill",
        &["-f", "/share/spotify/.spotify-wrapped|spotify"],
    )];
    thread::sleep(Duration::from_secs(2));
    match Command::new("spotify")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => actions.push("ok: launched spotify".to_string()),
        Err(err) => actions.push(format!("failed: launch spotify: {err}")),
    }
    actions
}

fn restart_cosmic_panel() -> Vec<String> {
    let panel_pids = output("sh", &["-c", "pgrep -u \"$USER\" -x cosmic-panel"]);
    let mut actions = Vec::new();
    for pid in panel_pids
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        actions.push(run("kill", &["-TERM", pid]));
    }
    if actions.is_empty() {
        actions.push("warn: cosmic-panel pid not found".to_string());
    }
    thread::sleep(Duration::from_secs(3));
    if shell_success("pgrep -u \"$USER\" -f cosmic-applet-audio >/dev/null 2>&1") {
        actions.push("ok: cosmic-applet-audio running after panel restart".to_string());
    } else {
        actions.push("failed: cosmic-applet-audio did not respawn".to_string());
    }
    actions
}

fn wait_for_sink(sink: &str, timeout: Duration) -> String {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if output("pactl", &["list", "short", "sinks"]).contains(sink) {
            return format!("ok: sink {sink} reappeared");
        }
        thread::sleep(Duration::from_millis(500));
    }
    format!(
        "failed: sink {sink} did not reappear within {}s",
        timeout.as_secs()
    )
}

fn run(command: &str, args: &[&str]) -> String {
    match Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => format!("ok: {command} {}", args.join(" ")),
        Ok(status) => format!("failed: {command} {} exited with {status}", args.join(" ")),
        Err(err) => format!("failed: {command} {}: {err}", args.join(" ")),
    }
}

fn run_status(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn shell_success(script: &str) -> bool {
    run_status("sh", &["-c", script])
}

fn output(command: &str, args: &[&str]) -> String {
    Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

fn line_containing(text: &str, needle: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains(needle))
        .map(|line| line.trim().to_string())
}

fn nonempty_or(text: &str, fallback: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_recovery_describes_full_dependency_chain() {
        let actions = recover(&RecoverAudioOptions {
            execute: false,
            sink: DEFAULT_SINK.to_string(),
            source: DEFAULT_SOURCE.to_string(),
            card: DEFAULT_CARD.to_string(),
            port: DEFAULT_PORT.to_string(),
            volume: "15%".to_string(),
            restart_spotify: true,
            restart_cosmic_panel: true,
        });
        assert!(actions
            .iter()
            .any(|action| action.contains("wireplumber pipewire pipewire-pulse")));
        assert!(actions
            .iter()
            .any(|action| action.contains("Spotify: true")));
        assert!(actions
            .iter()
            .any(|action| action.contains("COSMIC panel/audio applet: true")));
    }
}
