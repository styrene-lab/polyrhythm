use crate::profiles::{DeviceProfile, KitProfile};
use crate::td50_mapper::{MapperState, MidiEvent, VelocityCurve};
use crate::workbench::event::{RawMidiEvent, RawObserved, WorkbenchEvent};
use crate::workbench::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    pub raw_events: usize,
    pub output_events: usize,
    pub warnings: Vec<String>,
    pub outputs: Vec<MidiEvent>,
}

pub fn replay_events(
    events: &[WorkbenchEvent],
    _device: &DeviceProfile,
    _kit: &KitProfile,
) -> ReplayReport {
    let mut mapper = MapperState::new(VelocityCurve::Linear);
    let mut raw_events = 0;
    let mut outputs = Vec::new();
    let mut warnings = Vec::new();

    for event in events {
        let WorkbenchEvent::RawObserved(raw) = event else {
            continue;
        };
        raw_events += 1;
        match midi_event_from_raw(raw) {
            Some(event) => outputs.extend(mapper.map_event(event)),
            None => warnings.push(format!("unsupported raw MIDI event at {}", raw.time_millis)),
        }
    }

    ReplayReport {
        raw_events,
        output_events: outputs.len(),
        warnings,
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
    match raw.event {
        RawMidiEvent::NoteOn {
            channel,
            note,
            velocity,
        } => Some(MidiEvent::NoteOn {
            channel,
            note,
            velocity,
        }),
        RawMidiEvent::NoteOff {
            channel,
            note,
            velocity,
        } => Some(MidiEvent::NoteOff {
            channel,
            note,
            velocity,
        }),
        RawMidiEvent::ControlChange {
            channel,
            controller,
            value,
        } => Some(MidiEvent::Controller {
            channel,
            param: controller,
            value,
        }),
        RawMidiEvent::Other | RawMidiEvent::PolyAftertouch { .. } => None,
    }
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
            "{\"type\":\"raw_observed\",\"time_millis\":2,\"source\":\"fixture\",\"event\":{\"kind\":\"note_on\",\"channel\":0,\"note\":46,\"velocity\":90}}\n"
        );
        let report = replay_jsonl(jsonl, &device(), &kit()).unwrap();
        assert_eq!(report.raw_events, 2);
        assert_eq!(report.outputs.len(), 1);
        assert_eq!(
            report.outputs[0],
            MidiEvent::NoteOn {
                channel: 0,
                note: 41,
                velocity: 90,
            }
        );
    }
}
