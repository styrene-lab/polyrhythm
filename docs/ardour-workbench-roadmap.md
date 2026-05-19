# Polyrhythm translation layer toward Ardour and future workbench UI

This guide describes where Polyrhythm now stands after the TD-50 capture/profile work and the initial workbench regression backend, and how that maps into an Ardour-oriented recording path.

The immediate goal is **not** to make Ardour the live drum engine. The immediate goal is to make Ardour a reliable consumer of Polyrhythm's normalized MIDI and/or DrumGizmo audio while Polyrhythm remains the semantic translation authority.

## Current architectural position

Polyrhythm now has three important layers:

```text
1. Device/kit profiles
   profiles/devices/td50.pikl
   profiles/kits/crocell.pikl
   profiles/kits/drs.pikl

2. Existing mapper core
   src/td50_mapper.rs
   hardcoded TD-50 behavior for hi-hat, crash1, velocity curves, suppression

3. Workbench backend foundation
   src/workbench/event.rs
   src/workbench/coverage.rs
   src/workbench/trace.rs
   src/workbench/replay.rs
```

The system has moved from ad-hoc map editing toward a testable translation-layer architecture:

```text
raw TD-50 MIDI
  -> Polyrhythm canonical/normalized mapping
  -> target output notes/events
  -> DrumGizmo / DAW / event log / future UI
```

The important principle remains:

```text
Pikl and Polyrhythm are canonical.
DrumGizmo XML and DAW note maps are generated/compatibility artifacts.
```

## What the current backend gives us

### Static coverage

Command:

```bash
polyrhythm workbench coverage --device td50 --kit crocell
```

This reports:

```text
device input -> canonical intent -> kit target -> exact/fallback/unsupported quality
```

It is safe:

```text
live audio: skipped
PipeWire graph probing: skipped
```

This is the first profile-builder primitive.

### JSONL event traces

Coverage can emit workbench JSONL:

```bash
polyrhythm workbench coverage --device td50 --kit crocell --jsonl coverage.jsonl
```

The workbench trace format now supports:

```text
raw_observed
canonical_matched
target_resolved
expect_output
warning
```

### Regression replay

Command:

```bash
polyrhythm workbench replay fixture.jsonl --device td50 --kit crocell
```

Fixture example:

```jsonl
{"type":"raw_observed","time_millis":1,"source":"fixture","event":{"kind":"control_change","channel":0,"controller":4,"value":32}}
{"type":"raw_observed","time_millis":2,"source":"fixture","event":{"kind":"note_on","channel":0,"note":46,"velocity":90}}
{"type":"expect_output","event":{"kind":"note_on","channel":0,"note":41,"velocity":90}}
```

Expected output:

```text
regression: ok
```

This gives us the golden path for safe refactors:

```text
captured raw MIDI fixture
  -> expected normalized output
  -> replay offline
  -> fail if mapper behavior changes unintentionally
```

This matters because the next major implementation step is migrating hardcoded TD-50 logic into profile-driven predicate support.

## Where this leaves the Ardour translation layer

Ardour should consume **normalized Polyrhythm output**, not raw TD-50 MIDI.

The correct Ardour path is:

```text
TD-50 raw MIDI
  -> Polyrhythm semantic mapper
  -> Polyrhythm virtual MIDI output
  -> Ardour MIDI track
  -> DrumGizmo/plugin/GM/SFZ target inside or outside Ardour
```

There are two near-term Ardour workflows.

## Workflow A: Record normalized MIDI into Ardour

This is the most important DAW integration path.

```text
TD-50
  -> Polyrhythm mapper
  -> virtual MIDI port: Polyrhythm Canonical Out
  -> Ardour MIDI track input
```

Benefits:

```text
Ardour records clean notes, not TD-50 quirks.
Hi-hat openness is already bucketed.
Crash companion notes are already suppressed.
Velocity curves are already applied.
Future chokes/cross-stick predicates are already translated.
Recorded performances can later be rendered through different kits.
```

This avoids recording raw TD-50 behaviors such as:

```text
compound crash notes
CC4-dependent hi-hat state
poly-aftertouch choke gestures
controller-specific suppression needs
```

### First implementation requirement

Polyrhythm needs a stable virtual MIDI output mode:

```bash
polyrhythm start --target virtual-midi
```

or a narrower command:

```bash
polyrhythm mapper --device td50 --kit crocell --virtual-out "Polyrhythm Canonical Out"
```

