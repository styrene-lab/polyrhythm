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
polyrhythm start --kit drs --dry-run
polyrhythm switch --kit drs --dry-run
polyrhythm preflight
polyrhythm status
polyrhythm status --strict
polyrhythm stop --dry-run
polyrhythm trace path
polyrhythm graph
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

## Known local side effects

Starting or stopping the TD-50/DrumGizmo stack can cause the physical speakers to
emit a sudden burst/blare as JACK/PipeWire routes are created, removed, or
reconnected. This is a known side effect of live audio graph churn on the current
rig, not evidence that broad PipeWire probing is occurring.

Near-term safety wrapper work should mute or clamp monitor-facing sources before
feedback-prone operations, then restore only the intended onboard monitor path.
Until that exists, keep playback paused and monitor volume conservative before
`polyrhythm start --execute`, `polyrhythm stop`, or kit switching.

The CLI now applies a first-pass monitor safety wrapper around live start/stop:
it drops the configured monitor sink to a low safety volume before graph churn
and restores the configured monitor volume afterward. This is a mitigation, not
a guarantee; keep external playback paused for live stack transitions.

## Graph feedback loop

Use these commands during PipeWire routing diagnostics. `quiet` is non-destructive: it lowers the monitor sink without killing DrumGizmo or the mapper.

```bash
polyrhythm start --kit drs --execute       # engine + MIDI only by default
polyrhythm graph-dump                      # bounded pw-dump snapshot and summary
polyrhythm graph-check --state engine-only # expected safe baseline
polyrhythm quiet --volume 5%               # lower speakers without destroying graph evidence
```

Current desired baseline is `engine-only`: DrumGizmo running, mapper MIDI linked, and all DrumGizmo audio outputs disconnected. OBS receiving the onboard speaker monitor feed is flagged suspicious because it may participate in feedback loops.
