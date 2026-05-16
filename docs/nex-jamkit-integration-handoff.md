# nex-jamkit integration handoff for polyrhythm

This is a handoff from the `nex-jamkit` agent. The goal is to reconcile `polyrhythm` runtime policy with the latest confirmed safe TD-50 / DrumGizmo behavior in `/home/wilson/workspace/nex-jamkit`.

## Current division of responsibility

```text
nex-jamkit installs/provides packages, assets, and compatibility shell wrappers.
polyrhythm owns runtime graph policy, safe start/check/route behavior, and diagnostics.
```

Do not make `nex-jamkit` the long-term runtime authority. Instead, port the proven safety behavior from `nex-jamkit` into `polyrhythm`, then let `nex-jamkit` wrap or document `polyrhythm` commands.

## Hard safety constraints

Follow the local `AGENTS.md` rules in this repo. In particular:

- Do not restart PipeWire, WirePlumber, or pipewire-pulse.
- Do not use broad/unbounded graph discovery.
- Do not kill DrumGizmo or the mapper during diagnosis unless explicitly doing a teardown/start operation.
- Do not route DrumGizmo audio to speakers by default.
- Treat OBS routes and speaker-monitor feeds as possible feedback-loop participants.

Additional constraints confirmed by `nex-jamkit`:

- DRSKit is the only normal/recovery kit path.
- Do not switch engines or kits during an incident.
- Do not use sfizz as fallback unless explicitly requested after warning about prior CPU/no-audio issues.
- The only normal speaker target on this machine is:
  `alsa_output.pci-0000_0e_00.4.analog-stereo`
- Do not route to HDMI, Yamaha/Burr-Brown USB audio, webcams, or OBS by default.

## Highest-priority mismatch: DrumGizmo streaming mode

`polyrhythm/src/start.rs` currently plans DrumGizmo with async/streaming flags:

```text
-a -s -S limit=67108864
```

That is now known unsafe on this machine.

Observed bad behavior from `nex-jamkit`:

- DrumGizmo launched with `-a -s -S limit=67108864` stayed at the initial load line:
  `Loading drumkit, this may take a while:`
- drum audio was extremely quiet and corrupt / deep-fried / 8-bit sounding.
- this path is treated as hazardous until isolated.

Observed good behavior:

- DrumGizmo launched without `-a -s -S` full-loaded DRSKit.
- Log progressed through all `7033` samples and ended with:
  `done`
- Current successful command shape:

```text
drumgizmo \
  -i jackmidi -I midimap=/home/wilson/workspace/nex-jamkit/assets/drumgizmo/DRSKit/Midimap_td50.xml \
  -o jackaudio \
  -p close=1.0,diverse=0.12,random=0.02 \
  /home/wilson/.local/share/drumgizmo/kits/DRSKit/DRSKit_full.xml
```

### Required change

Make full-load / non-streaming DrumGizmo the default in `polyrhythm`.

Implementation target:

- `src/start.rs`
- tests in the same module
- CLI/help/docs as needed

Expected behavior:

- Default start plan must not include `-a`, `-s`, or `-S`.
- Streaming may remain available only as explicit diagnostic opt-in, e.g. env/flag such as `TD50_DRUMGIZMO_STREAMING=1` or `--streaming`.
- If streaming is disabled, execution should wait for DrumGizmo load completion before MIDI/audio routing.
- Load completion should be detected from `drumgizmo.log` containing `done`.
- Add a bounded timeout, matching or replacing the shell default:
  `TD50_DRUMGIZMO_LOAD_TIMEOUT`, default `90` seconds.

Do not mark `polyrhythm start --execute` as the canonical path until this is fixed.

## Second mismatch: monitor routing must use a safety bus

`nex-jamkit` changed monitor policy after direct speaker routes caused unsafe transitions.

Current `nex-jamkit` policy:

```text
Default: engine + MIDI only, no DrumGizmo speaker path.
If monitor routing is explicitly enabled:
  DrumGizmo overhead pair -> TD50-Safety-Bus -> onboard analog sink
```

The safety bus defaults are:

```text
TD50_SAFETY_BUS_NAME=TD50-Safety-Bus
TD50_SAFETY_BUS_VOLUME=5%
TD50_ROUTE_MONITOR=0
TD50_MONITOR_VOLUME=5%
TD50_MONITOR_SINKS=alsa_output.pci-0000_0e_00.4.analog-stereo
```

`polyrhythm/src/monitor.rs` and `polyrhythm/src/graph.rs` currently model direct overhead monitor routes:

```text
DrumGizmo:5-OHL -> alsa_output...:playback_FL
DrumGizmo:6-OHR -> alsa_output...:playback_FR
```

That is now stale relative to the safer shell launcher.

### Required change

Adopt a low-volume PipeWire loopback safety bus for monitor routing, or explicitly mark direct physical-sink routing as a dangerous/manual override.

Implementation targets:

- `src/monitor.rs`
- `src/graph.rs`
- CLI command `monitor-test`
- graph-check state `overhead-monitor`
- docs/tests

Expected safe monitor shape:

```text
DrumGizmo:5-OHL -> TD50-Safety-Bus:playback_FL
DrumGizmo:6-OHR -> TD50-Safety-Bus:playback_FR
TD50-Safety-Bus.monitor:capture_FL -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FL
TD50-Safety-Bus.monitor:capture_FR -> alsa_output.pci-0000_0e_00.4.analog-stereo:playback_FR
```

The bus should start muted or at low volume before DrumGizmo is linked, then open only after the intended links are present.

## Third integration surface: graph diagnostics are the canonical recovery loop

`polyrhythm` already has the better recovery/diagnostic commands:

