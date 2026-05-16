use std::collections::BTreeSet;

pub const DRS_EMITTED_NOTES: &[u8] = &[
    26, 27, 35, 36, 37, 38, 41, 42, 43, 44, 45, 46, 48, 49, 51, 55, 57, 80, 81, 82, 90, 91, 92,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    Controller { channel: u8, param: u8, value: u8 },
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HihatOpenness {
    Open,
    SemiOpen,
    Closed,
}

impl HihatOpenness {
    pub fn from_cc4(value: u8) -> Self {
        match value {
            0..=31 => Self::Open,
            32..=71 => Self::SemiOpen,
            _ => Self::Closed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapperState {
    cc4: u8,
    cc18: u8,
    suppress49: bool,
    suppress55: bool,
    pub trace: Vec<String>,
}

impl Default for MapperState {
    fn default() -> Self {
        Self {
            cc4: 0,
            cc18: 0,
            suppress49: false,
            suppress55: false,
            trace: Vec::new(),
        }
    }
}

impl MapperState {
    pub fn cc4(&self) -> u8 {
        self.cc4
    }

    pub fn cc18(&self) -> u8 {
        self.cc18
    }

    pub fn pending_suppression(&self) -> (bool, bool) {
        (self.suppress49, self.suppress55)
    }

    pub fn map_event(&mut self, event: MidiEvent) -> Vec<MidiEvent> {
        match event {
            MidiEvent::Controller { param: 4, value, .. } => {
                self.cc4 = value;
                Vec::new()
            }
            MidiEvent::Controller { param: 18, value, .. } => {
                self.cc18 = value;
                Vec::new()
            }
            MidiEvent::Controller { .. } | MidiEvent::Other => Vec::new(),
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => self.map_note_on(channel, note, velocity),
            MidiEvent::NoteOff {
                channel,
                note,
                velocity,
            } => self.map_note_off(channel, note, velocity),
        }
    }

    fn map_note_on(&mut self, channel: u8, note: u8, velocity: u8) -> Vec<MidiEvent> {
        let mapped = map_note(note, self.cc4, self.cc18, velocity);
        let out_velocity = map_velocity(velocity);

        if note == 27 {
            self.suppress49 = true;
            self.suppress55 = true;
            self.trace.push(format!(
                "noteon crash1 in={note} cc18={} out={mapped} vel={velocity} suppress49/55",
                self.cc18
            ));
            return vec![MidiEvent::NoteOn {
                channel,
                note: mapped,
                velocity,
            }];
        }

        if note == 49 && self.suppress49 {
            self.suppress49 = false;
            self.trace
                .push(format!("suppress companion noteon in=49 vel={velocity}"));
            return Vec::new();
        }

        if note == 55 && self.suppress55 {
            self.suppress55 = false;
            self.trace
                .push(format!("suppress companion noteon in=55 vel={velocity}"));
            return Vec::new();
        }

        self.trace.push(format!(
            "noteon in={note} cc4={} cc18={} out={mapped} vel={velocity} out_vel={out_velocity}",
            self.cc4, self.cc18
        ));
        vec![MidiEvent::NoteOn {
            channel,
            note: mapped,
            velocity: out_velocity,
        }]
    }

    fn map_note_off(&mut self, channel: u8, note: u8, velocity: u8) -> Vec<MidiEvent> {
        if matches!(note, 26 | 27 | 44 | 46) {
            return hihat_off_notes()
                .iter()
                .copied()
                .map(|note| MidiEvent::NoteOff {
                    channel,
                    note,
                    velocity,
                })
                .collect();
        }

        if note == 49 && self.suppress49 {
            self.suppress49 = false;
            return Vec::new();
        }

        if note == 55 && self.suppress55 {
            self.suppress55 = false;
            return Vec::new();
        }

        vec![MidiEvent::NoteOff {
            channel,
            note,
            velocity,
        }]
    }
}

pub fn map_velocity(velocity: u8) -> u8 {
    let v = (u16::from(velocity) * u16::from(velocity) + 126) / 127;
    v.clamp(1, 127) as u8
}

pub fn map_note(note: u8, cc4: u8, cc18: u8, velocity: u8) -> u8 {
    if note == 44 {
        return 44;
    }

    if note == 46 {
        return match HihatOpenness::from_cc4(cc4) {
            HihatOpenness::Closed => 42,
            HihatOpenness::SemiOpen => 41,
            HihatOpenness::Open => 46,
        };
    }

    if note == 26 {
        return match HihatOpenness::from_cc4(cc4) {
            HihatOpenness::Closed => 90,
            HihatOpenness::SemiOpen => 91,
            HihatOpenness::Open => 92,
        };
    }

    if note == 27 {
        if cc18 >= 96 {
            if velocity < 40 {
                return 82;
            }
            return 81;
        }
        if cc18 >= 48 {
            return 80;
        }
        return 80;
    }

    note
}

pub fn hihat_off_notes() -> &'static [u8] {
    &[41, 42, 44, 46, 80, 81, 82, 90, 91, 92]
}

pub fn mapped_notes_from_midimap(xml: &str) -> BTreeSet<u8> {
    let mut notes = BTreeSet::new();
    let mut rest = xml;

    while let Some(idx) = rest.find("note=\"") {
        rest = &rest[idx + 6..];
        let Some(end) = rest.find('"') else { break };
        if let Ok(note) = rest[..end].parse::<u8>() {
            notes.insert(note);
        }
        rest = &rest[end + 1..];
    }

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_on(note: u8, velocity: u8) -> MidiEvent {
        MidiEvent::NoteOn {
            channel: 0,
            note,
            velocity,
        }
    }

    fn note_off(note: u8) -> MidiEvent {
        MidiEvent::NoteOff {
            channel: 0,
            note,
            velocity: 64,
        }
    }

    #[test]
    fn cc4_boundaries_match_c_mapper() {
        assert_eq!(HihatOpenness::from_cc4(31), HihatOpenness::Open);
        assert_eq!(HihatOpenness::from_cc4(32), HihatOpenness::SemiOpen);
        assert_eq!(HihatOpenness::from_cc4(71), HihatOpenness::SemiOpen);
        assert_eq!(HihatOpenness::from_cc4(72), HihatOpenness::Closed);
    }

    #[test]
    fn hihat_bow_maps_by_cc4() {
        assert_eq!(map_note(46, 0, 0, 80), 46);
        assert_eq!(map_note(46, 32, 0, 80), 41);
        assert_eq!(map_note(46, 72, 0, 80), 42);
    }

    #[test]
    fn hihat_edge_maps_by_cc4() {
        assert_eq!(map_note(26, 0, 0, 80), 92);
        assert_eq!(map_note(26, 32, 0, 80), 91);
        assert_eq!(map_note(26, 72, 0, 80), 90);
    }

    #[test]
    fn foot_chick_stays_44() {
        assert_eq!(map_note(44, 0, 0, 80), 44);
        assert_eq!(map_note(44, 127, 127, 80), 44);
    }

    #[test]
    fn crash1_maps_by_cc18_and_velocity() {
        assert_eq!(map_note(27, 0, 0, 100), 80);
        assert_eq!(map_note(27, 0, 48, 100), 80);
        assert_eq!(map_note(27, 0, 96, 39), 82);
        assert_eq!(map_note(27, 0, 96, 40), 81);
    }

    #[test]
    fn crash1_suppresses_companion_noteons_once_each() {
        let mut state = MapperState::default();
        assert_eq!(state.map_event(MidiEvent::Controller { channel: 0, param: 18, value: 96 }), vec![]);
        assert_eq!(state.map_event(note_on(27, 80)), vec![note_on(81, 80)]);
        assert_eq!(state.pending_suppression(), (true, true));
        assert_eq!(state.map_event(note_on(49, 70)), vec![]);
        assert_eq!(state.pending_suppression(), (false, true));
        assert_eq!(state.map_event(note_on(55, 70)), vec![]);
        assert_eq!(state.pending_suppression(), (false, false));
        assert_eq!(state.map_event(note_on(49, 70)), vec![note_on(49, map_velocity(70))]);
    }

    #[test]
    fn hihat_noteoff_emits_all_articulation_offs() {
        let mut state = MapperState::default();
        let out = state.map_event(note_off(46));
        let notes: Vec<u8> = out
            .into_iter()
            .map(|event| match event {
                MidiEvent::NoteOff { note, .. } => note,
                _ => panic!("unexpected event"),
            })
            .collect();
        assert_eq!(notes, hihat_off_notes());
    }

    #[test]
    fn velocity_curve_matches_current_c_policy() {
        assert_eq!(map_velocity(1), 1);
        assert_eq!(map_velocity(20), 4);
        assert_eq!(map_velocity(40), 13);
        assert_eq!(map_velocity(64), 33);
        assert_eq!(map_velocity(80), 51);
        assert_eq!(map_velocity(100), 79);
        assert_eq!(map_velocity(127), 127);
    }

    #[test]
    fn emitted_drs_notes_are_present_in_drs_midimap() {
        let midimap = include_str!("../assets/drumgizmo/DRSKit/Midimap_td50.xml");
        let mapped = mapped_notes_from_midimap(midimap);
        let missing: Vec<u8> = DRS_EMITTED_NOTES
            .iter()
            .copied()
            .filter(|note| !mapped.contains(note))
            .collect();
        assert_eq!(missing, Vec::<u8>::new());
    }
}
