# polyrhythm

Rust tooling for e-drum MIDI mapping and live drum-rig stability.

This repo starts as the Rust extraction path from `styrene-lab/nex-jamkit`'s
Roland TD-50 / DrumGizmo mapper. The first goal is a tested, pure mapping core
that can replace ad-hoc C mapper binaries without changing the known-good live
audio route until ALSA integration is explicitly validated.

## Current status

- Pure Rust TD-50 mapping core exists.
- ALSA sequencer I/O is not implemented yet.
- The prototype binary exits with status 2 so it cannot be mistaken for a live
  mapper.

## Safety constraints

- Do not probe broad PipeWire graphs as part of normal control flows.
- Do not restart PipeWire/WirePlumber as part of kit switching.
- Preserve the known TD-50 mapper client/port names when ALSA I/O is added:
  - client: `TD50-DrumGizmo-Hihat-Mapper`
  - port: `out`
- Keep live replacement opt-in until validated against the DRS DrumGizmo path.
