# AGENTS.md

## Project mission

`polyrhythm` is the Rust control surface for the local TD-50 / DrumGizmo drum rig. The immediate goal is reliable local playability on this NixOS/COSMIC machine before generalizing.

## Hard safety rules

- Do not restart PipeWire, WirePlumber, or the desktop audio stack.
- Do not kill DrumGizmo or the mapper during graph diagnosis unless the operator explicitly asks for teardown.
- Do not use `polyrhythm stop --force` as a diagnostic action; it destroys the graph evidence.
- Do not route DrumGizmo audio to speakers by default.
- Do not re-enable full close-mic fanout to the onboard sink without an explicit operator decision.
- Treat OBS monitor/capture links as possible feedback-loop participants.
- Prefer bounded graph snapshots with `pw-dump`; avoid unbounded graph discovery.

## Normal diagnostic loop

Use this loop when debugging live routing:

```bash
cd /home/wilson/workspace/styrene-lab/polyrhythm
nix develop --command cargo run --bin polyrhythm -- quiet --volume 5%
nix develop --command cargo run --bin polyrhythm -- start --kit drs --execute
nix develop --command cargo run --bin polyrhythm -- graph-dump
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Expected safe baseline:

```text
DrumGizmo running: true
required MIDI link: true
DrumGizmo audio connections: none
```

If the operator reports screech/feedback during graph diagnosis, use:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- graph-dump
```

Do not stop the stack unless asked.

## Validation

Before committing Rust changes:

```bash
nix develop --command cargo fmt
nix develop --command cargo test --all-targets
nix develop --command cargo clippy --all-targets -- -D warnings
nix develop --command cargo build --all-targets
```

## Current known graph facts

The desired baseline is `engine-only`:

```text
U2MIDI Pro -> TD50-DrumGizmo-Hihat-Mapper -> DrumGizmo:drumgizmo_midiin
DrumGizmo audio outputs -> disconnected
```

The onboard speaker sink is:

```text
alsa_output.pci-0000_0e_00.4.analog-stereo
```

Suspicious current/non-drum route pattern:

```text
alsa_output.pci-0000_0e_00.4.analog-stereo:monitor_FL -> OBS:input_FL
alsa_output.pci-0000_0e_00.4.analog-stereo:monitor_FR -> OBS:input_FR
```

This is flagged because OBS receiving the speaker monitor feed can participate in feedback loops if OBS monitoring routes back to speakers.

## Screenshot limitation

Omegon image viewing is currently broken on this NixOS/COSMIC deployment: PNG screenshots return metadata only. Upstream issue:

```text
https://github.com/styrene-lab/omegon/issues/70
```

Use `polyrhythm graph-dump` / `graph-check` as the primary feedback loop instead of screenshots.
