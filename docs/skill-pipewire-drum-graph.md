# Skill: PipeWire drum graph debugging

Use this skill when working on the local TD-50 / DrumGizmo rig in `polyrhythm`.

## Goal

Preserve graph evidence while converging on a safe playable routing state.

## Desired baseline state

```text
U2MIDI Pro MIDI 1 (capture)
  -> TD50-DrumGizmo-Hihat-Mapper
  -> Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture)
  -> DrumGizmo:drumgizmo_midiin

DrumGizmo audio outputs:
  disconnected
```

This baseline proves MIDI + engine health without risking speaker feedback.

## Never do during diagnosis

- Do not restart PipeWire/WirePlumber.
- Do not stop/kill DrumGizmo just to reduce volume or inspect the graph.
- Do not use `stop --force` unless the operator asks to tear down the rig.
- Do not connect full DrumGizmo close-mic fanout to the onboard sink.

## Safe commands

Lower speaker level without destroying graph state:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
```

Capture and summarize current graph:

```bash
nix develop --command cargo run --bin polyrhythm -- graph-dump
```

Check safe baseline:

```bash
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Open visual graph only for human inspection:

```bash
nix develop --command cargo run --bin polyrhythm -- graph
```

## Real-world loop

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 5%
nix develop --command cargo run --bin polyrhythm -- start --kit drs --execute
nix develop --command cargo run --bin polyrhythm -- graph-dump
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

If feedback/screech occurs:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- graph-dump
```

Then inspect the text summary. Do not destroy the graph.

## Interpretation

`graph-check --state engine-only` should pass when:

- DrumGizmo is running.
- Required MIDI link exists.
- No DrumGizmo audio output connects to the onboard speaker sink.

Warnings about OBS receiving onboard speaker monitor feed are significant. They may indicate a feedback route independent of DrumGizmo.
