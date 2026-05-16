use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::process::{stop, StopOptions};

const DEFAULT_MONITOR_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const DEFAULT_SAMPLE_PARAMS: &str = "close=1.0,diverse=0.12,random=0.02";
const DEFAULT_STREAM_LIMIT: u64 = 67_108_864;
const DEFAULT_LOAD_TIMEOUT_SECS: u64 = 90;
const DEFAULT_SAFETY_BUS: &str = "TD50-Safety-Bus";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartConfig {
    pub run_id: String,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    pub midi_device_name: String,
    pub midi_client: Option<u32>,
    pub midi_port: u8,
    pub mapper_bin: PathBuf,
    pub mapper_client_name: String,
    pub mapper_port_name: String,
    pub drumgizmo_bin: String,
    pub kit: PathBuf,
    pub midimap: PathBuf,
    pub sample_params: String,
    pub streaming: bool,
    pub stream_limit: u64,
    pub load_timeout_secs: u64,
    pub safety_bus: String,
    pub route_monitor: bool,
    pub route_obs: bool,
    pub monitor_sinks: Vec<String>,
    pub monitor_volume: String,
}

impl StartConfig {
    pub fn drs_default(home: PathBuf, repo: PathBuf) -> Self {
        Self {
            run_id: run_id(),
            cache_dir: env::var_os("TD50_CACHE")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache/td50")),
            state_dir: env::var_os("POLYRHYTHM_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache/polyrhythm")),
            midi_device_name: "U2MIDI Pro".to_string(),
            midi_client: None,
            midi_port: 0,
            mapper_bin: env::var_os("TD50_HIHAT_MAPPER")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/bin/td50-drumgizmo-hihat-mapper")),
            mapper_client_name: "TD50-DrumGizmo-Hihat-Mapper".to_string(),
            mapper_port_name: "out".to_string(),
            drumgizmo_bin: "drumgizmo".to_string(),
            kit: env::var_os("TD50_DRUMGIZMO_KIT")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share/drumgizmo/kits/DRSKit/DRSKit_full.xml")),
            midimap: env::var_os("TD50_DRUMGIZMO_MIDIMAP")
                .map(PathBuf::from)
                .unwrap_or_else(|| repo.join("assets/drumgizmo/DRSKit/Midimap_td50.xml")),
            sample_params: env::var("TD50_DRUMGIZMO_SAMPLE_PARAMS")
                .unwrap_or_else(|_| DEFAULT_SAMPLE_PARAMS.to_string()),
            streaming: env_flag("TD50_DRUMGIZMO_STREAMING"),
            stream_limit: env::var("TD50_DRUMGIZMO_STREAM_LIMIT")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_STREAM_LIMIT),
            load_timeout_secs: env::var("TD50_DRUMGIZMO_LOAD_TIMEOUT")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_LOAD_TIMEOUT_SECS),
            safety_bus: env::var("TD50_SAFETY_BUS_NAME")
                .unwrap_or_else(|_| DEFAULT_SAFETY_BUS.to_string()),
            route_monitor: false,
            route_obs: false,
            monitor_sinks: vec![
                env::var("TD50_MONITOR_SINKS").unwrap_or_else(|_| DEFAULT_MONITOR_SINK.to_string())
            ],
            monitor_volume: env::var("TD50_MONITOR_VOLUME").unwrap_or_else(|_| "75%".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedOp {
    StopExisting,
    ClampMonitorVolume {
        sink: String,
        volume: String,
    },
    StartMapper {
        command: Vec<String>,
        pidfile: PathBuf,
        log: PathBuf,
    },
    StartDrumGizmo {
        command: Vec<String>,
        pidfile: PathBuf,
        log: PathBuf,
    },
    WaitDrumGizmoLoaded {
        log: PathBuf,
        timeout_secs: u64,
    },
    Link {
        source: String,
        target: String,
        timeout_secs: u64,
    },
    WriteManifest {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub description: String,
    pub required: bool,
    pub status: ExecutionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    Ok,
    Failed(String),
    Skipped(String),
}

pub fn plan_drs(config: &StartConfig) -> Vec<PlannedOp> {
    let mut ops = Vec::new();
    ops.push(PlannedOp::StopExisting);
    if config.route_monitor {
        for sink in &config.monitor_sinks {
            ops.push(PlannedOp::ClampMonitorVolume {
                sink: sink.clone(),
                volume: config.monitor_volume.clone(),
            });
        }
    }
    ops.push(PlannedOp::StartMapper {
        command: vec![
            config.mapper_bin.display().to_string(),
            config
                .midi_client
                .map(|client| client.to_string())
                .unwrap_or_else(|| "<detected-midi-client>".to_string()),
            config.midi_port.to_string(),
        ],
        pidfile: config.cache_dir.join("hihat-mapper.pid"),
        log: config.cache_dir.join("hihat-mapper.log"),
    });
    let drumgizmo_log = config.cache_dir.join("drumgizmo.log");
    ops.push(PlannedOp::StartDrumGizmo {
        command: drumgizmo_command(config),
        pidfile: config.cache_dir.join("drumgizmo.pid"),
        log: drumgizmo_log.clone(),
    });
    if !config.streaming {
        ops.push(PlannedOp::WaitDrumGizmoLoaded {
            log: drumgizmo_log,
            timeout_secs: config.load_timeout_secs,
        });
    }
    ops.push(PlannedOp::Link {
        source: "Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture)".to_string(),
        target: "DrumGizmo:drumgizmo_midiin".to_string(),
        timeout_secs: 1,
    });
    ops.push(PlannedOp::Link {
        source: "Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture)".to_string(),
        target: "DrumGizmo:midi_in".to_string(),
        timeout_secs: 1,
    });
    if config.route_monitor {
        for (source, target) in monitor_links(&config.safety_bus, &config.monitor_sinks) {
            ops.push(PlannedOp::Link {
                source,
                target,
                timeout_secs: 1,
            });
        }
    }
    if config.route_obs {
        ops.push(PlannedOp::Link {
            source: "DrumGizmo:5-OHL".to_string(),
            target: "OBS:input_FL".to_string(),
            timeout_secs: 1,
        });
        ops.push(PlannedOp::Link {
            source: "DrumGizmo:6-OHR".to_string(),
            target: "OBS:input_FR".to_string(),
            timeout_secs: 1,
        });
    }
    ops.push(PlannedOp::WriteManifest {
        path: manifest_path(config),
    });
    ops
}

fn drumgizmo_command(config: &StartConfig) -> Vec<String> {
    let mut command = vec![
        config.drumgizmo_bin.clone(),
        "-i".to_string(),
        "jackmidi".to_string(),
        "-I".to_string(),
        format!("midimap={}", config.midimap.display()),
        "-o".to_string(),
        "jackaudio".to_string(),
        "-p".to_string(),
        config.sample_params.clone(),
    ];
    if config.streaming {
        command.extend([
            "-a".to_string(),
            "-s".to_string(),
            "-S".to_string(),
            format!("limit={}", config.stream_limit),
        ]);
    }
    command.push(config.kit.display().to_string());
    command
}

fn monitor_links(safety_bus: &str, sinks: &[String]) -> Vec<(String, String)> {
    let mut links = vec![
        (
            "DrumGizmo:5-OHL".to_string(),
            format!("{safety_bus}:playback_FL"),
        ),
        (
            "DrumGizmo:6-OHR".to_string(),
            format!("{safety_bus}:playback_FR"),
        ),
    ];
    for sink in sinks {
        links.push((
            format!("{safety_bus}.monitor:capture_FL"),
            format!("{sink}:playback_FL"),
        ));
        links.push((
            format!("{safety_bus}.monitor:capture_FR"),
            format!("{sink}:playback_FR"),
        ));
    }
    links
}

pub fn execute_drs(config: &StartConfig, ops: &[PlannedOp]) -> Vec<ExecutionResult> {
    let mut results = Vec::new();
    let _ = fs::create_dir_all(&config.cache_dir);
    let mut blocked_by_required_failure: Option<String> = None;
    for op in ops {
        let description = describe_op(op);
        let status = if let Some(reason) = &blocked_by_required_failure {
            ExecutionStatus::Skipped(format!("blocked by earlier required failure: {reason}"))
        } else {
            match op {
                PlannedOp::StopExisting => stop(
                    &config.cache_dir,
                    StopOptions {
                        dry_run: false,
                        force: true,
                    },
                )
                .map(|_| ExecutionStatus::Ok)
                .unwrap_or_else(|err| ExecutionStatus::Failed(err.to_string())),
                PlannedOp::ClampMonitorVolume { sink, volume } => clamp_monitor(sink, volume),
                PlannedOp::StartMapper {
                    command,
                    pidfile,
                    log,
                } => start_process(command, pidfile, log, Duration::from_secs(1)),
                PlannedOp::StartDrumGizmo {
                    command,
                    pidfile,
                    log,
                } => start_process(command, pidfile, log, Duration::from_secs(3)),
                PlannedOp::WaitDrumGizmoLoaded { log, timeout_secs } => {
                    wait_for_drumgizmo_loaded(log, Duration::from_secs(*timeout_secs))
                }
                PlannedOp::Link {
                    source,
                    target,
                    timeout_secs,
                } => link(source, target, *timeout_secs),
                PlannedOp::WriteManifest { .. } => {
                    ExecutionStatus::Skipped("written after execution results".to_string())
                }
            }
        };
        let required = is_required(op);
        if required {
            if let ExecutionStatus::Failed(reason) = &status {
                blocked_by_required_failure = Some(reason.clone());
            }
        }
        results.push(ExecutionResult {
            description,
            required,
            status,
        });
    }
    if let Some(index) = results
        .iter()
        .position(|result| result.description.starts_with("write manifest:"))
    {
        results[index].status = write_manifest_with_results(config, ops, &results)
            .map(|_| ExecutionStatus::Ok)
            .unwrap_or_else(|err| ExecutionStatus::Failed(err.to_string()));
    }
    results
}

pub fn is_required(op: &PlannedOp) -> bool {
    match op {
        PlannedOp::StopExisting
        | PlannedOp::ClampMonitorVolume { .. }
        | PlannedOp::StartMapper { .. }
        | PlannedOp::StartDrumGizmo { .. }
        | PlannedOp::WaitDrumGizmoLoaded { .. }
        | PlannedOp::WriteManifest { .. } => true,
        PlannedOp::Link { source, target, .. } => {
            target == "DrumGizmo:drumgizmo_midiin"
                || source == "DrumGizmo:5-OHL"
                || source == "DrumGizmo:6-OHR"
                || source == "DrumGizmo:2-Kdrum_back"
                || source == "DrumGizmo:3-Kdrum_front"
                || source == "DrumGizmo:4-Hihat"
                || source == "DrumGizmo:8-Snare_bottom"
                || source == "DrumGizmo:9-Snare_top"
        }
    }
}

pub fn write_manifest(config: &StartConfig, ops: &[PlannedOp]) -> io::Result<PathBuf> {
    write_manifest_inner(config, ops, true, None)
}

pub fn write_manifest_with_results(
    config: &StartConfig,
    ops: &[PlannedOp],
    results: &[ExecutionResult],
) -> io::Result<PathBuf> {
    write_manifest_inner(config, ops, false, Some(results))
}

fn write_manifest_inner(
    config: &StartConfig,
    ops: &[PlannedOp],
    dry_run: bool,
    results: Option<&[ExecutionResult]>,
) -> io::Result<PathBuf> {
    let path = manifest_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = manifest_json(config, ops, dry_run, results);
    fs::write(&path, &json)?;
    fs::write(config.state_dir.join("current-run.json"), json)?;
    Ok(path)
}

pub fn manifest_path(config: &StartConfig) -> PathBuf {
    config
        .state_dir
        .join("runs")
        .join(format!("{}.json", config.run_id))
}

pub fn describe_op(op: &PlannedOp) -> String {
    match op {
        PlannedOp::StopExisting => "stop existing TD-50 clients".to_string(),
        PlannedOp::ClampMonitorVolume { sink, volume } => {
            format!("clamp monitor {sink} to {volume}")
        }
        PlannedOp::StartMapper {
            command,
            pidfile,
            log,
        } => format!(
            "start mapper: {} >{} pidfile={}",
            command.join(" "),
            log.display(),
            pidfile.display()
        ),
        PlannedOp::StartDrumGizmo {
            command,
            pidfile,
            log,
        } => format!(
            "start DrumGizmo: {} >{} pidfile={}",
            command.join(" "),
            log.display(),
            pidfile.display()
        ),
        PlannedOp::WaitDrumGizmoLoaded { log, timeout_secs } => {
            format!(
                "wait up to {timeout_secs}s for DrumGizmo load marker in {}",
                log.display()
            )
        }
        PlannedOp::Link {
            source,
            target,
            timeout_secs,
        } => format!("link timeout={}s: {source} -> {target}", timeout_secs),
        PlannedOp::WriteManifest { path } => format!("write manifest: {}", path.display()),
    }
}

fn clamp_monitor(sink: &str, volume: &str) -> ExecutionStatus {
    let vol = Command::new("pactl")
        .arg("set-sink-volume")
        .arg(sink)
        .arg(volume)
        .status();
    let mute = Command::new("pactl")
        .arg("set-sink-mute")
        .arg(sink)
        .arg("0")
        .status();
    if vol.map(|s| s.success()).unwrap_or(false) && mute.map(|s| s.success()).unwrap_or(false) {
        ExecutionStatus::Ok
    } else {
        ExecutionStatus::Failed("pactl monitor clamp failed".to_string())
    }
}

fn start_process(
    command: &[String],
    pidfile: &PathBuf,
    log: &PathBuf,
    wait: Duration,
) -> ExecutionStatus {
    if command.is_empty() {
        return ExecutionStatus::Failed("empty command".to_string());
    }
    let Some(parent) = log.parent() else {
        return ExecutionStatus::Failed("log has no parent".to_string());
    };
    if let Err(err) = fs::create_dir_all(parent) {
        return ExecutionStatus::Failed(err.to_string());
    }
    let log_file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log)
    {
        Ok(file) => file,
        Err(err) => return ExecutionStatus::Failed(err.to_string()),
    };
    let stderr = match log_file.try_clone() {
        Ok(file) => file,
        Err(err) => return ExecutionStatus::Failed(err.to_string()),
    };
    let mut child = match Command::new(&command[0])
        .args(&command[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr))
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return ExecutionStatus::Failed(err.to_string()),
    };
    if let Some(parent) = pidfile.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(err) = fs::write(pidfile, format!("{}\n", child.id())) {
        return ExecutionStatus::Failed(err.to_string());
    }
    thread::sleep(wait);
    match child.try_wait() {
        Ok(None) => ExecutionStatus::Ok,
        Ok(Some(status)) => ExecutionStatus::Failed(format!("process exited early: {status}")),
        Err(err) => ExecutionStatus::Failed(err.to_string()),
    }
}