The exact command name can change, but the system behavior should be:

```text
open TD-50/ALSA MIDI input
open named virtual MIDI output
run mapper
emit normalized MIDI
no audio routing changes by default
```

This command must not route DrumGizmo audio or touch OBS.

## Workflow B: Record DrumGizmo audio/stems into Ardour

This path keeps Polyrhythm/DrumGizmo as the live rendering engine and lets Ardour record audio.

```text
TD-50
  -> Polyrhythm mapper
  -> DrumGizmo
  -> PipeWire/JACK audio ports
  -> Ardour audio tracks
```

Possible recording levels:

```text
1. stereo monitor/summary track
2. drum support bus
3. selected close mics
4. full stem fanout later
```

Given this machine's known safety constraints, start with conservative routing:

```text
record only a bounded stereo/small-bus feed
avoid full close-mic fanout to speakers
avoid OBS feedback paths
keep live monitor routing separate from Ardour recording routing
```

Ardour should initially be a passive recorder, not the live monitor authority.

## Recommended Ardour milestones

### Milestone 1: Offline validation before Ardour

Before opening Ardour, ensure the mapper behavior is locked by replay fixtures.

Add fixtures for:

```text
hi-hat CC4 buckets
hi-hat edge buckets
hi-hat note-off fanout
crash1 CC18/velocity split
crash1 companion-note suppression
future choke predicates
future cross-stick velocity predicate
```

The command should become a standard pre-Ardour check:

```bash
polyrhythm workbench replay fixtures/td50/hihat-semi-open.jsonl --device td50 --kit crocell
```

### Milestone 2: Virtual MIDI output

Implement the standalone normalized MIDI output path.

Desired graph:

```text
U2MIDI Pro / TD-50 input
  -> Polyrhythm Mapper
  -> Polyrhythm Canonical Out
```

Validation:

```text
aseqdump or workbench observe sees raw input
Ardour sees Polyrhythm Canonical Out as MIDI input
recorded Ardour notes match replay expectations
```

### Milestone 3: Ardour normalized MIDI recording

In Ardour:

```text
create MIDI track
input = Polyrhythm Canonical Out
record-enable track
record short take
inspect piano-roll/event list
```

Expected result:

```text
hihat semi-open records normalized note, not raw CC4-dependent ambiguity
crash1 does not record doubled companion notes
fallback articulations are visible as target notes
```

### Milestone 4: Ardour audio capture

After MIDI recording works, add audio capture from DrumGizmo/PipeWire.

Start small:

```text
DrumGizmo bounded stereo pair or chosen recording bus -> Ardour stereo audio track
```

Do not start with full mic fanout.

### Milestone 5: Session template

Create an Ardour session template with:

```text
MIDI track: Polyrhythm Canonical In
Audio track: drum stereo/recording bus
Reference audio track if needed
Buses for drum processing
Clear routing labels
```

This template should assume Polyrhythm owns MIDI semantics.

## Longer-term Ardour plugin path

After the standalone mapper is stable, an Ardour-native plugin path is possible:

```text
Ardour raw MIDI track
  -> Polyrhythm LV2 MIDI effect
  -> instrument/plugin/DrumGizmo path
```

But this is **not** the first path.

The plugin path has extra obligations:

```text
realtime-safe matcher
no file I/O in process callback
precompiled Pikl profiles
DAW session state serialization
LV2 metadata/UI
host compatibility testing
```

The standalone virtual MIDI path should come first because it proves the mapper without DAW plugin complexity.

## Future workbench/profile-builder UI

The UI should be a consumer of the workbench backend event stream, not a separate mapper.

The backend stream already starts with:

```text
RawObserved
CanonicalMatched
TargetResolved
ExpectOutput
Warning
```

Future event types will likely include:

```text
StateUpdated
PredicateEvaluated
OutputEmitted
CoverageUpdated
ProfileProposalCreated
GraphWarning
```

### UI principle

The UI must answer these questions for every hit:

```text
What raw MIDI was observed?
What state was read? e.g. CC4/CC18
Which predicate matched?
What canonical intent resulted?
What target output was selected?
Was the mapping exact, fallback, unsupported, or predicate-required?
What MIDI/audio output was emitted?
```

If the backend cannot answer those questions, the UI should not guess.

## UI phase plan

### Phase 1: Read-only TUI/CLI inspector

