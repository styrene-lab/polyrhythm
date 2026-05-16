use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_MONITOR_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const DEFAULT_SAMPLE_PARAMS: &str = "close=1.0,diverse=0.12,random=0.02";
const DEFAULT_STREAM_LIMIT: u64 = 67_108_864;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartConfig {
    pub run_id: String,
    pub cache_dir: PathBuf,
    pub state_dir: PathBuf,
    pub midi_device_name: String,
    pub midi_port: u8,
    pub mapper_bin: PathBuf,
    pub mapper_client_name: String,
    pub mapper_port_name: String,
    pub drumgizmo_bin: String,
    pub kit: PathBuf,
    pub midimap: PathBuf,
    pub sample_params: String,
    pub stream_limit: u64,
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
            stream_limit: env::var("TD50_DRUMGIZMO_STREAM_LIMIT")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_STREAM_LIMIT),
            route_monitor: true,
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
    Link {
        source: String,
        target: String,
        timeout_secs: u64,
    },
    WriteManifest {
        path: PathBuf,
    },
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
            "<detected-midi-client>".to_string(),
            config.midi_port.to_string(),
        ],
        pidfile: config.cache_dir.join("hihat-mapper.pid"),
        log: config.cache_dir.join("hihat-mapper.log"),
    });

    ops.push(PlannedOp::StartDrumGizmo {
        command: vec![
            config.drumgizmo_bin.clone(),
            "-i".to_string(),
            "jackmidi".to_string(),
            "-I".to_string(),
            format!("midimap={}", config.midimap.display()),
            "-o".to_string(),
            "jackaudio".to_string(),
            "-p".to_string(),
            config.sample_params.clone(),
            "-a".to_string(),
            "-s".to_string(),
            "-S".to_string(),
            format!("limit={}", config.stream_limit),
            config.kit.display().to_string(),
        ],
        pidfile: config.cache_dir.join("drumgizmo.pid"),
        log: config.cache_dir.join("drumgizmo.log"),
    });

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
        for sink in &config.monitor_sinks {
            ops.push(PlannedOp::Link {
                source: "DrumGizmo:5-OHL".to_string(),
                target: format!("{sink}:playback_FL"),
                timeout_secs: 1,
            });
            ops.push(PlannedOp::Link {
                source: "DrumGizmo:6-OHR".to_string(),
                target: format!("{sink}:playback_FR"),
                timeout_secs: 1,
            });
            for channel in [
                "DrumGizmo:2-Kdrum_back",
                "DrumGizmo:3-Kdrum_front",
                "DrumGizmo:4-Hihat",
                "DrumGizmo:8-Snare_bottom",
                "DrumGizmo:9-Snare_top",
            ] {
                ops.push(PlannedOp::Link {
                    source: channel.to_string(),
                    target: format!("{sink}:playback_FL"),
                    timeout_secs: 1,
                });
                ops.push(PlannedOp::Link {
                    source: channel.to_string(),
                    target: format!("{sink}:playback_FR"),
                    timeout_secs: 1,
                });
            }
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

pub fn write_manifest(config: &StartConfig, ops: &[PlannedOp]) -> io::Result<PathBuf> {
    let path = manifest_path(config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, manifest_json(config, ops))?;
    let current = config.state_dir.join("current-run.json");
    fs::write(current, manifest_json(config, ops))?;
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
        PlannedOp::Link {
            source,
            target,
            timeout_secs,
        } => format!("link timeout={}s: {source} -> {target}", timeout_secs),
        PlannedOp::WriteManifest { path } => format!("write manifest: {}", path.display()),
    }
}

fn manifest_json(config: &StartConfig, ops: &[PlannedOp]) -> String {
    let ops_json = ops
        .iter()
        .map(|op| format!("\"{}\"", escape_json(&describe_op(op))))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        concat!(
            "{{\n",
            "  \"run_id\": \"{}\",\n",
            "  \"kit\": \"drs\",\n",
            "  \"engine\": \"drumgizmo\",\n",
            "  \"dry_run\": true,\n",
            "  \"midi_device\": \"{}\",\n",
            "  \"mapper\": \"{}\",\n",
            "  \"drumkit\": \"{}\",\n",
            "  \"midimap\": \"{}\",\n",
            "  \"route_monitor\": {},\n",
            "  \"route_obs\": {},\n",
            "  \"monitor_sinks\": [{}],\n",
            "  \"planned_ops\": [{}]\n",
            "}}\n"
        ),
        escape_json(&config.run_id),
        escape_json(&config.midi_device_name),
        escape_json(&config.mapper_bin.display().to_string()),
        escape_json(&config.kit.display().to_string()),
        escape_json(&config.midimap.display().to_string()),
        config.route_monitor,
        config.route_obs,
        config
            .monitor_sinks
            .iter()
            .map(|sink| format!("\"{}\"", escape_json(sink)))
            .collect::<Vec<_>>()
            .join(","),
        ops_json
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
}
