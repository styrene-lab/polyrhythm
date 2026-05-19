# Polyrhythm in the Linux MIDI/audio landscape

Polyrhythm should not be treated as only a TD-50-to-DrumGizmo helper. The TD-50 work demonstrates a broader architecture:

```text
MIDI-compliant device
  -> device-specific Pikl profile
  -> canonical Polyrhythm event stream
  -> target adapter
  -> Linux audio / MIDI / DAW / plugin ecosystem
```

The immediate use case is still reliable local TD-50 playability on this NixOS/COSMIC machine. The strategic value is that Pikl and Polyrhythm can become a semantic MIDI interop layer across devices and backends.

## The core distinction

Raw MIDI is transport data, not musical intent.

A TD-50 does not emit portable drum semantics. It emits device-specific MIDI behavior:

```text
note 46 + CC4 state for hi-hat openness
note 26 for hi-hat edge
note 38 for both snare head and soft cross-stick cases
compound crash note patterns such as [27, 49], [27, 55], [28, 49]
polyphonic aftertouch for cymbal chokes
CC17/CC18/CC88 context for specific zones
```

Most Linux tools and DAWs expect simpler event streams:

```text
one note per articulation
stable note assignments
simple note-on/note-off pairs
separate events for open/closed hi-hat, choke, rimshot, etc.
```

Polyrhythm's job is to bridge that gap:

```text
raw device MIDI
  -> canonical event intent
  -> backend-specific output
```

For drums, canonical events look like:

```text
kick.main
snare.head
snare.rimshot
snare.crossstick
hihat.bow.closed
hihat.edge.open
ride.bell
ride.choke
crash.1.edge
crash.2.choke
```

For other MIDI devices, the same pattern can expand into other domains:

```text
keyboard note/expression events
control surface faders/buttons/transport
MPE note-expression events
foot controller gestures
OSC/control/lighting events
```

## Why DrumGizmo XML is not enough

DrumGizmo midimap XML is a useful compatibility artifact, but it is not expressive enough to be the canonical mapping language. It can represent simple note-to-instrument mappings:

```xml
<map note="38" instr="Snare"/>
<map note="40" instr="SnareRimShot"/>
```

It cannot express the predicates needed for the TD-50:

```text
note + CC4 bucket -> hi-hat openness
note + velocity range -> cross-stick vs snare head
compound notes -> one crash intent, with companion-note suppression
polyphonic aftertouch -> cymbal choke
fallback/exact/unsupported capability reporting
```

Therefore:

```text
Pikl is canonical.
Polyrhythm evaluates predicates at runtime.
DrumGizmo XML maps Polyrhythm's normalized output notes to DrumGizmo instruments.
```

This means generated XML should eventually be understood as:

```text
Polyrhythm output note -> DrumGizmo instrument
```

not necessarily:

```text
raw TD-50 note -> DrumGizmo instrument
```

## Rust ecosystem survey

### MIDI I/O: `alsa`

The Rust `alsa` crate exposes Linux ALSA facilities, including raw MIDI and ALSA sequencer support. This is the best immediate fit for Polyrhythm on this machine because the current runtime graph is Linux/PipeWire/ALSA-centered.

Use cases:

```text
Linux-native MIDI ports
ALSA sequencer client/port enumeration
virtual MIDI output ports
integration with PipeWire/JACK bridges
low-level diagnostics
```

Strategic role:

```text
primary standalone Linux MIDI backend
```

### Cross-platform MIDI: `midir`

`midir` is a cross-platform realtime MIDI crate inspired by RtMidi. It supports realtime MIDI processing, virtual ports on supported platforms, and SysEx.

Use cases:

```text
cross-platform profile capture tools
portable MIDI utilities
future macOS/Windows support
```

Strategic role:

```text
secondary portability layer, not the first Linux runtime target
```

### MIDI parsing/export: `midly`

`midly` is a fast MIDI decoder/encoder for Standard MIDI Files and live MIDI events.

Use cases:

```text
export normalized MIDI files
import MIDI fixtures for tests
offline rendering
canonical performance archive conversion
regression tests from captured performances
```

Strategic role:

```text
offline I/O and test fixture support
```

### MIDI 2.0 / UMP crates

