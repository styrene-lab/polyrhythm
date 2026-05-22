use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::audio::{apply_safety, restore, MonitorSafety};
use crate::graph::{
    check as graph_check, dump as graph_dump, print_summary as print_graph_summary, DesiredState,
};
use crate::monitor::{
    default_recording_sink, default_sink as default_monitor_sink,
    describe_setup_op as describe_monitor_setup_op, execute as execute_monitor,
    execute_setup as execute_monitor_setup, plan as plan_monitor, recording_plan,
    setup_failed as monitor_setup_failed, setup_plan as plan_monitor_setup, MonitorAction,
    MonitorPair, RecordingMix,
};
use crate::preflight::{
    detect_midi_client, has_failures, run as run_preflight, CheckStatus, PreflightConfig,
    PreflightReport,
};
use crate::process::{pid_statuses, stop, ProcessState, StopOptions};
use crate::profiles::{
    builtin_devices, builtin_kits, find_device, find_kit, write_generated_midimap,
};
use crate::recovery::{
    doctor as audio_doctor_report, recover as recover_audio_steps, RecoverAudioOptions,
    DEFAULT_CARD as DEFAULT_AUDIO_CARD, DEFAULT_PORT as DEFAULT_AUDIO_PORT,
    DEFAULT_SINK as DEFAULT_AUDIO_SINK, DEFAULT_SOURCE as DEFAULT_AUDIO_SOURCE,
};
use crate::start::{
    describe_op, execute_drs, plan_drs, write_manifest, ExecutionStatus, StartConfig,
};
use crate::td50_mapper::{mapped_notes_from_midimap, DRS_EMITTED_NOTES};
use crate::trace::{tail as trace_tail, trace_path, write_event, TraceEvent};
use crate::virtual_midi::{self, VirtualMidiConfig, VirtualMidiStatus};
use crate::workbench::coverage as workbench_coverage;
use crate::workbench::replay as workbench_replay;
use crate::workbench::trace as workbench_trace;

const DEFAULT_CACHE: &str = ".cache/td50";
const DEFAULT_MONITOR_SINK: &str = "alsa_output.pci-0000_0e_00.4.analog-stereo";
const DEFAULT_MIDI_DEVICE: &str = "U2MIDI Pro";
const DEFAULT_MAPPER_CLIENT: &str = "TD50-DrumGizmo-Hihat-Mapper";
const DEFAULT_MAPPER_PORT: &str = "out";

#[derive(Debug, Parser)]
#[command(name = "polyrhythm")]
#[command(about = "Safe e-drum rig control and MIDI mapping tooling")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the safe TD-50/DRS launch plan without starting live audio clients.
    Plan {
        #[arg(long, value_enum, default_value_t = Kit::Drs)]
        kit: Kit,
        #[arg(long)]
        allow_experimental: bool,
        #[arg(long, default_value_t = false)]
        route_obs: bool,
        #[arg(long, default_value_t = false)]
        route_monitor: bool,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        monitor_sink: String,
        #[arg(long, default_value = "5%")]
        monitor_volume: String,
        #[arg(long, default_value_t = false)]
        streaming: bool,
    },

    /// Run offline checks that do not touch PipeWire, ALSA, or live audio clients.
    Doctor {
        #[arg(long, default_value = "assets/drumgizmo/DRSKit/Midimap_td50.xml")]
        drs_midimap: PathBuf,
    },

    /// Plan a safe DRS start and write a dry-run manifest.
    Start {
        #[arg(long, value_enum, default_value_t = Kit::Drs)]
        kit: Kit,
        #[arg(long)]
        allow_experimental: bool,
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long, default_value_t = false)]
        route_monitor: bool,
        #[arg(long, default_value_t = false)]
        route_obs: bool,
        #[arg(long, default_value_t = false)]
        streaming: bool,
    },

    /// Stop then start the selected kit. Dry-run by default.
    Switch {
        #[arg(long, value_enum, default_value_t = Kit::Drs)]
        kit: Kit,
        #[arg(long)]
        allow_experimental: bool,
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long, default_value_t = false)]
        route_monitor: bool,
        #[arg(long, default_value_t = false)]
        route_obs: bool,
        #[arg(long, default_value_t = false)]
        streaming: bool,
        #[arg(long, default_value_t = true)]
        safety: bool,
    },

    /// Run preflight checks for the known DRS path without starting live audio clients.
    Preflight {
        #[arg(long, value_enum, default_value_t = Kit::Drs)]
        kit: Kit,
        #[arg(long)]
        allow_experimental: bool,
    },

    /// Show cached TD-50 process state without PipeWire graph probing.
    Status {
        #[arg(long)]
        strict: bool,
    },

    /// Stop known TD-50 clients and stuck routing commands.
    Stop {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, default_value_t = true)]
        safety: bool,
    },

    /// Print environment variables needed to run the legacy DRS shell path safely.
    LegacyEnv {
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        monitor_sink: String,
        #[arg(long, default_value = "5%")]
        monitor_volume: String,
    },

    /// Inspect the JSONL trace log.
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },

    /// Open a Qt PipeWire graph viewer for manual inspection/screenshots.
    Graph {
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },

    /// Capture and summarize a bounded PipeWire graph snapshot.
    GraphDump,

    /// Check current PipeWire graph against an expected drum-rig state.
    GraphCheck {
        #[arg(long, value_enum)]
        state: GraphState,
    },

    /// Lower monitor volume without killing DrumGizmo or the mapper.
    Quiet {
        #[arg(long, default_value = "50%")]
        volume: String,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        sink: String,
    },

    /// Add a proven-good full-kit DrumGizmo monitor route. Dry-run by default.
    MonitorTest {
        #[arg(long, value_enum, default_value_t = MonitorPairArg::FullKit)]
        pair: MonitorPairArg,
        #[arg(long, default_value = "50%")]
        volume: String,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        sink: String,
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
    },

    /// Apply the recording-balanced OBS/player routing preset. Dry-run by default.
    RecordingBalanced {
        #[arg(long, default_value = "50%")]
        volume: String,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        monitor_sink: String,
        #[arg(long)]
        recording_sink: Option<String>,
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
    },

    /// Remove low-volume DrumGizmo monitor test routes without killing the engine.
    MonitorClear {
        #[arg(long, value_enum, default_value_t = MonitorPairArg::FullKit)]
        pair: MonitorPairArg,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        sink: String,
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
    },

    /// Check desktop/COSMIC audio health after PipeWire incidents.
    AudioDoctor {
        #[arg(long, default_value = DEFAULT_AUDIO_SINK)]
        sink: String,
        #[arg(long, default_value = DEFAULT_AUDIO_SOURCE)]
        source: String,
    },

    /// Recover PipeWire, desktop audio UI, and common clients. Dry-run by default.
    RecoverAudio {
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long, default_value = DEFAULT_AUDIO_SINK)]
        sink: String,
        #[arg(long, default_value = DEFAULT_AUDIO_SOURCE)]
        source: String,
        #[arg(long, default_value = DEFAULT_AUDIO_CARD)]
        card: String,
        #[arg(long, default_value = DEFAULT_AUDIO_PORT)]
        port: String,
        #[arg(long, default_value = "15%")]
        volume: String,
        #[arg(long, default_value_t = true)]
        restart_spotify: bool,
        #[arg(long, default_value_t = true)]
        restart_cosmic_panel: bool,
    },

    /// Start only the normalized mapper virtual MIDI output for DAW/Ardour recording.
    VirtualMidi {
        #[arg(long, default_value_t = true, conflicts_with = "execute")]
        dry_run: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long, default_value = DEFAULT_MIDI_DEVICE)]
        midi_device: String,
        #[arg(long, default_value_t = 0)]
        midi_port: u8,
        #[arg(long, default_value = DEFAULT_MAPPER_CLIENT)]
        output_client: String,
        #[arg(long, default_value = DEFAULT_MAPPER_PORT)]
        output_port: String,
    },

    /// Diagnose Ardour-side MIDI graph visibility without mutating routes.
    ArdourMidiDiagnose {
        #[arg(long, default_value = DEFAULT_MAPPER_CLIENT)]
        output_client: String,
    },

    /// List built-in device profiles.
    Devices,

    /// List built-in DrumGizmo kit profiles.
    Kits,

    /// Generate a DrumGizmo midimap from a device profile and kit profile.
    GenerateMidimap {
        #[arg(long, default_value = "td50")]
        device: String,
        #[arg(long, default_value = "crocell")]
        kit: String,
    },

    /// Validate semantic coverage between a device profile and kit profile.
    MapCheck {
        #[arg(long, default_value = "td50")]
        device: String,
        #[arg(long, default_value = "crocell")]
        kit: String,
    },

    /// Print canonical Pikl profile details for a device/kit pair.
    ProfileInspect {
        #[arg(long, default_value = "td50")]
        device: String,
        #[arg(long, default_value = "crocell")]
        kit: String,
    },

    /// Inspect the workbench/profile-builder backend state without touching live audio.
    Workbench {
        #[command(subcommand)]
        command: WorkbenchCommand,
    },

    /// Print the current safety policy encoded by the CLI.
    Policy,
}

