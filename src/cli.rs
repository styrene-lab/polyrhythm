use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::td50_mapper::{mapped_notes_from_midimap, DRS_EMITTED_NOTES};

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

    /// Print environment variables needed to run the legacy DRS shell path safely.
    LegacyEnv {
        #[arg(long, default_value = DEFAULT_MONITOR_SINK)]
        monitor_sink: String,
        #[arg(long, default_value = "75%")]
        monitor_volume: String,
    },

    /// Print the current safety policy encoded by the CLI.
    Policy,
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
        Command::LegacyEnv {
            monitor_sink,
            monitor_volume,
        } => legacy_env(&monitor_sink, &monitor_volume),
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
        return Err("DRS midimap does not cover all emitted mapper notes".to_string());
    }

    println!("live audio safety: ok (offline-only check)");
    println!("PipeWire graph probing: skipped");
    println!("ALSA client probing: skipped");
    Ok(())
}

fn legacy_env(monitor_sink: &str, monitor_volume: &str) -> Result<(), String> {
    println!("export TD50_ROUTE_AUDIO=1");
    println!("export TD50_ROUTE_MONITOR=1");
    println!("export TD50_ROUTE_OBS=0");
    println!("export TD50_MONITOR_SINKS='{monitor_sink}'");
    println!("export TD50_MONITOR_VOLUME='{monitor_volume}'");
    println!("export TD50_ALLOW_EXPERIMENTAL_KITS=0");
    Ok(())
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