Build before graphical UI.

Command shape:

```bash
polyrhythm workbench observe --device td50 --kit crocell
```

Display:

```text
last raw event
current CC state
canonical match
target output
coverage quality
warnings
```

No profile mutation.

### Phase 2: JSONL capture mode

Command shape:

```bash
polyrhythm workbench observe --device td50 --kit crocell --jsonl capture.jsonl
```

Use this to create replay fixtures.

Flow:

```text
capture live raw_observed events
trim fixture
add expect_output lines
commit fixture
replay in tests
```

### Phase 3: Visual web UI

Optional after backend stability.

Architecture:

```text
polyrhythm workbench serve
  -> WebSocket event stream
  -> browser-rendered drum kit / controls
```

UI panels:

```text
Live kit view
Raw MIDI inspector
Canonical event inspector
Target mapping inspector
Predicate/evidence panel
Coverage table
Regression fixture builder
```

Rendered kit colors:

```text
green  = exact
yellow = fallback
red    = unsupported
purple = requires runtime predicate
blue   = observed live
```

### Phase 4: MIDI Learn/profile proposal mode

Do not directly mutate Pikl during live capture.

Safe flow:

```text
observe
infer candidate rule
create proposal
review proposal
apply proposal
validate profile
replay fixtures
```

Possible command shape:

```bash
polyrhythm workbench learn --device td50 --intent crash.2.edge --proposal proposal.json
polyrhythm workbench apply-proposal proposal.json
```

### Phase 5: Ardour-aware diagnostics

Once Ardour enters the graph, the UI can show:

```text
Polyrhythm MIDI input connected?
Polyrhythm Canonical Out connected to Ardour?
Ardour MIDI track receiving events?
DrumGizmo audio route connected to Ardour audio track?
OBS/speaker monitor feedback warning?
```

But graph mutation should remain explicit and guarded.

## Backend tasks before UI

The UI should wait until these backend tasks exist:

```text
1. More replay fixtures for known TD-50 behaviors.
2. Workbench observe mode for live raw MIDI capture.
3. Predicate model in Pikl/device profiles.
4. Generic matcher that emits CanonicalMatched with evidence.
5. Target resolver that emits TargetResolved and OutputEmitted.
6. Virtual MIDI output path for Ardour.
7. Bounded graph checks for Ardour recording routes.
```

## Audio path continuation

The immediate audio path should remain conservative.

Current safe principle:

```text
Polyrhythm/DrumGizmo live stack first.
Ardour as passive consumer second.
OBS/speaker monitor feedback guarded.
No full close-mic fanout to speakers by default.
```

Recommended next audio steps:

```text
1. Preserve current working Spotify + drums + OBS state.
2. Add more regression fixtures before changing mapper behavior.
3. Implement virtual MIDI output mode without touching audio routing.
4. Verify Ardour can record Polyrhythm Canonical Out.
5. Then add a bounded Ardour audio recording route.
```

## Practical next work items

### Code

```text
1. Add fixture files under a committed fixtures directory.
2. Add tests that load all fixtures and replay them.
3. Add workbench observe command for safe raw MIDI capture.
4. Add virtual MIDI output mode.
5. Add predicate structs and profile parser support.
```

### Docs

```text
1. Ardour setup checklist.
2. Fixture authoring guide.
3. Workbench UI design notes.
4. Routing safety notes for Ardour/OBS/PipeWire.
```

### Safety checks

Before any Ardour live routing command:

```text
run audio-doctor if desktop audio state is suspect
run graph-check/graph-dump only in bounded form
verify OBS is not receiving speaker monitor feed if monitor routing is involved
keep live monitor volume clamped
```

## Summary

Where we are:

```text
TD-50 capture is substantially complete.
Pikl profiles model the simple mapping layer.
The mapper core handles important hardcoded TD-50 behaviors.
Workbench coverage, JSONL trace, replay, and expected-output regression primitives now exist.
```

Where this leaves Ardour:

```text
Ardour should receive normalized Polyrhythm MIDI, not raw TD-50 MIDI.
The first Ardour integration should be virtual MIDI recording.
Audio/stem capture should come after MIDI recording is stable.
LV2 plugin integration is a later path, not the first path.
```

Where this leaves UI:

```text
Build UI after backend observe/replay/predicate evidence are stable.
The UI should render backend truth, not implement mapping logic.
```
