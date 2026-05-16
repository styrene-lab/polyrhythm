# Known issues

## Omegon image viewing on NixOS/COSMIC returns metadata only

Observed on this host while debugging the TD-50/PipeWire graph with qpwgraph screenshots.

### Environment

- OS/session: NixOS + COSMIC/Wayland
- Project path: `/home/wilson/workspace/styrene-lab/polyrhythm`
- Screenshot path tested inside workspace:
  - `Screenshot_2026-05-16_10-13-12.png`
- Screenshot path tested outside workspace:
  - `/home/wilson/Documents/Screenshot_2026-05-16_10-13-12.png`
- ImageMagick can identify the screenshot as valid PNG:
  - `PNG 915x1140`, roughly `200 KB`

### Symptom

Omegon's `view` tool returns only metadata, not rendered visual content:

```text
**/home/wilson/workspace/styrene-lab/polyrhythm/Screenshot_2026-05-16_10-13-12.png** (200.3 KB)
```

Omegon's `read` tool also returns no usable visual content for the PNG.

The operator reports macOS deployments work correctly, so this appears specific to this NixOS/COSMIC deployment or to the Linux image-render/multimodal handoff path.

### What was ruled out

- The file is valid and readable by shell tools.
- The failure is not caused by outside-workspace path restrictions: copying the screenshot into the workspace produced the same metadata-only result.

### Impact

This blocks visual collaboration on qpwgraph/PipeWire screenshots. The agent can launch `qpwgraph`, but cannot inspect screenshots, making graph debugging much slower and forcing textual/manual descriptions.

### Working hypothesis

One of:

1. the NixOS/COSMIC harness deployment lacks image-rendering dependencies;
2. the `view` tool's rendered image payload is not being injected into model context;
3. the Linux/Wayland/COSMIC portal/render bridge is missing or failing silently;
4. this deployment's model/tool path has vision disabled despite exposing `view`.

### Desired behavior

- `view` should either provide rendered image content to the model or return an explicit renderer failure.
- Metadata-only success is misleading; the tool should not appear successful when no visual payload reaches the model.

### Repro sketch

```bash
cd /home/wilson/workspace/styrene-lab/polyrhythm
magick identify Screenshot_2026-05-16_10-13-12.png
# Then call Omegon view on the same path.
# Actual: metadata only.
# Expected: image visible to model.
```

## 2026-05-16 lockup after PipeWire restart: RCU stall in `data-loop.0`, OBS/V4L2 spam

Observed after the operator rebooted a machine that was still moving the mouse but had keyboard input delayed by roughly a minute.

### Environment

- Host/project: `/home/wilson/workspace/styrene-lab/polyrhythm`
- OS/session: NixOS + COSMIC/Wayland
- Kernel shown in previous boot logs: `6.18.23 #1-NixOS PREEMPT_{RT,(lazy)}`
- Previous boot: `461be386695440ccaebc3ce9f11ffee0`, `2026-05-15 21:53:45 EDT` to `2026-05-16 15:17:57 EDT`
- Current boot after reboot: `086a205dd4c54fba912f3aa397bdd246`, from `2026-05-16 15:18:53 EDT`

### What was checked

Commands used were read-only / non-destructive:

```bash
journalctl --list-boots
journalctl -b -1 -p warning..alert --no-pager
journalctl -b -1 --no-pager | grep -Ei 'suspend|resume|sleep|wake|pipewire|wireplumber|drumgizmo|alsa|midi|xrun|rtkit|oom|hung|blocked|cosmic|keyboard|input|watchdog|soft lockup|hard lockup'
journalctl -b -1 --since '2026-05-16 15:00:00' --until '2026-05-16 15:18:00' --no-pager -o short-iso
journalctl -b -1 --since '2026-05-16 14:05:57' --until '2026-05-16 15:17:57' --no-pager
nix develop --command cargo run --bin polyrhythm -- graph-check --state engine-only
pw-link -l
pgrep -af 'drumgizmo|hihat|polyrhythm|pw-link|obs|stream-startup|pipewire|wireplumber'
```

### Evidence found

No suspend/resume evidence was found in the previous boot journal:

- `sleep/resume marker lines: 0`
- no `systemd-sleep`
- no `PM: suspend`
- no `PM: resume`
- no `sleep.target` / `suspend.target`

The failure window shows kernel-level stalls:

```text
2026-05-16T15:14:01-04:00 kernel: rcu: INFO: rcu_preempt self-detected stall on CPU
2026-05-16T15:14:01-04:00 kernel: CPU: 2 UID: 1000 PID: 42694 Comm: data-loop.0 ... PREEMPT_{RT,(lazy)}
2026-05-16T15:16:12-04:00 kernel: rcu: INFO: rcu_preempt self-detected stall on CPU
2026-05-16T15:16:12-04:00 kernel: CPU: 2 UID: 1000 PID: 42694 Comm: data-loop.0 ... PREEMPT_{RT,(lazy)}
```

The same window reports multiple tasks blocked for more than 122 seconds, including:

- `wpa_supplicant`
- `nsncd`
- Chromium `ThreadPoolForeg`
- Chromium `Network Thread`
- `kworker/u64:0`

About an hour earlier, at `2026-05-16T14:05:57-04:00`, the user audio stack was restarted:

```text
Stopping Multimedia Service Session Manager...
Stopping PipeWire PulseAudio...
Stopped PipeWire PulseAudio.
Stopping PipeWire Multimedia Service...
Stopped PipeWire Multimedia Service.
Started PipeWire Multimedia Service.
Started PipeWire PulseAudio.
Started Multimedia Service Session Manager.
```

Immediately after that restart, OBS / `stream-startup.sh` began repeatedly logging V4L2 camera failures:

```text
stream-startup.sh[2516]: error: v4l2-input: /dev/video2: select timed out
stream-startup.sh[2516]: error: v4l2-input: /dev/video2: failed to log status
```

Count from `2026-05-16 14:05:57` through reboot:

```text
v4l2 timeout/log-status lines: 4044
rcu stall lines near failure: 8
hung task lines near failure: 14
sleep/resume marker lines: 0
```

Current post-reboot graph check showed the drum rig was not running and no DrumGizmo audio was connected:

```text
nodes: 9 ports: 19 links: 0
drumgizmo running: false
required midi link: false
DrumGizmo audio connections:
  none
graph-check: failed
  DrumGizmo node is not running
  required MIDI link is missing
```

Current suspect processes after reboot included PipeWire, PipeWire-Pulse, WirePlumber, and OBS, but no DrumGizmo / hihat mapper / polyrhythm engine process.

### Current interpretation

Known from evidence:

- The incident was not accompanied by journaled suspend/resume markers.
- The visible hard failure was a kernel RCU stall involving a realtime-style PipeWire thread name, `data-loop.0`.
- OBS / V4L2 camera logging was pathological after the audio-stack restart and before the lockup.
- Polyrhythm/DrumGizmo was not visible as an active participant in the captured failure evidence.

Suspected, not proven:

- The strongest local suspect is the PipeWire + OBS + V4L2 camera path, especially `/dev/video2`, interacting badly with RT scheduling / kernel locks.
- Sleep/wake may still be relevant as a human-observed timing correlation, but this incident's logs do not support it as the primary mechanism.

### Safety guidance

Do not treat this as a normal polyrhythm drum-graph recovery case.

- Do not restart PipeWire/WirePlumber as a diagnostic action while preserving graph evidence.
- Do not kill DrumGizmo or the mapper unless the operator asks for teardown.
- If the system is still responsive enough during recurrence, prefer bounded journal capture before reboot.

Useful bounded capture during recurrence:

```bash
journalctl -k --since '10 minutes ago' | grep -Ei 'rcu|blocked|hung|data-loop|pipewire|wireplumber|v4l2|video2'
journalctl --user --since '30 minutes ago' | grep -Ei 'pipewire|wireplumber|obs|stream-startup|v4l2|video2'
ps -eLo pid,tid,ppid,cls,rtprio,pri,stat,pcpu,comm,args | grep -E 'pipewire|wireplumber|data-loop|obs' | grep -v grep
```

### Follow-up research targets

- PipeWire `data-loop.0` realtime threads on PREEMPT_RT kernels and RCU stalls.
- OBS V4L2 source `/dev/video*` `select timed out` spam leading to PipeWire or kernel stalls.
- Linux `cfg80211` / `nl80211` / `rtnl` blocked tasks under PREEMPT_RT after userspace realtime stalls.
- COSMIC/Wayland + OBS + camera portal behavior after PipeWire restart.

### Web research notes

Search status on 2026-05-16:

- The built-in web search providers failed due provider parsing / bot-detection errors, so GitHub's public search API was used as a fallback.
- GitHub issue search for exact `"data-loop.0" "rcu_preempt"` found only one unrelated sched-ext report, not a PipeWire-specific known issue.
- GitHub issue search for `"PipeWire" "data-loop.0" "PREEMPT_RT"`, `"rcu_preempt self-detected stall" "pipewire"`, and `"PREEMPT_RT" "PipeWire" "RCU stall"` found no direct matches.
- GitHub issue search for `"v4l2-input" "select timed out" OBS Linux` found multiple matches. The clearest relevant result was:
  - <https://github.com/GloriousEggroll/AVMATRIX-VC12-4K-CAPTURE/issues/15>
  - It reports OBS flooding logs with exactly:
    - `error: v4l2-input: /dev/video0: select timed out`
    - `error: v4l2-input: /dev/video0: failed to log status`
  - The reported mechanism there is a capture-device/driver problem: misreported frame intervals and ineffective `VIDIOC_S_PARM` / `v4l2-ctl --set-parm` behavior.

Interpretation from research:

- I did not find evidence of a known PipeWire `data-loop.0` + PREEMPT_RT + RCU-stall bug matching this incident.
- I did find evidence that OBS `v4l2-input` timeout/status-log flooding is a known failure signature for problematic V4L2 capture devices/drivers or unsupported frame interval negotiation.
- That strengthens the local hypothesis that `/dev/video2` / OBS camera configuration is worth isolating before blaming polyrhythm or sleep/wake.

Useful next checks for `/dev/video2` if this recurs or before the next stream session:

```bash
v4l2-ctl -d /dev/video2 --all
v4l2-ctl -d /dev/video2 --list-formats-ext
journalctl --user --since '30 minutes ago' | grep -Ei 'obs|stream-startup|v4l2|video2|select timed out|failed to log status'
```