Emerging crates such as `midi20` and `midi2` target MIDI 2.0 / Universal MIDI Packet support. Linux kernel documentation also describes MIDI 2.0/UMP support.

This is not the immediate implementation target, but it affects internal design. Polyrhythm should avoid baking MIDI 1.0 assumptions into its canonical event model.

Prefer:

```text
RawMidiEvent::Midi1(...)
RawMidiEvent::Midi2(...)
  -> CanonicalEvent
```

Avoid:

```text
u8 note + u8 velocity everywhere forever
```

## Plugin and DAW ecosystem

### Standalone virtual MIDI processor

This is the lowest-risk integration path:

```text
hardware MIDI device
  -> Polyrhythm standalone process
  -> virtual MIDI output
  -> DAW / sampler / DrumGizmo / synth
```

Advantages:

```text
works with current ALSA/PipeWire graph
does not require plugin hosting
keeps audio processing out of Polyrhythm
can feed any DAW that can read a MIDI port
```

This should be the first-class runtime shape.

### LV2

LV2 is an open Linux-native plugin standard. The Rust `rust-lv2`/`lv2` ecosystem provides a Rust framework for LV2 plugins.

Potential Polyrhythm use:

```text
LV2 MIDI effect:
  MIDI in -> Pikl predicate mapper -> MIDI out
```

Best DAW fit:

```text
Ardour and other Linux-native LV2 hosts
```

Tradeoffs:

```text
Linux-first rather than cross-platform
plugin lifecycle and realtime-safety requirements
profile loading/state management
```

### CLAP/VST3 via NIH-plug or successors

NIH-plug is a Rust framework for CLAP and VST3 plugins, though upstream search results indicate it is in maintenance mode and point to community fork activity.

Potential Polyrhythm use:

```text
CLAP/VST3 MIDI processor plugin:
  DAW MIDI track -> Polyrhythm profile mapper -> drum/synth plugin
```

Best DAW fit:

```text
REAPER Linux
Bitwig
other CLAP/VST3 hosts
```

Tradeoffs:

```text
host compatibility work
realtime safety
DAW session state and UI work
VST3 distribution/licensing concerns
NIH-plug maintenance risk
```

This is a good medium-term target after the standalone matcher is stable.

### Plugin hosting

Crates such as `rack` point toward Rust plugin hosting across plugin formats. That would let Polyrhythm host instruments directly:

```text
MIDI device -> Polyrhythm canonical mapper -> hosted plugin instrument -> audio out
```

This is powerful but expensive:

```text
plugin discovery
state persistence
realtime audio engine
GUI/editor integration
latency compensation
crash isolation
routing
```

This should not be an early target.

## Linux audio/MIDI web

Polyrhythm sits between several Linux subsystems:

```text
ALSA MIDI / ALSA sequencer
PipeWire MIDI/audio graph
JACK compatibility layer
DrumGizmo
sfizz/SFZ
Hydrogen
LinuxSampler/FluidSynth
Ardour / REAPER / Bitwig
OBS
COSMIC desktop audio control state
```

For this machine, keep the immediate runtime conservative:

```text
TD-50 raw MIDI
  -> Polyrhythm mapper
  -> DrumGizmo or virtual MIDI target
  -> PipeWire-managed audio graph
```

Do not make Polyrhythm a general audio engine prematurely. Its defensible center is semantic MIDI transformation plus graph policy/diagnostics.

## Target adapters

A mature Polyrhythm can support multiple adapters from the same canonical stream.

### DrumGizmo adapter

```text
canonical drum event -> normalized MIDI note -> generated DrumGizmo XML -> kit instrument
```

Use for realistic Linux-native acoustic kit playback.

### GM MIDI adapter

```text
canonical drum event -> General MIDI percussion note
```

Use for:

```text
backend-independent debugging
quick synth fallback
portable MIDI export
DAW sketching
```

### Event log adapter

```text
canonical event -> JSONL / structured log
```

Example:

```json
{"t":1.203,"intent":"kick.main","velocity":91}
{"t":1.501,"intent":"snare.head","velocity":75}
{"t":1.742,"intent":"hihat.edge.closed","velocity":60}
{"t":2.022,"intent":"crash.2.choke","pressure":127}
```

Use for:

```text
debugging
regression tests
practice analysis
offline rendering
notation/export
```

### SFZ/sfizz adapter