fn wait_for_drumgizmo_loaded(log: &PathBuf, timeout: Duration) -> ExecutionStatus {
    let started = Instant::now();
    loop {
        if fs::read_to_string(log)
            .map(|text| text.lines().any(|line| line.trim() == "done"))
            .unwrap_or(false)
        {
            return ExecutionStatus::Ok;
        }
        if started.elapsed() >= timeout {
            return ExecutionStatus::Failed(format!(
                "DrumGizmo did not log 'done' within {}s",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn link(source: &str, target: &str, timeout_secs: u64) -> ExecutionStatus {
    let status = Command::new("timeout")
        .arg(format!("{}s", timeout_secs.max(1)))
        .arg("pw-link")
        .arg(source)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => ExecutionStatus::Ok,
        Ok(status) if status.code() == Some(124) => {
            ExecutionStatus::Failed("pw-link timed out".to_string())
        }
        Ok(status) => ExecutionStatus::Skipped(format!("pw-link exited with {status}")),
        Err(err) => ExecutionStatus::Failed(err.to_string()),
    }
}

fn manifest_json(
    config: &StartConfig,
    ops: &[PlannedOp],
    dry_run: bool,
    results: Option<&[ExecutionResult]>,
) -> String {
    let ops_json = ops
        .iter()
        .map(|op| format!("\"{}\"", escape_json(&describe_op(op))))
        .collect::<Vec<_>>()
        .join(",");
    let results_json = results
        .map(|results| {
            results
                .iter()
                .map(|result| {
                    format!(
                        "{{\"op\":\"{}\",\"required\":{},\"status\":\"{}\"}}",
                        escape_json(&result.description),
                        result.required,
                        escape_json(&status_text(&result.status))
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!(
        concat!(
            "{{\n",
            "  \"run_id\": \"{}\",\n",
            "  \"kit\": \"drs\",\n",
            "  \"engine\": \"drumgizmo\",\n",
            "  \"dry_run\": {},\n",
            "  \"midi_device\": \"{}\",\n",
            "  \"midi_client\": {},\n",
            "  \"mapper\": \"{}\",\n",
            "  \"drumkit\": \"{}\",\n",
            "  \"midimap\": \"{}\",\n",
            "  \"streaming\": {},\n",
            "  \"load_timeout_secs\": {},\n",
            "  \"safety_bus\": \"{}\",\n",
            "  \"route_monitor\": {},\n",
            "  \"route_obs\": {},\n",
            "  \"monitor_sinks\": [{}],\n",
            "  \"planned_ops\": [{}],\n",
            "  \"results\": [{}]\n",
            "}}\n"
        ),
        escape_json(&config.run_id),
        dry_run,
        escape_json(&config.midi_device_name),
        config
            .midi_client
            .map(|client| client.to_string())
            .unwrap_or_else(|| "null".to_string()),
        escape_json(&config.mapper_bin.display().to_string()),
        escape_json(&config.kit.display().to_string()),
        escape_json(&config.midimap.display().to_string()),
        config.streaming,
        config.load_timeout_secs,
        escape_json(&config.safety_bus),
        config.route_monitor,
        config.route_obs,
        config
            .monitor_sinks
            .iter()
            .map(|sink| format!("\"{}\"", escape_json(sink)))
            .collect::<Vec<_>>()
            .join(","),
        ops_json,
        results_json
    )
}

fn status_text(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Ok => "ok".to_string(),
        ExecutionStatus::Failed(reason) => format!("failed: {reason}"),
        ExecutionStatus::Skipped(reason) => format!("skipped: {reason}"),
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn run_id() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn escape_json(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drs_plan_contains_bounded_links_and_manifest() {
        let config = StartConfig::drs_default(PathBuf::from("/home/test"), PathBuf::from("/repo"));
        let ops = plan_drs(&config);
        assert!(ops.iter().any(|op| matches!(op, PlannedOp::StopExisting)));
        assert!(ops.iter().any(|op| matches!(
            op,
            PlannedOp::Link {
                timeout_secs: 1,
                ..
            }
        )));
        assert!(ops
            .iter()
            .any(|op| matches!(op, PlannedOp::WriteManifest { .. })));
    }

    #[test]
    fn full_load_is_default_and_waits_before_routing() {
        let config = StartConfig::drs_default(PathBuf::from("/home/test"), PathBuf::from("/repo"));
        let ops = plan_drs(&config);
        let start_index = ops
            .iter()
            .position(|op| matches!(op, PlannedOp::StartDrumGizmo { .. }))
            .unwrap();
        let wait_index = ops
            .iter()
            .position(|op| matches!(op, PlannedOp::WaitDrumGizmoLoaded { .. }))
            .unwrap();
        let first_link_index = ops
            .iter()
            .position(|op| matches!(op, PlannedOp::Link { .. }))
            .unwrap();
        assert!(start_index < wait_index);
        assert!(wait_index < first_link_index);
        let PlannedOp::StartDrumGizmo { command, .. } = &ops[start_index] else {
            unreachable!();
        };
        assert!(!command
            .iter()
            .any(|arg| arg == "-a" || arg == "-s" || arg == "-S"));
    }

    #[test]
    fn streaming_opt_in_keeps_legacy_flags_and_skips_load_wait() {
        let mut config =
            StartConfig::drs_default(PathBuf::from("/home/test"), PathBuf::from("/repo"));
        config.streaming = true;
        let ops = plan_drs(&config);
        assert!(!ops
            .iter()
            .any(|op| matches!(op, PlannedOp::WaitDrumGizmoLoaded { .. })));
        let command = ops
            .iter()
            .find_map(|op| match op {
                PlannedOp::StartDrumGizmo { command, .. } => Some(command),
                _ => None,
            })
            .unwrap();
        assert!(command.iter().any(|arg| arg == "-a"));
        assert!(command.iter().any(|arg| arg == "-s"));
        assert!(command.iter().any(|arg| arg == "-S"));
    }

    #[test]
    fn monitor_routes_use_safety_bus_not_direct_sink() {
        let mut config =
            StartConfig::drs_default(PathBuf::from("/home/test"), PathBuf::from("/repo"));
        config.route_monitor = true;
        let ops = plan_drs(&config);
        let monitor_targets: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                PlannedOp::Link { source, target, .. } if source.starts_with("DrumGizmo:") => {
                    Some(target.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(monitor_targets
            .iter()
            .any(|target| target.starts_with("TD50-Safety-Bus:")));
        assert!(!monitor_targets
            .iter()
            .any(|target| target.starts_with(DEFAULT_MONITOR_SINK)));
    }

    #[test]
    fn load_wait_requires_fresh_done_marker() {
        let log = PathBuf::from("/tmp/polyrhythm-test-missing-drumgizmo-done.log");
        let _ = std::fs::remove_file(&log);
        let status = wait_for_drumgizmo_loaded(&log, Duration::from_millis(0));
        assert!(
            matches!(status, ExecutionStatus::Failed(reason) if reason.contains("did not log 'done'"))
        );
        std::fs::write(&log, "loading\ndone\n").unwrap();
        assert_eq!(
            wait_for_drumgizmo_loaded(&log, Duration::from_millis(0)),
            ExecutionStatus::Ok
        );
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn alternate_midi_in_is_optional() {
        let op = PlannedOp::Link {
            source: "Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture)".to_string(),
            target: "DrumGizmo:midi_in".to_string(),
            timeout_secs: 1,
        };
        assert!(!is_required(&op));
    }
}
