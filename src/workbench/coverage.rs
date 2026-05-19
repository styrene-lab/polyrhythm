use crate::profiles::{DeviceInput, DeviceProfile, Intent, KitProfile};
use crate::workbench::event::MappingQuality;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    pub device_id: String,
    pub kit_id: String,
    pub rows: Vec<CoverageRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRow {
    pub input_id: String,
    pub intent: Intent,
    pub device_notes: Vec<u8>,
    pub target_note: Option<u8>,
    pub instrument: Option<String>,
    pub quality: MappingQuality,
}

pub fn report(device: &DeviceProfile, kit: &KitProfile) -> CoverageReport {
    CoverageReport {
        device_id: device.id.clone(),
        kit_id: kit.id.clone(),
        rows: device.inputs.iter().map(|input| row(input, kit)).collect(),
    }
}

fn row(input: &DeviceInput, kit: &KitProfile) -> CoverageRow {
    match kit.mappings.get(&input.intent) {
        Some(target) => CoverageRow {
            input_id: input.id.clone(),
            intent: input.intent.clone(),
            device_notes: input.notes.clone(),
            target_note: Some(target.note),
            instrument: Some(target.instrument.clone()),
            quality: if target.fallback {
                MappingQuality::Fallback
            } else {
                MappingQuality::Exact
            },
        },
        None => CoverageRow {
            input_id: input.id.clone(),
            intent: input.intent.clone(),
            device_notes: input.notes.clone(),
            target_note: None,
            instrument: None,
            quality: MappingQuality::Unsupported,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{DeviceInput, KitTarget};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn coverage_marks_exact_fallback_and_unsupported() {
        let device = DeviceProfile {
            id: "test-device".to_string(),
            name: "Test Device".to_string(),
            inputs: vec![
                DeviceInput {
                    id: "snare".to_string(),
                    intent: Intent::SnareHead,
                    notes: vec![38],
                },
                DeviceInput {
                    id: "rim".to_string(),
                    intent: Intent::SnareRim,
                    notes: vec![37],
                },
                DeviceInput {
                    id: "choke".to_string(),
                    intent: Intent::CrashChoke(1),
                    notes: vec![56],
                },
            ],
        };
        let mut mappings = BTreeMap::new();
        mappings.insert(
            Intent::SnareHead,
            KitTarget {
                note: 38,
                instrument: "Snare".to_string(),
                fallback: false,
            },
        );
        mappings.insert(
            Intent::SnareRim,
            KitTarget {
                note: 38,
                instrument: "Snare".to_string(),
                fallback: true,
            },
        );
        let kit = KitProfile {
            id: "test-kit".to_string(),
            name: "Test Kit".to_string(),
            kit_xml: PathBuf::from("kit.xml"),
            sample_params: String::new(),
            mappings,
        };

        let report = report(&device, &kit);
        assert_eq!(report.rows[0].quality, MappingQuality::Exact);
        assert_eq!(report.rows[1].quality, MappingQuality::Fallback);
        assert_eq!(report.rows[2].quality, MappingQuality::Unsupported);
    }
}
