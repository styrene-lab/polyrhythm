# polyrhythm

Rust tooling for e-drum MIDI mapping and live drum-rig stability.

This repo starts as the Rust extraction path from `styrene-lab/nex-jamkit`'s
Roland TD-50 / DrumGizmo mapper. The goal is a small, boring CLI that replaces
ad-hoc shell orchestration with explicit safety policy and tested MIDI mapping.

## Current status

- Pure Rust TD-50 mapping core exists.
- Initial `polyrhythm` CLI exists for offline planning/policy/doctor checks.
- ALSA sequencer I/O is not implemented yet.
- The prototype mapper binary exits with status 2 so it cannot be mistaken for a
  live mapper.

## CLI

```sh
polyrhythm policy
polyrhythm plan --kit drs
polyrhythm doctor
polyrhythm preflight
polyrhythm status
polyrhythm stop --dry-run
polyrhythm trace path
polyrhythm trace tail
polyrhythm legacy-env
```

These commands are intentionally offline/safe: they do not start DrumGizmo, do
not probe the PipeWire graph, and do not restart audio services. `stop` is the
first live-control command: it only targets known TD-50 client processes and
pidfiles, never PipeWire/WirePlumber.

## Safety constraints

- DRS is the only default kit path.
- Alternate kits are explicit diagnostics only.
- OBS routing is off by default.
- Do not probe broad PipeWire graphs as part of normal control flows.
- Do not restart PipeWire/WirePlumber as part of kit switching.
- Preserve the known TD-50 mapper client/port names when ALSA I/O is added:
  - client: `TD50-DrumGizmo-Hihat-Mapper`
  - port: `out`
- Keep live replacement opt-in until validated against the DRS DrumGizmo path.
