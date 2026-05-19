use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::workbench::coverage::CoverageReport;
use crate::workbench::event::{
    CanonicalMatched, MappingQuality, RawMidiEvent, RawObserved, TargetResolved, WorkbenchEvent,
    WorkbenchWarning,
};

pub fn append_event(path: &Path, event: &WorkbenchEvent) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", encode_event(event))
}

pub fn append_coverage(path: &Path, report: &CoverageReport) -> io::Result<()> {
    for event in coverage_events(report) {
        append_event(path, &event)?;
    }
    Ok(())
}

pub fn coverage_events(report: &CoverageReport) -> Vec<WorkbenchEvent> {
    let mut events = Vec::new();
    for row in &report.rows {
        match (&row.target_note, &row.instrument) {
            (Some(note), Some(instrument)) => {
                events.push(WorkbenchEvent::TargetResolved(TargetResolved {
                    intent: row.intent.clone(),
                    note: *note,
                    instrument: instrument.clone(),
                    quality: row.quality,
                }))
            }
            _ => events.push(WorkbenchEvent::Warning(WorkbenchWarning {
                message: format!(
                    "unsupported intent {} from input {}",
                    row.intent, row.input_id
                ),
            })),
        }
    }
    events
}

pub fn encode_event(event: &WorkbenchEvent) -> String {
    match event {
        WorkbenchEvent::RawObserved(raw) => encode_raw_observed(raw),
        WorkbenchEvent::CanonicalMatched(matched) => encode_canonical_matched(matched),
        WorkbenchEvent::TargetResolved(resolved) => encode_target_resolved(resolved),
        WorkbenchEvent::Warning(warning) => format!(
            "{{\"type\":\"warning\",\"message\":\"{}\"}}",
            escape_json(&warning.message)
        ),
    }
}

fn encode_raw_observed(raw: &RawObserved) -> String {
    let source = raw
        .source
        .as_ref()
        .map(|source| format!("\"{}\"", escape_json(source)))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"type\":\"raw_observed\",\"time_millis\":{},\"source\":{},\"event\":{}}}",
        raw.time_millis,
        source,
        encode_raw_midi_event(&raw.event)
    )
}

fn encode_raw_midi_event(event: &RawMidiEvent) -> String {
    match event {
        RawMidiEvent::NoteOn {
            channel,
            note,
            velocity,
        } => format!(
            "{{\"kind\":\"note_on\",\"channel\":{channel},\"note\":{note},\"velocity\":{velocity}}}"
        ),
        RawMidiEvent::NoteOff {
            channel,
            note,
            velocity,
        } => format!(
            "{{\"kind\":\"note_off\",\"channel\":{channel},\"note\":{note},\"velocity\":{velocity}}}"
        ),
        RawMidiEvent::ControlChange {
            channel,
            controller,
            value,
        } => format!(
            "{{\"kind\":\"control_change\",\"channel\":{channel},\"controller\":{controller},\"value\":{value}}}"
        ),
        RawMidiEvent::PolyAftertouch {
            channel,
            note,
            pressure,
        } => format!(
            "{{\"kind\":\"poly_aftertouch\",\"channel\":{channel},\"note\":{note},\"pressure\":{pressure}}}"
        ),
        RawMidiEvent::Other => "{\"kind\":\"other\"}".to_string(),
    }
}

fn encode_canonical_matched(matched: &CanonicalMatched) -> String {
    let evidence = matched
        .evidence
        .iter()
        .map(|item| format!("\"{}\"", escape_json(item)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"type\":\"canonical_matched\",\"intent\":\"{}\",\"evidence\":[{}]}}",
        matched.intent, evidence
    )
}

fn encode_target_resolved(resolved: &TargetResolved) -> String {
    format!(
        "{{\"type\":\"target_resolved\",\"intent\":\"{}\",\"note\":{},\"instrument\":\"{}\",\"quality\":\"{}\"}}",
        resolved.intent,
        resolved.note,
        escape_json(&resolved.instrument),
        quality_str(resolved.quality)
    )
}

fn quality_str(quality: MappingQuality) -> &'static str {
    quality.as_str()
}

fn escape_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::Intent;

    #[test]
    fn encodes_raw_note_event() {
        let encoded = encode_event(&WorkbenchEvent::RawObserved(RawObserved {
            time_millis: 42,
            source: Some("TD-50".to_string()),
            event: RawMidiEvent::NoteOn {
                channel: 10,
                note: 52,
                velocity: 104,
            },
        }));
        assert!(encoded.contains("\"type\":\"raw_observed\""));
        assert!(encoded.contains("\"kind\":\"note_on\""));
        assert!(encoded.contains("\"note\":52"));
    }

    #[test]
    fn encodes_target_quality_and_escapes_instrument() {
        let encoded = encode_event(&WorkbenchEvent::TargetResolved(TargetResolved {
            intent: Intent::CrashEdge(2),
            note: 52,
            instrument: "China \"R\"".to_string(),
            quality: MappingQuality::Fallback,
        }));
        assert!(encoded.contains("\"intent\":\"crash.2.edge\""));
        assert!(encoded.contains("\"quality\":\"fallback\""));
        assert!(encoded.contains("China \\\"R\\\""));
    }
}
