# COSMIC audio recovery

`polyrhythm recover-audio` is the only supported command for deliberate desktop audio-stack recovery on this host.

## Why this exists

On NixOS/COSMIC, restarting `pipewire`, `pipewire-pulse`, and `wireplumber` can leave the desktop control/UI layer stale even when `pactl` and `wpctl` look healthy. The confirmed failure mode is COSMIC reporting no usable output device, or surviving media clients staying silent, until the COSMIC panel/audio applet and clients are refreshed.

Recovery must treat the system as three coupled layers:

1. backend audio graph: `pipewire`, `pipewire-pulse`, `wireplumber`;
2. desktop control/UI layer: `cosmic-panel`, `cosmic-applet-audio`;
3. audio clients: Spotify and drum clients/routes.

## Commands

Dry-run first:

```bash
polyrhythm recover-audio --dry-run
```

Execute recovery:

```bash
polyrhythm recover-audio --execute
```

Verify afterward:

```bash
polyrhythm audio-doctor
```

## Encoded recovery sequence

`recover-audio --execute`:

1. sets the onboard analog sink to `1%` and mutes it;
2. clears TD-50 monitor routing and stops known TD-50 clients;
3. restarts `wireplumber`, `pipewire`, and `pipewire-pulse`;
4. restores the onboard analog card profile, default sink/source, line-out port, unmute state, and safe volume;
5. optionally restarts Spotify;
6. optionally restarts `cosmic-panel`, causing `cosmic-applet-audio` to respawn;
7. runs `audio-doctor` when invoked through the CLI execute path.

Default local targets:

```text
sink:   alsa_output.pci-0000_0e_00.4.analog-stereo
source: alsa_input.pci-0000_0e_00.4.analog-stereo
card:   alsa_card.pci-0000_0e_00.4
port:   analog-output-lineout
```

## Safety guardrail

Normal drum controls must not restart PipeWire/WirePlumber. Use `recover-audio` only when the operator explicitly chooses incident recovery.

Live monitor routing is also guarded: `monitor-test --execute` refuses to add DrumGizmo speaker routes while OBS receives the onboard speaker monitor feed, because that graph shape can participate in feedback loops.
