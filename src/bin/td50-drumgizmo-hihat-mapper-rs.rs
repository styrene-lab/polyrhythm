use std::env;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use polyrhythm::td50_mapper::{MapperState, MidiEvent, VelocityCurve};

const SND_SEQ_OPEN_DUPLEX: c_int = 3;
const SND_SEQ_PORT_CAP_READ: c_uint = 1 << 0;
const SND_SEQ_PORT_CAP_WRITE: c_uint = 1 << 1;
const SND_SEQ_PORT_CAP_SUBS_READ: c_uint = 1 << 5;
const SND_SEQ_PORT_TYPE_MIDI_GENERIC: c_uint = 1 << 1;
const SND_SEQ_PORT_TYPE_APPLICATION: c_uint = 1 << 20;

const SND_SEQ_EVENT_NOTEON: u8 = 6;
const SND_SEQ_EVENT_NOTEOFF: u8 = 7;
const SND_SEQ_EVENT_CONTROLLER: u8 = 10;

static RUNNING: AtomicBool = AtomicBool::new(true);

type SndSeq = c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct SndSeqAddr {
    client: u8,
    port: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SndSeqEvNote {
    channel: u8,
    note: u8,
    velocity: u8,
    off_velocity: u8,
    duration: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SndSeqEvCtrl {
    channel: u8,
    unused: [u8; 3],
    param: u32,
    value: i32,
}

#[repr(C)]
union SndSeqEventData {
    note: SndSeqEvNote,
    control: SndSeqEvCtrl,
    raw: [u8; 12],
}

#[repr(C)]
struct SndSeqEvent {
    type_: u8,
    flags: u8,
    tag: u8,
    queue: u8,
    time: [u32; 2],
    source: SndSeqAddr,
    dest: SndSeqAddr,
    data: SndSeqEventData,
}

#[link(name = "asound")]
extern "C" {
    fn snd_seq_open(
        handle: *mut *mut SndSeq,
        name: *const c_char,
        streams: c_int,
        mode: c_int,
    ) -> c_int;
    fn snd_seq_close(handle: *mut SndSeq) -> c_int;
    fn snd_seq_set_client_name(handle: *mut SndSeq, name: *const c_char) -> c_int;
    fn snd_seq_create_simple_port(
        handle: *mut SndSeq,
        name: *const c_char,
        caps: c_uint,
        type_: c_uint,
    ) -> c_int;
    fn snd_seq_connect_from(
        handle: *mut SndSeq,
        my_port: c_int,
        src_client: c_int,
        src_port: c_int,
    ) -> c_int;
    fn snd_seq_event_input(handle: *mut SndSeq, ev: *mut *mut SndSeqEvent) -> c_int;
    fn snd_seq_event_output_direct(handle: *mut SndSeq, ev: *mut SndSeqEvent) -> c_int;
    fn snd_strerror(errnum: c_int) -> *const c_char;
    fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> extern "C" fn(c_int);
}

extern "C" fn on_signal(_: c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let in_client = args
        .get(1)
        .map(|raw| {
            raw.parse::<c_int>()
                .map_err(|_| format!("bad client: {raw}"))
        })
        .transpose()?
        .unwrap_or(36);
    let in_port = args
        .get(2)
        .map(|raw| raw.parse::<c_int>().map_err(|_| format!("bad port: {raw}")))
        .transpose()?
        .unwrap_or(0);

    unsafe {
        signal(2, on_signal);
        signal(15, on_signal);
    }

    let mut seq: *mut SndSeq = ptr::null_mut();
    check(unsafe { snd_seq_open(&mut seq, cstr("default")?.as_ptr(), SND_SEQ_OPEN_DUPLEX, 0) })?;
    let _guard = SeqGuard(seq);

    let client_name = env::var("POLYRHYTHM_MIDI_CLIENT_NAME")
        .unwrap_or_else(|_| "TD50-DrumGizmo-Hihat-Mapper".to_string());
    let port_name = env::var("POLYRHYTHM_MIDI_PORT_NAME").unwrap_or_else(|_| "out".to_string());

    check(unsafe { snd_seq_set_client_name(seq, cstr(&client_name)?.as_ptr()) })?;
    let port = unsafe {
        snd_seq_create_simple_port(
            seq,
            cstr(&port_name)?.as_ptr(),
            SND_SEQ_PORT_CAP_READ | SND_SEQ_PORT_CAP_SUBS_READ | SND_SEQ_PORT_CAP_WRITE,
            SND_SEQ_PORT_TYPE_MIDI_GENERIC | SND_SEQ_PORT_TYPE_APPLICATION,
        )
    };
    check(port)?;
    check(unsafe { snd_seq_connect_from(seq, port, in_client, in_port) })?;

    let velocity_curve = VelocityCurve::from_env();
    eprintln!(
        "{client_name} from {in_client}:{in_port} to port {port_name}. CC4 bands: open <32, semi 32..71, closed >=72. Velocity curve: {}.",
        velocity_curve.name()
    );

    let mut mapper = MapperState::new(velocity_curve);
    while RUNNING.load(Ordering::SeqCst) {
        let mut ev_ptr: *mut SndSeqEvent = ptr::null_mut();
        let rc = unsafe { snd_seq_event_input(seq, &mut ev_ptr) };
        if rc < 0 || ev_ptr.is_null() {
            continue;
        }
        let Some(input) = decode_event(unsafe { &*ev_ptr }) else {
            continue;
        };
        for output in mapper.map_event(input) {
            emit(seq, port, output);
        }
        for line in mapper.trace.drain(..) {
            eprintln!("{line}");
        }
    }

    Ok(())
}

fn decode_event(ev: &SndSeqEvent) -> Option<MidiEvent> {
    match ev.type_ {
        SND_SEQ_EVENT_CONTROLLER => {
            let ctrl = unsafe { ev.data.control };
            Some(MidiEvent::Controller {
                channel: ctrl.channel,
                param: ctrl.param as u8,
                value: ctrl.value.clamp(0, 127) as u8,
            })
        }
        SND_SEQ_EVENT_NOTEON => {
            let note = unsafe { ev.data.note };
            Some(MidiEvent::NoteOn {
                channel: note.channel,
                note: note.note,
                velocity: note.velocity,
            })
        }
        SND_SEQ_EVENT_NOTEOFF => {
            let note = unsafe { ev.data.note };
            Some(MidiEvent::NoteOff {
                channel: note.channel,
                note: note.note,
                velocity: note.velocity,
            })
        }
        _ => None,
    }
}

fn emit(seq: *mut SndSeq, port: c_int, event: MidiEvent) {
    let mut ev = SndSeqEvent {
        type_: 0,
        flags: 0,
        tag: 0,
        queue: 253,
        time: [0; 2],
        source: SndSeqAddr {
            client: 0,
            port: port as u8,
        },
        dest: SndSeqAddr {
            client: 254,
            port: 253,
        },
        data: SndSeqEventData { raw: [0; 12] },
    };
    match event {
        MidiEvent::NoteOn {
            channel,
            note,
            velocity,
        } => {
            ev.type_ = SND_SEQ_EVENT_NOTEON;
            ev.data = SndSeqEventData {
                note: SndSeqEvNote {
                    channel,
                    note,
                    velocity,
                    off_velocity: 0,
                    duration: 0,
                },
            };
        }
        MidiEvent::NoteOff {
            channel,
            note,
            velocity,
        } => {
            ev.type_ = SND_SEQ_EVENT_NOTEOFF;
            ev.data = SndSeqEventData {
                note: SndSeqEvNote {
                    channel,
                    note,
                    velocity,
                    off_velocity: 0,
                    duration: 0,
                },
            };
        }
        MidiEvent::Controller {
            channel,
            param,
            value,
        } => {
            ev.type_ = SND_SEQ_EVENT_CONTROLLER;
            ev.data = SndSeqEventData {
                control: SndSeqEvCtrl {
                    channel,
                    unused: [0; 3],
                    param: param.into(),
                    value: value.into(),
                },
            };
        }
        MidiEvent::Other => return,
    }
    unsafe {
        snd_seq_event_output_direct(seq, &mut ev);
    }
}

struct SeqGuard(*mut SndSeq);

impl Drop for SeqGuard {
    fn drop(&mut self) {
        unsafe {
            snd_seq_close(self.0);
        }
    }
}

fn check(rc: c_int) -> Result<c_int, String> {
    if rc < 0 {
        Err(alsa_error(rc))
    } else {
        Ok(rc)
    }
}

fn alsa_error(rc: c_int) -> String {
    unsafe {
        let ptr = snd_strerror(rc);
        if ptr.is_null() {
            format!("ALSA error {rc}")
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

fn cstr(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| format!("string contains nul byte: {value:?}"))
}