SFZ can express richer sample rules than DrumGizmo XML in some areas, including velocity regions, CC conditions, groups, and chokes. A future adapter could generate SFZ-compatible key/CC output or profile metadata.

This should remain secondary until the current CPU/audio concerns around sfizz are intentionally revisited.

### OSC/control adapter

```text
canonical event -> OSC packet / control message
```

Use for:

```text
OBS overlays
visual hit indicators
lighting
practice HUDs
external controllers
```

## Generalizing beyond drums

Pikl should evolve toward profile domains rather than being drum-only.

Possible domains:

```text
drum
keyboard
control_surface
transport
generic_controller
mpe_expression
lighting/osc
```

The general pipeline remains:

```text
Raw MIDI event
  -> DeviceProfile matcher
  -> CanonicalEvent
  -> TargetAdapter
  -> OutputEvent
```

A likely internal shape:

```rust
enum CanonicalEvent {
    Drum(DrumEvent),
    Key(KeyEvent),
    Control(ControlEvent),
    Transport(TransportEvent),
}
```

Output targets can include:

```text
MIDI 1.0 note/CC/aftertouch
MIDI 2.0/UMP later
OSC
JSONL/event log
DrumGizmo XML generation
DAW plugin MIDI output
```

## Profile split

The important split is:

```text
Device profile: interprets raw input.
Target profile: maps canonical events to backend output.
Adapter: knows how to emit for a backend format/runtime.
```

Example device profile concept:

```pikl
device td50 {
  input hihat_bow {
    domain drum
    intent hihat.bow
    notes [46]
    cc 4 range 0..90 orientation inverted
  }

  input crash1_bow {
    domain drum
    intent crash.1.bow
    compound notes [27, 49]
    cc 18 range 40..80
    suppress [27]
  }
}
```

Example target profile concept:

```pikl
target drumgizmo.crocell {
  event hihat.bow.closed {
    emit note 42
    instr HihatClosed
  }

  event crash.2.choke {
    emit note 75
    instr CrashRStopped
  }
}
```

The existing Pikl files are already moving this direction; predicate support is the next structural step.

## Recommended roadmap

### Phase 1: Finish TD-50 drum runtime mapper

```text
Pikl predicates
canonical drum events
DrumGizmo/Crocell target adapter
event log adapter
```

### Phase 2: Add GM MIDI adapter

```text
canonical drum -> GM percussion notes
```

This proves backend independence.

### Phase 3: Add profile coverage reports

Report:

```text
exact
fallback
unsupported
requires_runtime_predicate
```

This turns profile quality into inspectable data.

### Phase 4: Add stable virtual MIDI output

Expose a named output such as:

```text
Polyrhythm Canonical Out
```

This allows DAWs and external tools to consume normalized MIDI.

### Phase 5: Broaden kit/backend profiles

Add or refine:

```text
Crocell
DRS
Muldjord/Aasimonster or other DrumGizmo kits
GM
SFZ candidates
```

### Phase 6: Plugin prototype

Start with a MIDI effect, not an audio instrument:

```text
LV2 MIDI effect for Linux/Ardour
or CLAP MIDI effect for REAPER/Bitwig
```

The plugin should reuse the same compiled predicate engine as the standalone runtime.

### Phase 7: Generalize beyond drums

Add non-drum domains only after the drum path proves the architecture:

```text
keyboard
control surface
transport
MPE/generic expression
OSC/control
```

## Design constraints

1. Keep Pikl canonical.
2. Treat generated XML and MIDI maps as compatibility artifacts.
3. Keep the runtime predicate engine reusable across standalone and plugin modes.
4. Avoid early plugin-host ambitions.
5. Avoid MIDI 1.0 lock-in in the internal event model.
6. Preserve this machine's safety policy: do not restart the audio stack casually, do not route DrumGizmo to speakers by default, and keep OBS/speaker-monitor links treated as possible feedback participants.

## Strategic summary

The platform is not:

```text
TD-50 -> DrumGizmo XML
```

The platform is:

```text
MIDI device -> Pikl semantic profile -> Polyrhythm canonical event stream -> target adapter
```

DrumGizmo is one important adapter. DAWs, GM MIDI, SFZ, LV2/CLAP plugins, OSC, event logs, and future MIDI 2.0 paths are other adapters.
