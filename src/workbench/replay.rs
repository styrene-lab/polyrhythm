use crate::profiles::{DeviceProfile, KitProfile};
use crate::td50_mapper::{MapperState, MidiEvent, VelocityCurve};
use crate::workbench::event::{ExpectedOutput, RawMidiEvent, RawObserved, WorkbenchEvent};
use crate::workbench::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    pub raw_events: usize,
    pub expected_outputs: usize,
    pub output_events: usize,
    pub warnings: Vec<String>,
    pub mismatches: Vec<ReplayMismatch>,
    pub outputs: Vec<MidiEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMismatch {
    pub index: usize,
    pub expected: Option<MidiEvent>,
    pub actual: Option<MidiEvent>,
}

pub fn replay_events(
    events: &[WorkbenchEvent],
    _device: &DeviceProfile,
    _kit: &KitProfile,
) -> ReplayReport {
    let mut mapper = MapperState::new(VelocityCurve::Linear);
    let mut raw_events = 0;
    let mut outputs = Vec::new();
    let mut expected = Vec::new();
    let mut warnings = Vec::new();

    for event in events {
        match event {
            WorkbenchEvent::RawObserved(raw) => {
                raw_events += 1;
                match midi_event_from_raw(raw) {
                    Some(event) => outputs.extend(mapper.map_event(event)),
                    None => {
                        warnings.push(format!("unsupported raw MIDI event at {}", raw.time_millis))
                    }
                }
            }
            WorkbenchEvent::ExpectOutput(expected_output) => {
                match midi_event_from_expected(expected_output) {
                    Some(event) => expected.push(event),
                    None => warnings.push(format!(
                        "unsupported expected MIDI event at expectation {}",
                        expected.len()
                    )),
                }
            }
            _ => {}
        }
    }

    let mismatches = compare_outputs(&expected, &outputs);

    ReplayReport {
        raw_events,
        expected_outputs: expected.len(),
        output_events: outputs.len(),
        warnings,
        mismatches,
        outputs,
    }
}

pub fn replay_jsonl(
    text: &str,
    device: &DeviceProfile,
    kit: &KitProfile,
) -> Result<ReplayReport, String> {
    let events = trace::decode_events(text)?;
    Ok(replay_events(&events, device, kit))
}

fn midi_event_from_raw(raw: &RawObserved) -> Option<MidiEvent> {
    midi_event_from_raw_midi(&raw.event)
}

fn midi_event_from_expected(expected: &ExpectedOutput) -> Option<MidiEvent> {
    midi_event_from_raw_midi(&expected.event)
}

fn midi_event_from_raw_midi(event: &RawMidiEvent) -> Option<MidiEvent> {
    match event {
        RawMidiEvent::NoteOn {
            channel,
            note,
            velocity,
        } => Some(MidiEvent::NoteOn {
            channel: *channel,
            note: *note,
            velocity: *velocity,
        }),
        RawMidiEvent::NoteOff {
            channel,
            note,
            velocity,
        } => Some(MidiEvent::NoteOff {
            channel: *channel,
            note: *note,
            velocity: *velocity,
        }),
        RawMidiEvent::ControlChange {
            channel,
            controller,
            value,
        } => Some(MidiEvent::Controller {
            channel: *channel,
            param: *controller,
            value: *value,
        }),
        RawMidiEvent::Other | RawMidiEvent::PolyAftertouch { .. } => None,
    }
}

fn compare_outputs(expected: &[MidiEvent], actual: &[MidiEvent]) -> Vec<ReplayMismatch> {
    let len = expected.len().max(actual.len());
    (0..len)
        .filter_map(|index| {
            let expected = expected.get(index).copied();
            let actual = actual.get(index).copied();
            (expected != actual).then_some(ReplayMismatch {
                index,
                expected,
                actual,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{DeviceInput, Intent, KitTarget};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn device() -> DeviceProfile {
        DeviceProfile {
            id: "td50".to_string(),
            name: "TD-50".to_string(),
            inputs: vec![DeviceInput {
                id: "hihat_open".to_string(),
                intent: Intent::HihatOpen,
                notes: vec![46],
            }],
        }
    }

    fn kit() -> KitProfile {
        let mut mappings = BTreeMap::new();
        mappings.insert(
            Intent::HihatSemiOpen,
            KitTarget {
                note: 80,
                instrument: "HihatSemiOpen".to_string(),
                fallback: false,
            },
        );
        KitProfile {
            id: "crocell".to_string(),
            name: "Crocell".to_string(),
            kit_xml: PathBuf::from("kit.xml"),
            sample_params: String::new(),
            mappings,
        }
    }

    #[test]
    fn replay_maps_raw_hihat_through_existing_mapper_core() {
        let jsonl = concat!(
            "{\"type\":\"raw_observed\",\"time_millis\":1,\"source\":\"fixture\",\"event\":{\"kind\":\"control_change\",\"channel\":0,\"controller\":4,\"value\":32}}\n",
            "{\"type\":\"raw_observed\",\"time_millis\":2,\"source\":\"fixture\",\"event\":{\"kind\":\"note_on\",\"channel\":0,\"note\":46,\"velocity\":90}}\n",
            "{\"type\":\"expect_output\",\"event\":{\"kind\":\"note_on\",\"channel\":0,\"note\":41,\"velocity\":90}}\n"
        );
        let report = replay_jsonl(jsonl, &device(), &kit()).unwrap();
        assert_eq!(report.raw_events, 2);
        assert_eq!(report.expected_outputs, 1);
        assert_eq!(report.outputs.len(), 1);
        assert!(report.mismatches.is_empty());
        assert_eq!(
            report.outputs[0],
            MidiEvent::NoteOn {
                channel: 0,
                note: 41,
                velocity: 90,
            }
        );
    }

    #[test]
    fn replay_reports_expected_output_mismatches() {
        let jsonl = concat!(
            "{\"type\":\"raw_observed\",\"time_millis\":1,\"source\":\"fixture\",\"event\":{\"kind\":\"control_change\",\"channel\":0,\"controller\":4,\"value\":32}}\n",
            "{\"type\":\"raw_observed\",\"time_millis\":2,\"source\":\"fixture\",\"event\":{\"kind\":\"note_on\",\"channel\":0,\"note\":46,\"velocity\":90}}\n",
            "{\"type\":\"expect_output\",\"event\":{\"kind\":\"note_on\",\"channel\":0,\"note\":42,\"velocity\":90}}\n"
        );
        let report = replay_jsonl(jsonl, &device(), &kit()).unwrap();
        assert_eq!(report.expected_outputs, 1);
        assert_eq!(report.mismatches.len(), 1);
        assert_eq!(
            report.mismatches[0],
            ReplayMismatch {
                index: 0,
                expected: Some(MidiEvent::NoteOn {
                    channel: 0,
                    note: 42,
                    velocity: 90,
                }),
                actual: Some(MidiEvent::NoteOn {
                    channel: 0,
                    note: 41,
                    velocity: 90,
                }),
            }
        );
    }
}
