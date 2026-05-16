# Monitor routing live-validation handoff

This handoff records the live validation results from `nex-jamkit` after the safety-bus work in `polyrhythm`.

## Summary

Engine-only `polyrhythm` runtime startup is now validated live and can become the primary launch path.

Safety-bus monitor routing is **not** validated yet. It failed safely: DrumGizmo connected to `TD50-Safety-Bus`, but the expected `TD50-Safety-Bus.monitor -> onboard sink` links failed with `pw-link` exit status `255`.

The live system was cleaned back to the safe engine-only baseline.

## Environment

Repos:

```text
/home/wilson/workspace/nex-jamkit
/home/wilson/workspace/styrene-lab/polyrhythm
```

Valid physical speaker target on this machine:

```text
alsa_output.pci-0000_0e_00.4.analog-stereo
```

Do not route to HDMI, USB/Yamaha/Burr-Brown, webcam, or OBS by default.

## Live validation that passed

Commands run from `polyrhythm`:

```bash
cd /home/wilson/workspace/styrene-lab/polyrhythm
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- start --kit drs --execute
nix develop --command cargo run --bin polyrhythm -- graph-dump
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Observed good evidence:

```text
ok: stop existing TD-50 clients
ok: start mapper: /home/wilson/.local/bin/td50-drumgizmo-hihat-mapper 32 0
ok: start DrumGizmo: drumgizmo -i jackmidi -I midimap=/home/wilson/workspace/styrene-lab/polyrhythm/assets/drumgizmo/DRSKit/Midimap_td50.xml -o jackaudio -p close=1.0,diverse=0.12,random=0.02 /home/wilson/.local/share/drumgizmo/kits/DRSKit/DRSKit_full.xml
ok: wait up to 90s for DrumGizmo load marker
ok: link Midi-Bridge:TD50-DrumGizmo-Hihat-Mapperout (capture) -> DrumGizmo:drumgizmo_midiin
```

Expected optional link still skipped:

```text
skipped: pw-link exited with exit status: 255: link ... -> DrumGizmo:midi_in
```

This is acceptable because `DrumGizmo:drumgizmo_midiin` is the required MIDI input.

DrumGizmo process after launch:

```text
drumgizmo -i jackmidi -I midimap=/home/wilson/workspace/styrene-lab/polyrhythm/assets/drumgizmo/DRSKit/Midimap_td50.xml -o jackaudio -p close=1.0,diverse=0.12,random=0.02 /home/wilson/.local/share/drumgizmo/kits/DRSKit/DRSKit_full.xml
```

No `-a -s -S` streaming flags.

`~/.cache/td50/drumgizmo.log` showed:

```text
7033 of 7033
done
```

Graph check passed:

```text
drumgizmo running: true
required midi link: true
DrumGizmo audio connections:
  none
speaker sink inputs:
OBS inputs:
graph-check: ok
```

Conclusion:

```text
polyrhythm start --kit drs --execute
polyrhythm graph-check --state engine-only
```

is validated as the primary engine-only runtime path.

## Live validation that failed safely

Commands run:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- monitor-test --execute
nix develop --command cargo run --bin polyrhythm -- graph-dump
nix develop --command cargo run --bin polyrhythm -- graph-check --state overhead-monitor
```

`monitor-test --execute` output:

```text
polyrhythm monitor-test execute
pair: Overheads
sink: alsa_output.pci-0000_0e_00.4.analog-stereo
volume: 5%
- ensure safety bus TD50-Safety-Bus exists at 5%
- clamp monitor alsa_output.pci-0000_0e_00.4.analog-stereo to 5%
- link DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
- link DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
- link TD50-Safety-Bus.monitor:capture_FL -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL
- link TD50-Safety-Bus.monitor:capture_FR -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR
ok: ensure safety bus TD50-Safety-Bus exists at 5%
ok: clamp monitor alsa_output.pci-0000_0e_00.4.analog-stereo to 5%
ok: link DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
ok: link DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
failed: link TD50-Safety-Bus.monitor:capture_FL -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL (exit status: 255)
failed: link TD50-Safety-Bus.monitor:capture_FR -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR (exit status: 255)
```

Graph immediately after failure showed partial monitor routing:

```text
drumgizmo running: true
required midi link: true
DrumGizmo audio connections:
  DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
  DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
speaker sink inputs:
OBS inputs:
```

`graph-check --state overhead-monitor` failed:

