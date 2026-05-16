use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::audio::{apply_safety, restore, MonitorSafety};
use crate::preflight::{
    has_failures, run as run_preflight, CheckStatus, PreflightConfig, PreflightReport,
};
use crate::process::{pid_statuses, stop, ProcessState, StopOptions};
use crate::start::{
    describe_op, execute_drs, plan_drs, write_manifest, ExecutionStatus, StartConfig,
};
use crate::td50_mapper::{mapped_notes_from_midimap, DRS_EMITTED_NOTES};
use crate::trace::{tail as trace_tail, trace_path, write_event, TraceEvent};

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
        #[arg(long, default_value_t = true)]
        route_monitor: bool,
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        monitor_sink: String,
        #[arg(long, default_value = "75%")]
        monitor_volume: String,
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
        #[arg(long, default_value_t = true)]
        route_monitor: bool,
        #[arg(long, default_value_t = false)]
        route_obs: bool,
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
        #[arg(long, default_value = "75%")]
        monitor_volume: String,
    },

    /// Inspect the JSONL trace log.
    Trace {
        #[command(subcommand)]
        command: TraceCommand,
    },

    /// Print the current safety policy encoded by the CLI.
    Policy,
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
        } => plan(
            kit,
            allow_experimental,
            route_obs,
            route_monitor,
            &monitor_sink,
            &monitor_volume,
        ),
        Command::Doctor { drs_midimap } => doctor(&drs_midimap),
        Command::Start {
            kit,
            allow_experimental,
            dry_run,
            execute,
            route_monitor,
            route_obs,
        } => start_command(
            kit,
            allow_experimental,
            dry_run,
            execute,
            route_monitor,
            route_obs,
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
    println!("cache: {}", cache_dir().display());
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
    route_monitor: bool,
    route_obs: bool,
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
    config.route_monitor = route_monitor;
    config.route_obs = route_obs;
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
        let safety_config =
            MonitorSafety::new(config.monitor_sinks.clone(), config.monitor_volume.clone());
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
            .any(|result| matches!(result.status, ExecutionStatus::Failed(_)));
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
        MonitorSafety::new(vec![DEFAULT_MONITOR_SINK.to_string()], "75%".to_string());
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

fn policy() -> Result<(), String> {
    println!("polyrhythm safety policy");
    println!("- DRS is the only default kit path.");
    println!("- Alternate kits are explicit diagnostics only.");
    println!("- OBS routing is off by default.");
    println!("- Normal controls must not restart PipeWire/WirePlumber.");
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
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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
