# polyrhythm

Rust tooling for e-drum MIDI mapping and live drum-rig stability.

This repo starts as the Rust extraction path from `styrene-lab/nex-jamkit`'s
Roland TD-50 / DrumGizmo mapper. The goal is a small, boring CLI that replaces
ad-hoc shell orchestration with explicit safety policy and tested MIDI mapping.

## Current status

- Pure Rust TD-50 mapping core exists, including reversible velocity curves for feel testing.
- The Rust TD-50 mapper has ALSA sequencer I/O and preserves the legacy client/port contract.
- `polyrhythm` is the validated runtime authority for DRS DrumGizmo start/check/monitor routing on this host.
- Desktop audio recovery is explicit: `recover-audio` handles PipeWire/WirePlumber, COSMIC audio UI state, and common media clients together.

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
polyrhythm audio-doctor
polyrhythm recover-audio --dry-run
polyrhythm recover-audio --execute
polyrhythm devices
polyrhythm kits
polyrhythm profile-inspect --device td50 --kit crocell
polyrhythm map-check --device td50 --kit crocell
polyrhythm generate-midimap --device td50 --kit crocell
```

Most commands are intentionally bounded: normal start/switch/monitor controls do not restart audio services, and graph inspection uses bounded snapshots instead of broad discovery. `recover-audio --execute` is the explicit exception for incident recovery; it recovers PipeWire/WirePlumber, onboard analog defaults, COSMIC audio UI state, and configured media clients as one dependency chain.

## Canonical profiles

Pikl is the canonical profile format for device and kit mapping. DrumGizmo XML midimaps are generated compatibility artifacts only; do not treat generated XML as source-of-truth.

Profile sources live under:

```text
profiles/devices/*.pikl
profiles/kits/*.pikl
```

The current built-in profile pair is:

```bash
polyrhythm profile-inspect --device td50 --kit crocell
polyrhythm map-check --device td50 --kit crocell
polyrhythm generate-midimap --device td50 --kit crocell
```

Generated DrumGizmo midimaps are written under the cache directory:

```text
~/.cache/td50/generated-midimaps/<device>-<kit>.xml
```

The compatibility direction is always:

```text
Pikl device profile + Pikl kit profile -> generated DrumGizmo XML
```

not the reverse.

## Safety constraints

- DRS is the only default kit path.
- Alternate kits are explicit diagnostics only.
- OBS routing is off by default.
- Do not probe broad PipeWire graphs as part of normal control flows.
- Do not restart PipeWire/WirePlumber as part of kit switching. Use `recover-audio` for explicit incident recovery only.
- Block live monitor routing when OBS receives the onboard speaker monitor feed.
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

Current desired baseline is `engine-only`: DrumGizmo running, mapper MIDI linked, and all DrumGizmo audio outputs disconnected. OBS receiving the onboard speaker monitor feed is flagged suspicious because it may participate in feedback loops; `monitor-test --execute` now refuses live speaker routing while that OBS feedback hazard is present.

For the current practice/audition/recording use case, use the balanced monitor preset rather than the full-kit probe:

```bash
polyrhythm monitor-test --pair practice-recording-balanced --volume 50% --execute
```

That preset intentionally keeps the player's speaker feed music-forward by routing only DrumGizmo overheads plus modest kick-front and snare-top support to the onboard monitor. Keep OBS pointed at the dedicated `TD50-OBS-Mix.monitor` source for recording/streaming audio; do not record the physical speaker monitor feed. The OBS/audience mix can carry more drum support than the player monitor without making the live room monitor fight Spotify.

Kit audition policy is conservative: DRS remains the default stable kit, while CrocellKit, MuldjordKit, and Aasimonster are explicit experimental candidates until they have local kit XML, TD-50 midimap coverage, acceptable load/CPU behavior, and a verified channel routing plan. Audition alternate kits inside the same balanced routing topology first; do not change the engine and the routing model at the same time.

## COSMIC desktop audio recovery

If desktop audio loses devices or media clients go silent after an audio-stack incident, do not keep restarting PipeWire by hand. Use the encoded recovery path:

```bash
polyrhythm recover-audio --dry-run   # inspect the actions
polyrhythm recover-audio --execute   # mute/lower first, restart backend, refresh COSMIC/client state
polyrhythm audio-doctor              # verify backend + COSMIC audio applet + onboard defaults
```

On this COSMIC host, healthy `pactl`/`wpctl` output alone is not enough. The COSMIC panel/audio applet and surviving media clients can stay stale after a backend restart.