```text
missing safety-bus monitor route: TD50-Safety-Bus.monitor:capture_FL -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL
missing safety-bus monitor route: TD50-Safety-Bus.monitor:capture_FR -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR
```

Interpretation:

- Safety-bus creation succeeded.
- DrumGizmo-to-bus links succeeded.
- Bus-to-onboard-sink links failed.
- No unsafe direct DrumGizmo-to-physical-sink route was created.
- This is a fail-closed/safe failure, but monitor workflow is not ready.

## Cleanup performed

Commands run:

```bash
nix develop --command cargo run --bin polyrhythm -- monitor-clear --execute
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Cleanup output:

```text
ok: clear DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
ok: clear DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
skipped: clear TD50-Safety-Bus.monitor:capture_FL -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL (exit status: 255)
skipped: clear TD50-Safety-Bus.monitor:capture_FR -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR (exit status: 255)
```

Final graph check passed:

```text
drumgizmo running: true
required midi link: true
DrumGizmo audio connections:
  none
speaker sink inputs:
OBS inputs:
graph-check: ok
```

The live system is back at safe engine-only baseline.

## Required next fix

Fix monitor routing port resolution for the safety bus.

The current assumption appears wrong:

```text
TD50-Safety-Bus.monitor:capture_FL
TD50-Safety-Bus.monitor:capture_FR
```

Those names did not link to the onboard sink with `pw-link`; both failed with exit status `255`.

Use bounded graph evidence to discover the actual node/port names created by:

```text
pactl load-module module-null-sink sink_name=TD50-Safety-Bus channels=2 channel_map=front-left,front-right sink_properties=device.description=TD50-Safety-Bus
```

Do **not** use unbounded `pw-link -l` / `pw-link -io` discovery. Existing `polyrhythm graph-dump` uses bounded `pw-dump` and is acceptable.

Recommended implementation direction:

1. After ensuring the safety bus, capture/parse a bounded graph snapshot or otherwise use bounded PipeWire/Pulse inspection.
2. Resolve the actual safety-bus monitor source node and output/capture ports from graph data.
3. Generate bus-to-sink links from resolved ports, not hard-coded `TD50-Safety-Bus.monitor:capture_FL/FR` strings.
4. Update `graph-check --state overhead-monitor` to expect the actual resolved/rendered route shape, or normalize aliases so the check is stable across PipeWire/Pulse naming differences.
5. Keep failure closed: if monitor source ports cannot be resolved, fail before or during monitor linking. Never fall back to direct DrumGizmo-to-onboard-sink routing.

Likely relevant files:

```text
src/monitor.rs
src/start.rs
src/graph.rs
src/cli.rs
```

## Acceptance criteria

### Static validation

```bash
cd /home/wilson/workspace/styrene-lab/polyrhythm
nix develop --command cargo test --all-targets
nix develop --command cargo clippy --all-targets -- -D warnings
```

### Dry-run expectations

```bash
nix develop --command cargo run --bin polyrhythm -- monitor-test --dry-run
```

Should still show:

```text
ensure safety bus TD50-Safety-Bus exists at 5%
clamp monitor alsa_output.pci-0000_0e_00.4.analog-stereo to 5%
```

It may show resolved bus source ports if available only at execute time; if so, the dry-run should clearly state that bus monitor ports are resolved during execution.

### Live validation

Start from safe baseline:

```bash
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Then run:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- monitor-test --execute
nix develop --command cargo run --bin polyrhythm -- graph-dump
nix develop --command cargo run --bin polyrhythm -- graph-check --state overhead-monitor
```

Expected:

```text
ok: ensure safety bus TD50-Safety-Bus exists at 5%
ok: clamp monitor ... to 5%
ok: link DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
ok: link DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
ok: link <resolved safety-bus-monitor-left> -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL
ok: link <resolved safety-bus-monitor-right> -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR
graph-check: ok
```

Then cleanup must restore engine-only:

```bash
nix develop --command cargo run --bin polyrhythm -- monitor-clear --execute
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
```

Expected:

```text
DrumGizmo audio connections:
  none
graph-check: ok
```

## nex-jamkit promotion decision

`nex-jamkit` can now promote `polyrhythm` for engine-only runtime startup.

Do not promote monitor workflow until this handoff's live monitor validation passes.

Recommended `nex-jamkit` state now:

```text
primary engine start: polyrhythm start --kit drs --execute
primary engine check: polyrhythm graph-check --state engine-only
legacy fallback: td50-kit drs / bin/td50-start-drumgizmo
monitor workflow: blocked pending safety-bus port-resolution fix
```
