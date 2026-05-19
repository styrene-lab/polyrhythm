use crate::profiles::Intent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbenchEvent {
    RawObserved(RawObserved),
    CanonicalMatched(CanonicalMatched),
    TargetResolved(TargetResolved),
    Warning(WorkbenchWarning),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawObserved {
    pub time_millis: u128,
    pub source: Option<String>,
    pub event: RawMidiEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawMidiEvent {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    PolyAftertouch {
        channel: u8,
        note: u8,
        pressure: u8,
    },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalMatched {
    pub intent: Intent,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetResolved {
    pub intent: Intent,
    pub note: u8,
    pub instrument: String,
    pub quality: MappingQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingQuality {
    Exact,
    Fallback,
    Unsupported,
    RequiresRuntimePredicate,
}

impl MappingQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Fallback => "fallback",
            Self::Unsupported => "unsupported",
            Self::RequiresRuntimePredicate => "requires_runtime_predicate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchWarning {
    pub message: String,
}
