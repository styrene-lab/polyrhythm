use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use crate::profiles::Intent;
use crate::workbench::coverage::CoverageReport;
use crate::workbench::event::{
    CanonicalMatched, ExpectedOutput, MappingQuality, RawMidiEvent, RawObserved, TargetResolved,
    WorkbenchEvent, WorkbenchWarning,
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
        WorkbenchEvent::ExpectOutput(expected) => encode_expected_output(expected),
        WorkbenchEvent::Warning(warning) => format!(
            "{{\"type\":\"warning\",\"message\":\"{}\"}}",
            escape_json(&warning.message)
        ),
    }
}

pub fn decode_events(text: &str) -> Result<Vec<WorkbenchEvent>, String> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim();
            (!line.is_empty())
                .then(|| decode_event(line).map_err(|err| format!("line {}: {err}", index + 1)))
        })
        .collect()
}

pub fn decode_event(line: &str) -> Result<WorkbenchEvent, String> {
    match string_field(line, "type").as_deref() {
        Some("raw_observed") => decode_raw_observed(line),
        Some("target_resolved") => decode_target_resolved(line),
        Some("canonical_matched") => decode_canonical_matched(line),
        Some("expect_output") => decode_expected_output(line),
        Some("warning") => Ok(WorkbenchEvent::Warning(WorkbenchWarning {
            message: string_field(line, "message").unwrap_or_default(),
        })),
        Some(kind) => Err(format!("unsupported workbench event type '{kind}'")),
        None => Err("missing workbench event type".to_string()),
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

fn encode_expected_output(expected: &ExpectedOutput) -> String {
    format!(
        "{{\"type\":\"expect_output\",\"event\":{}}}",
        encode_raw_midi_event(&expected.event)
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

fn decode_raw_observed(line: &str) -> Result<WorkbenchEvent, String> {
    Ok(WorkbenchEvent::RawObserved(RawObserved {
        time_millis: number_field_u128(line, "time_millis")?,
        source: nullable_string_field(line, "source"),
        event: decode_raw_midi_event(line)?,
    }))
}

fn decode_raw_midi_event(line: &str) -> Result<RawMidiEvent, String> {
    let event_object = object_field(line, "event")?;
    let kind =
        string_field(&event_object, "kind").ok_or_else(|| "raw event missing kind".to_string())?;
    let event = match kind.as_str() {
        "note_on" => RawMidiEvent::NoteOn {
            channel: number_field(&event_object, "channel")?,
            note: number_field(&event_object, "note")?,
            velocity: number_field(&event_object, "velocity")?,
        },
        "note_off" => RawMidiEvent::NoteOff {
            channel: number_field(&event_object, "channel")?,
            note: number_field(&event_object, "note")?,
            velocity: number_field(&event_object, "velocity")?,
        },
        "control_change" => RawMidiEvent::ControlChange {
            channel: number_field(&event_object, "channel")?,
            controller: number_field(&event_object, "controller")?,
            value: number_field(&event_object, "value")?,
        },
        "poly_aftertouch" => RawMidiEvent::PolyAftertouch {
            channel: number_field(&event_object, "channel")?,
            note: number_field(&event_object, "note")?,
            pressure: number_field(&event_object, "pressure")?,
        },
        "other" => RawMidiEvent::Other,
        other => return Err(format!("unsupported raw MIDI event kind '{other}'")),
    };
    Ok(event)
}

fn decode_target_resolved(line: &str) -> Result<WorkbenchEvent, String> {
    Ok(WorkbenchEvent::TargetResolved(TargetResolved {
        intent: parse_intent_for_trace(
            &string_field(line, "intent").ok_or_else(|| "target missing intent".to_string())?,
        )?,
        note: number_field(line, "note")?,
        instrument: string_field(line, "instrument")
            .ok_or_else(|| "target missing instrument".to_string())?,
        quality: parse_quality(
            &string_field(line, "quality").ok_or_else(|| "target missing quality".to_string())?,
        )?,
    }))
}

fn decode_canonical_matched(line: &str) -> Result<WorkbenchEvent, String> {
    Ok(WorkbenchEvent::CanonicalMatched(CanonicalMatched {
        intent: parse_intent_for_trace(
            &string_field(line, "intent")
                .ok_or_else(|| "canonical match missing intent".to_string())?,
        )?,
        evidence: Vec::new(),
    }))
}

fn decode_expected_output(line: &str) -> Result<WorkbenchEvent, String> {
    Ok(WorkbenchEvent::ExpectOutput(ExpectedOutput {
        event: decode_raw_midi_event(line)?,
    }))
}

fn parse_quality(raw: &str) -> Result<MappingQuality, String> {
    match raw {
        "exact" => Ok(MappingQuality::Exact),
        "fallback" => Ok(MappingQuality::Fallback),
        "unsupported" => Ok(MappingQuality::Unsupported),
        "requires_runtime_predicate" => Ok(MappingQuality::RequiresRuntimePredicate),
        _ => Err(format!("unknown mapping quality '{raw}'")),
    }
}

fn parse_intent_for_trace(raw: &str) -> Result<Intent, String> {
    match raw {
        "kick.main" => Ok(Intent::KickMain),
        "snare.head" => Ok(Intent::SnareHead),
        "snare.rim" => Ok(Intent::SnareRim),
        "snare.rimshot" => Ok(Intent::SnareRimshot),
        "hihat.closed" => Ok(Intent::HihatClosed),
        "hihat.semi_open" => Ok(Intent::HihatSemiOpen),
        "hihat.open" => Ok(Intent::HihatOpen),
        "hihat.pedal" => Ok(Intent::HihatPedal),
        "ride.bow" => Ok(Intent::RideBow),
        "ride.bell" => Ok(Intent::RideBell),
        "ride.edge" => Ok(Intent::RideEdge),
        _ => {
            if let Some(index) = raw
                .strip_prefix("tom.")
                .and_then(|rest| rest.strip_suffix(".head"))
            {
                return index
                    .parse()
                    .map(Intent::TomHead)
                    .map_err(|_| format!("invalid tom intent '{raw}'"));
            }
            if let Some(index) = raw
                .strip_prefix("tom.")
                .and_then(|rest| rest.strip_suffix(".rim"))
            {
                return index
                    .parse()
                    .map(Intent::TomRim)
                    .map_err(|_| format!("invalid tom intent '{raw}'"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".bow"))
            {
                return index
                    .parse()
                    .map(Intent::CrashBow)
                    .map_err(|_| format!("invalid crash intent '{raw}'"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".edge"))
            {
                return index
                    .parse()
                    .map(Intent::CrashEdge)
                    .map_err(|_| format!("invalid crash intent '{raw}'"));
            }
            if let Some(index) = raw
                .strip_prefix("crash.")
                .and_then(|rest| rest.strip_suffix(".choke"))
            {
                return index
                    .parse()
                    .map(Intent::CrashChoke)
                    .map_err(|_| format!("invalid crash intent '{raw}'"));
            }
            Err(format!("unknown intent '{raw}'"))
        }
    }
}

fn nullable_string_field(line: &str, key: &str) -> Option<String> {
    string_field(line, key)
}

fn string_field(line: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":");
    let start = line.find(&marker)? + marker.len();
    let rest = line[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut escaped = false;
    for ch in rest.chars() {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn number_field(line: &str, key: &str) -> Result<u8, String> {
    number_field_u128(line, key).and_then(|value| {
        u8::try_from(value).map_err(|_| format!("numeric field '{key}' out of MIDI byte range"))
    })
}

fn number_field_u128(line: &str, key: &str) -> Result<u128, String> {
    let marker = format!("\"{key}\":");
    let start = line
        .find(&marker)
        .ok_or_else(|| format!("missing numeric field '{key}'"))?
        + marker.len();
    let rest = line[start..].trim_start();
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(format!("numeric field '{key}' has no digits"));
    }
    digits
        .parse()
        .map_err(|_| format!("invalid numeric field '{key}'"))
}

fn object_field(line: &str, key: &str) -> Result<String, String> {
    let marker = format!("\"{key}\":");
    let start = line
        .find(&marker)
        .ok_or_else(|| format!("missing object field '{key}'"))?
        + marker.len();
    let rest = line[start..].trim_start();
    let mut chars = rest.char_indices();
    let Some((_, '{')) = chars.next() else {
        return Err(format!("object field '{key}' is not an object"));
    };
    let mut depth = 1usize;
    for (index, ch) in chars {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(rest[..=index].to_string());
                }
            }
            _ => {}
        }
    }
    Err(format!("object field '{key}' is unterminated"))
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

    #[test]
    fn decodes_raw_note_and_target_events() {
        let jsonl = concat!(
            "{\"type\":\"raw_observed\",\"time_millis\":42,\"source\":\"TD-50\",\"event\":{\"kind\":\"note_on\",\"channel\":10,\"note\":52,\"velocity\":104}}\n",
            "{\"type\":\"target_resolved\",\"intent\":\"crash.2.edge\",\"note\":52,\"instrument\":\"China \\\"R\\\"\",\"quality\":\"fallback\"}\n",
            "{\"type\":\"expect_output\",\"event\":{\"kind\":\"note_on\",\"channel\":10,\"note\":52,\"velocity\":104}}\n"
        );
        let events = decode_events(jsonl).unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], WorkbenchEvent::RawObserved(_)));
        assert_eq!(
            events[1],
            WorkbenchEvent::TargetResolved(TargetResolved {
                intent: Intent::CrashEdge(2),
                note: 52,
                instrument: "China \"R\"".to_string(),
                quality: MappingQuality::Fallback,
            })
        );
        assert!(matches!(events[2], WorkbenchEvent::ExpectOutput(_)));
    }
}