```bash
polyrhythm quiet --volume 5%
polyrhythm graph-dump
polyrhythm graph-check --state engine-only
```

Once the streaming and monitor-safety mismatches are fixed, `nex-jamkit` should wrap these rather than growing more shell graph logic.

Desired normal loop after fixes:

```bash
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

If feedback/screech occurs, preserve evidence:

```bash
nix develop --command cargo run --bin polyrhythm -- quiet --volume 1%
nix develop --command cargo run --bin polyrhythm -- graph-dump
```

Do not stop the stack just to inspect the graph.

## Fourth integration surface: shared runtime evidence

`nex-jamkit` currently writes legacy runtime state to:

```text
~/.cache/td50/status.txt
~/.cache/td50/drumgizmo.pid
~/.cache/td50/hihat-mapper.pid
~/.cache/td50/drumgizmo.log
~/.cache/td50/hihat-mapper.log
```

`polyrhythm` writes richer evidence:

```text
~/.cache/polyrhythm/current-run.json
~/.cache/polyrhythm/runs/*.json
~/.cache/polyrhythm/graphs/*.json
trace JSONL via polyrhythm trace
```

Recommended direction:

- `polyrhythm` should become the canonical runtime evidence format.
- During transition, either teach `polyrhythm status` to consume legacy `~/.cache/td50` state or have `nex-jamkit` wrappers write enough compatible manifest data.
- Keep `nex-jamkit` shell launchers as compatibility wrappers until `polyrhythm start --execute` matches the proven safe behavior.

## Fifth integration surface: OBS/V4L2 incident guidance

`polyrhythm/docs/known-issues.md` contains an important incident note that should influence runtime policy:

Known from evidence:

- A lockup after audio-stack restart showed kernel RCU stalls in `data-loop.0`.
- OBS / V4L2 `/dev/video2` spammed `select timed out` and `failed to log status` thousands of times.
- The captured evidence did not show DrumGizmo/polyrhythm as the active participant.
- No suspend/resume journal markers were found.

Policy implication:

- Do not treat every audio/desktop lockup as a drum-graph recovery case.
- Do not restart PipeWire/WirePlumber during evidence preservation.
- Flag OBS receiving the onboard speaker monitor feed as suspicious.
- Keep OBS routes off by default.

`nex-jamkit` should import this documentation later; `polyrhythm` should keep enforcing it in graph checks.

## Acceptance criteria for this handoff

Before asking `nex-jamkit` to delegate live launch to `polyrhythm`, verify:

1. `cargo test --all-targets` passes.
2. `cargo clippy --all-targets -- -D warnings` passes.
3. `polyrhythm plan --kit drs` shows no DrumGizmo `-a -s -S` flags by default.
4. `polyrhythm start --kit drs --execute` starts DRSKit full-load mode by default.
5. In full-load mode, `polyrhythm` waits for `drumgizmo.log` to contain `done` before routing.
6. `polyrhythm graph-check --state engine-only` passes after start with default routing.
7. Monitor test routes through `TD50-Safety-Bus` or is blocked behind an explicit unsafe override.
8. OBS routing remains off by default.
9. Direct full close-mic fanout to the onboard sink is not available as a default path.

## Files in nex-jamkit to compare against

Reference repo:

```text
/home/wilson/workspace/nex-jamkit
```

Relevant files:

```text
bin/td50-start-drumgizmo
bin/td50-stop
bin/td50-status
bin/td50-kit
Justfile
docs/td50-drum-stack.md
docs/dependency-contract.md
```

Current known-good launcher invocation from `nex-jamkit`:

```bash
TD50_ROUTE_OBS=0 \
TD50_ROUTE_MONITOR=0 \
TD50_MONITOR_VOLUME=5% \
TD50_DRUMGIZMO_STREAMING=0 \
TD50_DRUMGIZMO_LOAD_TIMEOUT=180 \
/home/wilson/workspace/nex-jamkit/bin/td50-kit drs
```

Current status evidence after that launch:

```text
Audio routing attempted: audio=1 monitor=0 obs=0
Streaming mode: 0
DrumGizmo loaded 7033 of 7033 samples and logged: done
```


## polyrhythm implementation status

Current local implementation status after importing this handoff:

- `polyrhythm start --kit drs` now defaults to DrumGizmo full-load mode; the default planned command does not include `-a`, `-s`, or `-S`.
- Streaming mode is explicit opt-in via either `TD50_DRUMGIZMO_STREAMING=1` or `--streaming` on `plan`, `start`, and `switch`.
- Full-load mode inserts a bounded wait for `drumgizmo.log` to contain a `done` line before MIDI or monitor links are attempted. The timeout is `TD50_DRUMGIZMO_LOAD_TIMEOUT`, default `90` seconds.
- DrumGizmo logs are truncated on process start so stale `done` markers cannot satisfy the new load wait.
- Later execution operations are skipped after an earlier required operation fails, so routing does not proceed after a load timeout.
- `monitor-test` and `start --route-monitor` now plan overhead-only routes through `TD50-Safety-Bus` instead of direct DrumGizmo-to-sink links.
- `start --route-monitor` now ensures `TD50-Safety-Bus` exists as a PipeWire/Pulse null sink before monitor links are attempted, using `pactl load-module module-null-sink` when needed.
- The safety bus and physical monitor sink are clamped to low volume (`5%` by default) before DrumGizmo monitor links are made.
- `graph-check --state overhead-monitor` now expects the `TD50-Safety-Bus` route shape and flags direct DrumGizmo-to-onboard-sink routes as unsafe.

Known remaining gap:

- The safety bus is created/ensured, but polyrhythm does not yet track or unload bus modules it created. That is intentionally conservative: teardown is avoided until there is stronger evidence about module ownership and live graph effects.