#[derive(Debug, Subcommand)]
enum WorkbenchCommand {
    /// Report static coverage between a device profile and kit profile.
    Coverage {
        #[arg(long, default_value = "td50")]
        device: String,
        #[arg(long, default_value = "crocell")]
        kit: String,
        #[arg(long)]
        jsonl: Option<PathBuf>,
    },

    /// Replay a workbench JSONL raw-event trace through the current mapper core.
    Replay {
        trace: PathBuf,
        #[arg(long, default_value = "td50")]
        device: String,
        #[arg(long, default_value = "crocell")]
        kit: String,
    },
}

#[derive(Debug, Subcommand)]
enum TraceCommand {
    /// Print the trace file path.
    Path,
    /// Print the last N trace events.
    Tail {
        #[arg(long, default_value_t = 20)]
        lines: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Kit {
    Drs,
    Crocell,
    Muldjord,
    Aasimonster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum GraphState {
    EngineOnly,
    OverheadMonitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum MonitorPairArg {
    Overheads,
    PracticeRecordingBalanced,
    FullKit,
}

impl From<MonitorPairArg> for MonitorPair {
    fn from(value: MonitorPairArg) -> Self {
        match value {
            MonitorPairArg::Overheads => Self::Overheads,
            MonitorPairArg::PracticeRecordingBalanced => Self::PracticeRecordingBalanced,
            MonitorPairArg::FullKit => Self::FullKit,
        }
    }
}

impl From<GraphState> for DesiredState {
    fn from(value: GraphState) -> Self {
        match value {
            GraphState::EngineOnly => Self::EngineOnly,
            GraphState::OverheadMonitor => Self::OverheadMonitor,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LaunchOptions {
    route_monitor: bool,
    route_obs: bool,
    streaming: bool,
}

impl Kit {
    fn is_experimental(self) -> bool {
        !matches!(self, Self::Drs)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Drs => "drs",
            Self::Crocell => "crocell",
            Self::Muldjord => "muldjord",
            Self::Aasimonster => "aasimonster",
        }
    }
}

pub fn run() -> i32 {
    match run_result(Cli::parse()) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("ERROR: {message}");
            let _ = write_event(TraceEvent::error("command_failed", &message));
            2
        }
    }
}

fn run_result(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Plan {
            kit,
            allow_experimental,
            route_obs,
            route_monitor,
            monitor_sink,
            monitor_volume,
            streaming,
        } => plan(
            kit,
            allow_experimental,
            route_obs,
            route_monitor,
            &monitor_sink,
            &monitor_volume,
            streaming,
        ),
        Command::Doctor { drs_midimap } => doctor(&drs_midimap),
        Command::Start {
            kit,
            allow_experimental,
            dry_run,
            execute,
            route_monitor,
            route_obs,
            streaming,
        } => start_command(
            kit,
            allow_experimental,
            dry_run,
            execute,
            LaunchOptions {
                route_monitor,
                route_obs,
                streaming,
            },
        ),
        Command::Switch {
            kit,
            allow_experimental,
            dry_run,
            execute,
            route_monitor,
            route_obs,
            streaming,
            safety,
        } => switch_command(
            kit,
            allow_experimental,
            dry_run,
            execute,
            LaunchOptions {
                route_monitor,
                route_obs,
                streaming,
            },
            safety,
        ),
        Command::Preflight {
            kit,
            allow_experimental,
        } => preflight(kit, allow_experimental),
        Command::Status { strict } => status(strict),
        Command::Stop {
            dry_run,
            force,
            safety,
        } => stop_command(dry_run, force, safety),
        Command::LegacyEnv {
            monitor_sink,
            monitor_volume,
        } => legacy_env(&monitor_sink, &monitor_volume),
        Command::Trace { command } => trace(command),
        Command::Graph { dry_run } => graph(dry_run),
        Command::GraphDump => graph_dump_command(),
        Command::GraphCheck { state } => graph_check_command(state),
        Command::Quiet { volume, sink } => quiet(&sink, &volume),
        Command::MonitorTest {
            pair,
            volume,
            sink,
            dry_run,
            execute,
        } => monitor_test(pair, &sink, &volume, dry_run, execute),
        Command::RecordingBalanced {
            volume,
            monitor_sink,
            recording_sink,
            dry_run,
            execute,
        } => recording_balanced(
            &monitor_sink,
            recording_sink
                .as_deref()
                .unwrap_or(default_recording_sink()),
            &volume,
            dry_run,
            execute,
        ),
        Command::MonitorClear {
            pair,
            sink,
            dry_run,
            execute,
        } => monitor_clear(pair, &sink, dry_run, execute),
        Command::AudioDoctor { sink, source } => audio_doctor(&sink, &source),
        Command::RecoverAudio {
            dry_run,
            execute,
            sink,
            source,
            card,
            port,
            volume,
            restart_spotify,
            restart_cosmic_panel,
        } => recover_audio(
            dry_run,
            execute,
            &sink,
            &source,
            &card,
            &port,
            &volume,
            restart_spotify,
            restart_cosmic_panel,
        ),
        Command::VirtualMidi {
            dry_run,
            execute,
            midi_device,
            midi_port,
            output_client,
            output_port,
        } => virtual_midi_command(
            dry_run,
            execute,
            &midi_device,
            midi_port,
            &output_client,
            &output_port,
        ),
        Command::ArdourMidiDiagnose { output_client } => ardour_midi_diagnose(&output_client),
        Command::Devices => devices(),
        Command::Kits => kits(),
        Command::GenerateMidimap { device, kit } => generate_midimap_command(&device, &kit),
        Command::MapCheck { device, kit } => map_check(&device, &kit),
        Command::ProfileInspect { device, kit } => profile_inspect(&device, &kit),
        Command::Workbench { command } => workbench(command),
        Command::Policy => policy(),
    }
}

fn plan(
    kit: Kit,
    allow_experimental: bool,
    route_obs: bool,
    route_monitor: bool,
    monitor_sink: &str,
    monitor_volume: &str,
    streaming: bool,
) -> Result<(), String> {
    if kit.is_experimental() && !allow_experimental {
        return Err(format!(
            "kit '{}' is blocked by default; pass --allow-experimental for isolated diagnostics",
            kit.name()
        ));
    }

    println!("polyrhythm TD-50 launch plan");
    println!("kit: {}", kit.name());
    println!("midi device: {DEFAULT_MIDI_DEVICE}");
    println!("mapper client: {DEFAULT_MAPPER_CLIENT}");
    println!("mapper port: {DEFAULT_MAPPER_PORT}");
    println!("route monitor: {route_monitor}");
    println!("monitor sink: {monitor_sink}");
    println!("monitor volume clamp: {monitor_volume}");
    println!("route OBS: {route_obs}");
    let mut config = StartConfig::drs_default(home_dir(), repo_dir());
    config.route_monitor = route_monitor;
    config.route_obs = route_obs;
    config.monitor_sinks = vec![monitor_sink.to_string()];
    config.monitor_volume = monitor_volume.to_string();
    if streaming {
        config.streaming = true;
    }
    println!("streaming: {}", config.streaming);
    println!("velocity curve: {}", config.velocity_curve);
    println!("load timeout: {}s", config.load_timeout_secs);
    println!("direct low-volume monitor: {}", route_monitor);
    println!("cache: {}", cache_dir().display());
    println!();
    for op in plan_drs(&config) {
        println!("- {}", describe_op(&op));
    }
    println!();
    println!("No live clients were started. No PipeWire graph was probed.");
    let _ = write_event(TraceEvent::info(
        "plan",
        format!("kit={} route_obs={route_obs}", kit.name()),
    ));
    Ok(())
}

fn doctor(drs_midimap: &Path) -> Result<(), String> {
    let xml = fs::read_to_string(drs_midimap)
        .map_err(|err| format!("failed to read {}: {err}", drs_midimap.display()))?;
    let mapped = mapped_notes_from_midimap(&xml);
    let missing = missing_emitted_notes(&mapped);

    println!("polyrhythm offline doctor");
    println!("DRS midimap: {}", drs_midimap.display());
    println!("mapped notes: {}", mapped.len());

    if missing.is_empty() {
        println!("mapper coverage: ok");
    } else {
        println!("mapper coverage: missing notes {missing:?}");
        let _ = write_event(TraceEvent::error(
            "doctor_failed",
            "midimap coverage failed",
        ));
        return Err("DRS midimap does not cover all emitted mapper notes".to_string());
    }

    println!("live audio safety: ok (offline-only check)");
    println!("PipeWire graph probing: skipped");
    println!("ALSA client probing: skipped");
    let _ = write_event(TraceEvent::info("doctor_ok", "offline doctor passed"));
    Ok(())
}

fn start_command(
    kit: Kit,
    allow_experimental: bool,
    dry_run: bool,
    execute: bool,
    launch: LaunchOptions,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    if kit.is_experimental() && !allow_experimental {
        return Err(format!(
            "kit '{}' is blocked by default; pass --allow-experimental for isolated diagnostics",
            kit.name()
        ));
    }
    if kit != Kit::Drs {
        return Err("first start planning pass only supports DRS".to_string());
    }

    let preflight_config = PreflightConfig::drs_default(&home_dir(), &repo_dir());
    let preflight_report = run_preflight(&preflight_config);
    if has_failures(&preflight_report.checks) {
        let _ = write_event(TraceEvent::error(
            "start_dry_run_preflight_failed",
            "one or more preflight checks failed",
        ));
        print_preflight_report(Kit::Drs, &preflight_report);
        return Err("start dry-run preflight failed".to_string());
    }

    let mut config = StartConfig::drs_default(home_dir(), repo_dir());
    config.midi_client = preflight_report.midi_client;
    config.route_monitor = launch.route_monitor;
    config.route_obs = launch.route_obs;
    if launch.streaming {
        config.streaming = true;
    }
    let ops = plan_drs(&config);
    let manifest = if live {
        None
    } else {
        Some(
            write_manifest(&config, &ops)
                .map_err(|err| format!("failed to write dry-run manifest: {err}"))?,
        )
    };

    println!(
        "polyrhythm start {}",
        if live { "execute" } else { "dry-run" }
    );
    println!("kit: {}", kit.name());
    println!("run_id: {}", config.run_id);
    if let Some(manifest) = &manifest {
        println!("manifest: {}", manifest.display());
    }
    for op in &ops {
        println!("- {}", describe_op(op));
    }

    if live {
        let safety_config = if config.route_monitor {
            MonitorSafety::new(config.monitor_sinks.clone(), config.monitor_volume.clone())
        } else {
            let restore_volume =
                env::var("TD50_MONITOR_VOLUME").unwrap_or_else(|_| "50%".to_string());
            MonitorSafety::new(config.monitor_sinks.clone(), restore_volume)
        };
        for action in apply_safety(&safety_config) {
            println!("{action}");
        }
        let results = execute_drs(&config, &ops);
        for result in &results {
            let status = match &result.status {
                ExecutionStatus::Ok => "ok".to_string(),
                ExecutionStatus::Failed(reason) => format!("failed: {reason}"),
                ExecutionStatus::Skipped(reason) => format!("skipped: {reason}"),
            };
            println!("{status}: {}", result.description);
        }
        for action in restore(&safety_config) {
            println!("{action}");
        }
        println!("PipeWire graph probing: skipped");
        let failed = results
            .iter()
            .any(|result| result.required && matches!(result.status, ExecutionStatus::Failed(_)));
        let _ = write_event(TraceEvent::info(
            "start_execute",
            format!("run_id={} failed={failed}", config.run_id),
        ));
        if failed {
            Err("start execution had failures".to_string())
        } else {
            Ok(())
        }
    } else {
        println!("live audio start: skipped");
        println!("PipeWire graph probing: skipped");
        let manifest_text = manifest
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let _ = write_event(TraceEvent::info(
            "start_dry_run",
            format!(
                "run_id={} kit=drs manifest={}",
                config.run_id, manifest_text
            ),
        ));
        Ok(())
    }
}

fn switch_command(
    kit: Kit,
    allow_experimental: bool,
    dry_run: bool,
    execute: bool,
    launch: LaunchOptions,
    safety: bool,
) -> Result<(), String> {
    println!(
        "polyrhythm switch {}",
        if execute { "execute" } else { "dry-run" }
    );
    let _ = write_event(TraceEvent::info(
        if execute {
            "switch_execute"
        } else {
            "switch_dry_run"
        },
        format!("kit={}", kit.name()),
    ));
    if execute {
        stop_command(false, true, safety)?;
        start_command(kit, allow_experimental, dry_run, execute, launch)
    } else {
        stop_command(true, true, safety)?;
        start_command(kit, allow_experimental, true, false, launch)
    }
}

fn preflight(kit: Kit, allow_experimental: bool) -> Result<(), String> {
    if kit.is_experimental() && !allow_experimental {
        return Err(format!(
            "kit '{}' is blocked by default; pass --allow-experimental for isolated diagnostics",
            kit.name()
        ));
    }
    if kit != Kit::Drs {
        return Err("first preflight pass only supports DRS".to_string());
    }

    let config = PreflightConfig::drs_default(&home_dir(), &repo_dir());
    let report = run_preflight(&config);
    print_preflight_report(kit, &report);

    if has_failures(&report.checks) {
        let _ = write_event(TraceEvent::error(
            "preflight_failed",
            "one or more checks failed",
        ));
        Err("preflight failed".to_string())
    } else {
        let _ = write_event(TraceEvent::info("preflight_ok", "DRS preflight passed"));
        Ok(())
    }
}

fn print_preflight_report(kit: Kit, report: &PreflightReport) {
    println!("polyrhythm preflight");
    println!("kit: {}", kit.name());
    for check in &report.checks {
        let status = match check.status {
            CheckStatus::Ok => "ok",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "fail",
        };
        println!("{status}: {} — {}", check.name, check.detail);
    }
    if let Some(client) = report.midi_client {
        println!("detected MIDI client: {client}");
    }
    println!("PipeWire graph probing: skipped");
    println!("live audio start: skipped");
}

fn status(strict: bool) -> Result<(), String> {
    let cache = cache_dir();
    println!("polyrhythm status");
    println!("cache: {}", cache.display());
    let statuses = pid_statuses(&cache);
    for status in &statuses {
        let pid = status
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "none".to_string());
        let state = match status.state {
            ProcessState::Running => "running",
            ProcessState::Zombie => "zombie",
            ProcessState::Dead => "dead",
        };
        println!(
            "{}: pid={} state={} pidfile={}",
            status.label,
            pid,
            state,
            status.pidfile.display()
        );
    }
    println!("PipeWire graph probing: skipped");
    let _ = write_event(TraceEvent::info("status", "status inspected pidfiles"));
    if strict {
        let drumgizmo_ok = statuses.iter().any(|status| {
            status.label == "drumgizmo" && matches!(status.state, ProcessState::Running)
        });
        let mapper_ok = statuses.iter().any(|status| {
            status.label == "hihat mapper" && matches!(status.state, ProcessState::Running)
        });
        if !drumgizmo_ok || !mapper_ok {
            return Err(format!(
                "strict status failed: drumgizmo_running={drumgizmo_ok} mapper_running={mapper_ok}"
            ));
        }
        println!("strict: ok");
    }
    Ok(())
}

fn stop_command(dry_run: bool, force: bool, safety: bool) -> Result<(), String> {
    let cache = cache_dir();
    let safety_config =
        MonitorSafety::new(vec![DEFAULT_MONITOR_SINK.to_string()], "50%".to_string());
    if safety && !dry_run {
        for action in apply_safety(&safety_config) {
            println!("{action}");
        }
    }
    let actions = stop(&cache, StopOptions { dry_run, force })
        .map_err(|err| format!("failed to stop TD-50 clients: {err}"))?;
    println!("polyrhythm stop");
    println!("cache: {}", cache.display());
    for action in actions {
        println!("{action}");
    }
    if dry_run {
        println!("dry run: no processes were signaled and no pidfiles were removed");
    }
    println!("PipeWire/WirePlumber restart: skipped");
    let event = if dry_run { "stop_dry_run" } else { "stop" };
    let _ = write_event(TraceEvent::warn(
        event,
        format!("force={force}; PipeWire restart skipped"),
    ));
    Ok(())
}

fn legacy_env(monitor_sink: &str, monitor_volume: &str) -> Result<(), String> {
    println!("export TD50_ROUTE_AUDIO=1");
    println!("export TD50_ROUTE_MONITOR=1");
    println!("export TD50_ROUTE_OBS=0");
    println!("export TD50_MONITOR_SINKS='{monitor_sink}'");
    println!("export TD50_MONITOR_VOLUME='{monitor_volume}'");
    println!("export TD50_ALLOW_EXPERIMENTAL_KITS=0");
    let _ = write_event(TraceEvent::info(
        "legacy_env",
        "printed safe legacy environment",
    ));
    Ok(())
}

fn trace(command: TraceCommand) -> Result<(), String> {
    match command {
        TraceCommand::Path => {
            println!("{}", trace_path().display());
            Ok(())
        }
        TraceCommand::Tail { lines } => {
            for line in trace_tail(lines).map_err(|err| format!("failed to read trace: {err}"))? {
                println!("{line}");
            }
            Ok(())
        }
    }
}

fn monitor_test(
    pair: MonitorPairArg,
    sink: &str,
    volume: &str,
    dry_run: bool,
    execute: bool,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    let sink = if sink.is_empty() {
        default_monitor_sink()
    } else {
        sink
    };
    let setup_ops = plan_monitor_setup(sink, volume);
    let ops = plan_monitor(pair.into(), sink);
    println!(
        "polyrhythm monitor-test {}",
        if live { "execute" } else { "dry-run" }
    );
    println!("pair: {pair:?}");
    println!("sink: {sink}");
    println!("volume: {volume}");
    for op in &setup_ops {
        println!("- {}", describe_monitor_setup_op(op));
    }
    for op in &ops {
        println!("- link {} -> {}", op.source, op.target);
    }
    if live {
        guard_no_obs_speaker_monitor()?;
        let setup_results = execute_monitor_setup(&setup_ops);
        for result in &setup_results {
            println!("{result}");
        }
        if monitor_setup_failed(&setup_results) {
            let _ = write_event(TraceEvent::error(
                "monitor_test_setup_failed",
                format!("pair={pair:?} sink={sink} volume={volume}"),
            ));
            return Err("monitor-test setup failed".to_string());
        }
        for result in execute_monitor(MonitorAction::Link, &ops) {
            println!("{result}");
        }
        let snapshot = graph_dump().map_err(|err| format!("graph dump failed: {err}"))?;
        print_graph_summary(&snapshot);
        let failures = graph_check(&snapshot, DesiredState::OverheadMonitor);
        if failures.is_empty() {
            println!("graph-check overhead-monitor: ok");
        } else {
            println!("graph-check overhead-monitor: failed");
            for failure in &failures {
                println!("  {failure}");
            }
        }
        let _ = write_event(TraceEvent::warn(
            "monitor_test_execute",
            format!(
                "pair={pair:?} sink={sink} volume={volume} failures={}",
                failures.len()
            ),
        ));
    } else {
        println!("live monitor links: skipped");
    }
    Ok(())
}

fn recording_balanced(
    monitor_sink: &str,
    recording_sink: &str,
    volume: &str,
    dry_run: bool,
    execute: bool,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    let monitor_sink = if monitor_sink.is_empty() {
        default_monitor_sink()
    } else {
        monitor_sink
    };
    let recording_sink = if recording_sink.is_empty() {
        default_recording_sink()
    } else {
        recording_sink
    };
    let setup_ops = plan_monitor_setup(monitor_sink, volume);
    let monitor_ops = plan_monitor(MonitorPair::PracticeRecordingBalanced, monitor_sink);
    let recording_ops = recording_plan(RecordingMix::Balanced, recording_sink);
    println!(
        "polyrhythm recording-balanced {}",
        if live { "execute" } else { "dry-run" }
    );
    println!("monitor sink: {monitor_sink}");
    println!("recording sink: {recording_sink}");
    println!("monitor volume: {volume}");
    for op in &setup_ops {
        println!("- {}", describe_monitor_setup_op(op));
    }
    for op in &monitor_ops {
        println!("- player link {} -> {}", op.source, op.target);
    }
    for op in &recording_ops {
        println!("- recording link {} -> {}", op.source, op.target);
    }
    if live {
        guard_no_obs_speaker_monitor()?;
        let setup_results = execute_monitor_setup(&setup_ops);
        for result in &setup_results {
            println!("{result}");
        }
        if monitor_setup_failed(&setup_results) {
            let _ = write_event(TraceEvent::error(
                "recording_balanced_setup_failed",
                format!(
                    "monitor_sink={monitor_sink} recording_sink={recording_sink} volume={volume}"
                ),
            ));
            return Err("recording-balanced setup failed".to_string());
        }
        for result in execute_monitor(MonitorAction::Link, &monitor_ops) {
            println!("{result}");
        }
        for result in execute_monitor(MonitorAction::Link, &recording_ops) {
            println!("{result}");
        }
        let snapshot = graph_dump().map_err(|err| format!("graph dump failed: {err}"))?;
        print_graph_summary(&snapshot);
        let _ = write_event(TraceEvent::warn(
            "recording_balanced_execute",
            format!("monitor_sink={monitor_sink} recording_sink={recording_sink}"),
        ));
    } else {
        println!("live recording-balanced links: skipped");
    }
    Ok(())
}

fn monitor_clear(
    pair: MonitorPairArg,
    sink: &str,
    dry_run: bool,
    execute: bool,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    let sink = if sink.is_empty() {
        default_monitor_sink()
    } else {
        sink
    };
    let ops = plan_monitor(pair.into(), sink);
    println!(
        "polyrhythm monitor-clear {}",
        if live { "execute" } else { "dry-run" }
    );
    for op in &ops {
        println!("- clear {} -> {}", op.source, op.target);
    }
    if live {
        for result in execute_monitor(MonitorAction::Clear, &ops) {
            println!("{result}");
        }
        let snapshot = graph_dump().map_err(|err| format!("graph dump failed: {err}"))?;
        print_graph_summary(&snapshot);
        let _ = write_event(TraceEvent::warn(
            "monitor_clear_execute",
            format!("pair={pair:?} sink={sink}"),
        ));
    } else {
        println!("live monitor clear: skipped");
    }
    Ok(())
}

fn graph_dump_command() -> Result<(), String> {
    let snapshot = graph_dump().map_err(|err| format!("graph dump failed: {err}"))?;
    print_graph_summary(&snapshot);
    let _ = write_event(TraceEvent::info(
        "graph_dump",
        format!("path={}", snapshot.path.display()),
    ));
    Ok(())
}

fn graph_check_command(state: GraphState) -> Result<(), String> {
    let snapshot = graph_dump().map_err(|err| format!("graph dump failed: {err}"))?;
    print_graph_summary(&snapshot);
    let failures = graph_check(&snapshot, state.into());
    if failures.is_empty() {
        println!("graph-check: ok");
        let _ = write_event(TraceEvent::info(
            "graph_check_ok",
            format!("state={state:?}"),
        ));
        Ok(())
    } else {
        println!("graph-check: failed");
        for failure in &failures {
            println!("  {failure}");
        }
        let _ = write_event(TraceEvent::error(
            "graph_check_failed",
            format!("state={state:?} failures={}", failures.len()),
        ));
        Err("graph check failed".to_string())
    }
}

fn guard_no_obs_speaker_monitor() -> Result<(), String> {
    let snapshot = graph_dump().map_err(|err| format!("pre-monitor graph dump failed: {err}"))?;
    let failures = graph_check(&snapshot, DesiredState::EngineOnly);
    let obs_failures: Vec<_> = failures
        .iter()
        .filter(|failure| failure.contains("OBS is receiving speaker monitor feed"))
        .collect();
    if obs_failures.is_empty() {
        return Ok(());
    }

    println!("pre-monitor safety guard: failed");
    print_graph_summary(&snapshot);
    for failure in &obs_failures {
        println!("  {failure}");
    }
    let _ = write_event(TraceEvent::error(
        "monitor_test_blocked_obs_monitor",
        format!("failures={}", obs_failures.len()),
    ));
    Err("refusing live monitor routing while OBS receives the speaker monitor feed".to_string())
}

fn audio_doctor(sink: &str, source: &str) -> Result<(), String> {
    let report = audio_doctor_report(sink, source);
    println!("polyrhythm audio-doctor");
    println!("sink: {sink}");
    println!("source: {source}");
    for check in &report.checks {
        let status = if check.ok { "ok" } else { "fail" };
        println!("{status}: {} — {}", check.name, check.detail);
    }
    if report.ok() {
        let _ = write_event(TraceEvent::info(
            "audio_doctor_ok",
            "desktop audio checks passed",
        ));
        Ok(())
    } else {
        let _ = write_event(TraceEvent::error(
            "audio_doctor_failed",
            "one or more desktop audio checks failed",
        ));
        Err("audio doctor failed".to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn recover_audio(
    dry_run: bool,
    execute: bool,
    sink: &str,
    source: &str,
    card: &str,
    port: &str,
    volume: &str,
    restart_spotify: bool,
    restart_cosmic_panel: bool,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    let options = RecoverAudioOptions {
        execute: live,
        sink: sink.to_string(),
        source: source.to_string(),
        card: card.to_string(),
        port: port.to_string(),
        volume: volume.to_string(),
        restart_spotify,
        restart_cosmic_panel,
    };
    println!(
        "polyrhythm recover-audio {}",
        if live { "execute" } else { "dry-run" }
    );
    for action in recover_audio_steps(&options) {
        println!("{action}");
    }
    if live {
        audio_doctor(sink, source)?;
    }
    let _ = write_event(TraceEvent::warn(
        if live { "recover_audio_execute" } else { "recover_audio_dry_run" },
        format!("sink={sink} source={source} restart_spotify={restart_spotify} restart_cosmic_panel={restart_cosmic_panel}"),
    ));
    Ok(())
}

fn quiet(sink: &str, volume: &str) -> Result<(), String> {
    println!("quiet: setting {sink} to {volume}");
    let safety = MonitorSafety {
        sinks: vec![sink.to_string()],
        safety_volume: volume.to_string(),
        restore_volume: volume.to_string(),
        mute: false,
    };
    for result in apply_safety(&safety) {
        println!("{result}");
    }
    println!("DrumGizmo/mapper untouched");
    let _ = write_event(TraceEvent::warn(
        "quiet",
        format!("sink={sink} volume={volume}"),
    ));
    Ok(())
}

fn graph(dry_run: bool) -> Result<(), String> {
    let candidates = ["qpwgraph", "helvum"];
    let Some(tool) = candidates.iter().find(|tool| command_exists(tool)) else {
        return Err("no PipeWire graph viewer found; install qpwgraph or helvum".to_string());
    };
    println!("PipeWire graph viewer: {tool}");
    println!("This is an explicit manual visualization tool; polyrhythm automation still avoids broad graph probing.");
    let _ = write_event(TraceEvent::info(
        "graph",
        format!("tool={tool} dry_run={dry_run}"),
    ));
    if dry_run {
        return Ok(());
    }
    std::process::Command::new(tool)
        .spawn()
        .map_err(|err| format!("failed to launch {tool}: {err}"))?;
    Ok(())
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", command))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn virtual_midi_command(
    dry_run: bool,
    execute: bool,
    midi_device: &str,
    midi_port: u8,
    output_client: &str,
    output_port: &str,
) -> Result<(), String> {
    let live = execute;
    if !dry_run && !live {
        return Err("use --dry-run or --execute".to_string());
    }
    let midi_client = detect_midi_client(midi_device, std::time::Duration::from_secs(2))
        .ok_or_else(|| format!("{midi_device} not visible via bounded aconnect"))?;
    let mut start_config = StartConfig::drs_default(home_dir(), repo_dir());
    start_config.midi_client = Some(midi_client);
    start_config.midi_port = midi_port;
    let config = VirtualMidiConfig {
        cache_dir: cache_dir(),
        midi_client,
        midi_port,
        mapper_bin: start_config.mapper_bin,
        velocity_curve: start_config.velocity_curve,
        client_name: output_client.to_string(),
        port_name: output_port.to_string(),
    };
    let ops = virtual_midi::plan(&config);
    let plan_path = cache_dir().join("virtual-midi-plan.json");
    virtual_midi::write_plan(&plan_path, &config, &ops)
        .map_err(|err| format!("failed to write virtual MIDI plan: {err}"))?;
    println!(
        "polyrhythm virtual-midi {}",
        if live { "execute" } else { "dry-run" }
    );
    println!("midi device: {midi_device}");
    println!("midi source: {midi_client}:{midi_port}");
    println!("output: {output_client}:{output_port}");
    println!("plan: {}", plan_path.display());
    println!("DrumGizmo: skipped");
    println!("PipeWire audio routing: skipped");
    for op in &ops {
        println!("- {}", virtual_midi::describe(op));
    }
    if live {
        let results = virtual_midi::execute(&config, &ops);
        for result in &results {
            let status = match &result.status {
                VirtualMidiStatus::Ok => "ok".to_string(),
                VirtualMidiStatus::OkWithDetails(details) => {
                    format!("ok: {}", details.join(", "))
                }
                VirtualMidiStatus::Failed(reason) => format!("failed: {reason}"),
                VirtualMidiStatus::Skipped(reason) => format!("skipped: {reason}"),
            };
            println!("{status}: {}", result.description);
        }
        let failed = results
            .iter()
            .any(|result| result.required && matches!(result.status, VirtualMidiStatus::Failed(_)));
        if failed {
            Err("virtual MIDI start had failures".to_string())
        } else {
            println!("Ardour: create a MIDI track and select input {output_client}:{output_port}");
            for result in &results {
                if let VirtualMidiStatus::OkWithDetails(details) = &result.status {
                    println!("JACK/PipeWire MIDI bridge candidates:");
                    for detail in details {
                        println!("- {detail}");
                    }
                }
            }
            Ok(())
        }
    } else {
        println!("live mapper start: skipped");
        Ok(())
    }
}

fn ardour_midi_diagnose(output_client: &str) -> Result<(), String> {
    println!("polyrhythm ardour-midi-diagnose");
    println!("mutation: none");
    let ardour_pids = process_ids_matching("ardour");
    if ardour_pids.is_empty() {
        println!("Ardour: not running");
    } else {
        println!("Ardour pids: {}", join_u32(&ardour_pids));
        for pid in &ardour_pids {
            println!(
                "Ardour pid {pid} backend hint: {}",
                ardour_backend_hint(*pid)
            );
        }
    }

    let pw_ports = bounded_pw_link_io(std::time::Duration::from_secs(5));
    match &pw_ports {
        Ok(lines) => {
            let bridge_candidates =
                virtual_midi::jack_midi_bridge_candidates(&lines.join("\n"), output_client);
            if bridge_candidates.is_empty() {
                println!("Polyrhythm JACK/PipeWire source: missing for {output_client}");
            } else {
                println!("Polyrhythm JACK/PipeWire source candidates:");
                for candidate in &bridge_candidates {
                    println!("- {candidate}");
                }
            }
            let ardour_ports = lines
                .iter()
                .filter(|line| line.to_ascii_lowercase().contains("ardour"))
                .cloned()
                .collect::<Vec<_>>();
            if ardour_ports.is_empty() {
                println!("Ardour JACK/PipeWire ports: none visible");
            } else {
                println!("Ardour JACK/PipeWire ports:");
                for port in &ardour_ports {
                    println!("- {port}");
                }
            }
            if !bridge_candidates.is_empty() && !ardour_ports.is_empty() {
                println!("Expected edge: Polyrhythm bridge candidate -> Ardour MIDI input port");
                println!("Next action: inspect Ardour MIDI track input names before enabling auto-connect.");
            } else if !bridge_candidates.is_empty() {
                println!("Next action: start Ardour with JACK/PipeWire backend or use ALSA sequencer input {output_client}:out.");
            } else {
                println!("Next action: run polyrhythm virtual-midi --execute before diagnosing Ardour graph input.");
            }
        }
        Err(reason) => {
            println!("PipeWire/JACK MIDI ports: skipped: {reason}");
            println!("Next action: use ALSA sequencer input {output_client}:out if Ardour is on ALSA backend.");
        }
    }
    Ok(())
}

fn process_ids_matching(needle: &str) -> Vec<u32> {
    let output = std::process::Command::new("pgrep")
        .arg("-fi")
        .arg(needle)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
                .map(|cmdline| cmdline.to_ascii_lowercase().contains("ardour"))
                .unwrap_or(false)
        })
        .collect()
}

fn ardour_backend_hint(pid: u32) -> String {
    let fd_dir = format!("/proc/{pid}/fd");
    let Ok(entries) = std::fs::read_dir(fd_dir) else {
        return "unknown; cannot read process fd table".to_string();
    };
    let mut has_snd_seq = false;
    let mut has_snd_pcm = false;
    let mut has_pipewire = false;
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        if target.contains("/dev/snd/seq") {
            has_snd_seq = true;
        }
        if target.contains("/dev/snd/pcm") {
            has_snd_pcm = true;
        }
        if target.contains("pipewire") || target.contains("jack") {
            has_pipewire = true;
        }
    }
    match (has_snd_seq, has_snd_pcm, has_pipewire) {
        (true, true, false) => {
            "ALSA backend likely; use ALSA sequencer input if mapper is not in JACK graph"
                .to_string()
        }
        (_, _, true) => {
            "JACK/PipeWire backend likely; inspect Ardour JACK/PipeWire ports".to_string()
        }
        (true, false, false) => "ALSA sequencer MIDI visible; audio backend unclear".to_string(),
        _ => "unknown".to_string(),
    }
}

fn bounded_pw_link_io(timeout: std::time::Duration) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("timeout")
        .arg(format!("{}s", timeout.as_secs().max(1)))
        .arg("pw-link")
        .arg("-io")
        .output()
        .map_err(|err| format!("pw-link unavailable: {err}"))?;
    if !output.status.success() {
        return Err(format!("pw-link exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("midi")
                || lower.contains("ardour")
                || lower.contains("polyrhythm")
                || lower.contains("mapper")
        })
        .collect())
}

fn join_u32(values: &[u32]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn devices() -> Result<(), String> {
    println!("polyrhythm device profiles");
    for device in builtin_devices(&repo_dir()) {
        println!(
            "{}\t{}\tinputs={}",
            device.id,
            device.name,
            device.inputs.len()
        );
    }
    Ok(())
}

fn kits() -> Result<(), String> {
    println!("polyrhythm kit profiles");
    for kit in builtin_kits(&repo_dir(), &home_dir()) {
        println!(
            "{}\t{}\tkit={}\tmappings={}",
            kit.id,
            kit.name,
            kit.kit_xml.display(),
            kit.mappings.len()
        );
    }
    Ok(())
}

fn generate_midimap_command(device_id: &str, kit_id: &str) -> Result<(), String> {
    let device = find_device(&repo_dir(), device_id)
        .ok_or_else(|| format!("unknown device profile '{device_id}'"))?;
    let kit = find_kit(&repo_dir(), &home_dir(), kit_id)
        .ok_or_else(|| format!("unknown kit profile '{kit_id}'"))?;
    let (path, generated) = write_generated_midimap(&cache_dir(), &device, &kit)
        .map_err(|err| format!("failed to write generated midimap: {err}"))?;
    println!("generated: {}", path.display());
    println!("device: {} ({})", device.id, device.name);
    println!("kit: {} ({})", kit.id, kit.name);
    println!("mapped notes: {}", generated.mapped.len());
    for warning in &generated.warnings {
        println!("warn: {warning}");
    }
    if generated.warnings.is_empty() {
        println!("warnings: 0");
    }
    Ok(())
}

fn map_check(device_id: &str, kit_id: &str) -> Result<(), String> {
    let device = find_device(&repo_dir(), device_id)
        .ok_or_else(|| format!("unknown device profile '{device_id}'"))?;
    let kit = find_kit(&repo_dir(), &home_dir(), kit_id)
        .ok_or_else(|| format!("unknown kit profile '{kit_id}'"))?;
    let generated = crate::profiles::generate_midimap(&device, &kit);
    println!("polyrhythm map-check");
    println!("device: {} ({})", device.id, device.name);
    println!("kit: {} ({})", kit.id, kit.name);
    for mapping in &generated.mapped {
        let qualifier = kit
            .mappings
            .get(&mapping.intent)
            .map(|target| if target.fallback { "fallback" } else { "exact" })
            .unwrap_or("unknown");
        println!(
            "ok: {} note {} -> {} ({})",
            mapping.intent, mapping.device_note, mapping.instrument, qualifier
        );
    }
    for warning in &generated.warnings {
        println!("warn: {warning}");
    }
    if generated.warnings.is_empty() {
        println!("coverage: ok");
        Ok(())
    } else {
        Err(format!(
            "map-check found {} warning(s)",
            generated.warnings.len()
        ))
    }
}

fn profile_inspect(device_id: &str, kit_id: &str) -> Result<(), String> {
    let device = find_device(&repo_dir(), device_id)
        .ok_or_else(|| format!("unknown device profile '{device_id}'"))?;
    let kit = find_kit(&repo_dir(), &home_dir(), kit_id)
        .ok_or_else(|| format!("unknown kit profile '{kit_id}'"))?;
    println!("polyrhythm profile-inspect");
    println!("canonical format: Pikl");
    println!("generated compatibility target: DrumGizmo midimap XML");
    println!("device: {} ({})", device.id, device.name);
    println!("kit: {} ({})", kit.id, kit.name);
    println!("kit xml: {}", kit.kit_xml.display());
    println!("sample params: {}", kit.sample_params);
    println!("device inputs:");
    for input in &device.inputs {
        let notes = input
            .notes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        match kit.mappings.get(&input.intent) {
            Some(target) => {
                let qualifier = if target.fallback { "fallback" } else { "exact" };
                println!(
                    "  {}: {} notes=[{}] -> note {} instr {} ({})",
                    input.id, input.intent, notes, target.note, target.instrument, qualifier
                );
            }
            None => println!(
                "  {}: {} notes=[{}] -> unmapped",
                input.id, input.intent, notes
            ),
        }
    }
    Ok(())
}

fn workbench(command: WorkbenchCommand) -> Result<(), String> {
    match command {
        WorkbenchCommand::Coverage { device, kit, jsonl } => {
            workbench_coverage_command(&device, &kit, jsonl.as_deref())
        }
        WorkbenchCommand::Replay { trace, device, kit } => {
            workbench_replay_command(&trace, &device, &kit)
        }
    }
}

fn workbench_replay_command(path: &Path, device_id: &str, kit_id: &str) -> Result<(), String> {
    let device = find_device(&repo_dir(), device_id)
        .ok_or_else(|| format!("unknown device profile '{device_id}'"))?;
    let kit = find_kit(&repo_dir(), &home_dir(), kit_id)
        .ok_or_else(|| format!("unknown kit profile '{kit_id}'"))?;
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let report = workbench_replay::replay_jsonl(&text, &device, &kit)?;
    println!("polyrhythm workbench replay");
    println!("trace: {}", path.display());
    println!("device: {} ({})", device.id, device.name);
    println!("kit: {} ({})", kit.id, kit.name);
    println!("live audio: skipped");
    println!("PipeWire graph probing: skipped");
    println!("raw events: {}", report.raw_events);
    println!("expected outputs: {}", report.expected_outputs);
    println!("output events: {}", report.output_events);
    for output in &report.outputs {
        println!("out: {output:?}");
    }
    for mismatch in &report.mismatches {
        println!(
            "mismatch[{}]: expected {:?} actual {:?}",
            mismatch.index, mismatch.expected, mismatch.actual
        );
    }
    for warning in &report.warnings {
        println!("warn: {warning}");
    }
    if report.expected_outputs > 0 && report.mismatches.is_empty() {
        println!("regression: ok");
    } else if report.expected_outputs > 0 {
        println!("regression: failed");
    } else {
        println!("regression: unchecked");
    }
    if !report.warnings.is_empty() {
        Err(format!(
            "workbench replay produced {} warning(s)",
            report.warnings.len()
        ))
    } else if !report.mismatches.is_empty() {
        Err(format!(
            "workbench replay found {} mismatch(es)",
            report.mismatches.len()
        ))
    } else {
        Ok(())
    }
}

fn workbench_coverage_command(
    device_id: &str,
    kit_id: &str,
    jsonl: Option<&Path>,
) -> Result<(), String> {
    let device = find_device(&repo_dir(), device_id)
        .ok_or_else(|| format!("unknown device profile '{device_id}'"))?;
    let kit = find_kit(&repo_dir(), &home_dir(), kit_id)
        .ok_or_else(|| format!("unknown kit profile '{kit_id}'"))?;
    let report = workbench_coverage::report(&device, &kit);
    println!("polyrhythm workbench coverage");
    println!("device: {} ({})", device.id, device.name);
    println!("kit: {} ({})", kit.id, kit.name);
    println!("live audio: skipped");
    println!("PipeWire graph probing: skipped");
    println!("intent\tinput\tdevice_notes\ttarget\tquality");
    for row in &report.rows {
        let notes = row
            .device_notes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let target = match (row.target_note, row.instrument.as_deref()) {
            (Some(note), Some(instrument)) => format!("note {note} instr {instrument}"),
            _ => "unsupported".to_string(),
        };
        println!(
            "{}\t{}\t[{}]\t{}\t{}",
            row.intent,
            row.input_id,
            notes,
            target,
            row.quality.as_str()
        );
    }
    let unsupported = report
        .rows
        .iter()
        .filter(|row| row.quality == crate::workbench::event::MappingQuality::Unsupported)
        .count();
    let fallback = report
        .rows
        .iter()
        .filter(|row| row.quality == crate::workbench::event::MappingQuality::Fallback)
        .count();
    println!(
        "summary: rows={} fallback={} unsupported={unsupported}",
        report.rows.len(),
        fallback
    );
    if let Some(path) = jsonl {
        workbench_trace::append_coverage(path, &report)
            .map_err(|err| format!("failed to write workbench JSONL trace: {err}"))?;
        println!("jsonl: {}", path.display());
    }
    if unsupported == 0 {
        Ok(())
    } else {
        Err(format!(
            "workbench coverage found {unsupported} unsupported intent(s)"
        ))
    }
}

fn policy() -> Result<(), String> {
    println!("polyrhythm safety policy");
    println!("- DRS is the only default kit path.");
    println!("- Alternate kits are explicit diagnostics only.");
    println!("- OBS routing is off by default.");
    println!("- Normal controls must not restart PipeWire/WirePlumber.");
    println!("- PipeWire recovery must use recover-audio so COSMIC applet/client state is refreshed too.");
    println!("- Live monitor routing is blocked while OBS receives the speaker monitor feed.");
    println!("- Normal controls must not run broad pw-link graph discovery.");
    println!("- The safe default monitor sink is {DEFAULT_MONITOR_SINK}.");
    println!("- The future ALSA mapper must preserve client '{DEFAULT_MAPPER_CLIENT}' and port '{DEFAULT_MAPPER_PORT}'.");
    let _ = write_event(TraceEvent::info("policy", "printed safety policy"));
    Ok(())
}

fn missing_emitted_notes(mapped: &BTreeSet<u8>) -> Vec<u8> {
    DRS_EMITTED_NOTES
        .iter()
        .copied()
        .filter(|note| !mapped.contains(note))
        .collect()
}

fn cache_dir() -> PathBuf {
    env::var_os("TD50_CACHE")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(DEFAULT_CACHE)))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CACHE))
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn repo_dir() -> PathBuf {
    if let Some(repo) = env::var_os("POLYRHYTHM_REPO_DIR").map(PathBuf::from) {
        return repo;
    }
    if let Ok(current) = env::current_dir() {
        if current.join("Cargo.toml").exists() && current.join("profiles").exists() {
            return current;
        }
    }
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
        .and_then(|bin| bin.parent().map(PathBuf::from))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drs_is_not_experimental() {
        assert!(!Kit::Drs.is_experimental());
        assert!(Kit::Muldjord.is_experimental());
    }

    #[test]
    fn missing_notes_reports_unmapped_emissions() {
        let mapped = BTreeSet::from([35, 36]);
        let missing = missing_emitted_notes(&mapped);
        assert!(missing.contains(&37));
    }
}
