# TD-50 canonical capture notes

Polyrhythm treats Pikl device profiles as canonical. DrumGizmo XML midimaps are generated compatibility artifacts and cannot express every TD-50 gesture by themselves.

This document records observed TD-50 raw MIDI facts from interactive captures on this rig. The goal is to make `profiles/devices/td50.pikl` empirical rather than assumption-driven.

## Capture method

Raw MIDI was captured from the TD-50/U2MIDI ALSA input:

```bash
aseqdump -p 32:0
```

`Active Sensing` events were filtered out for analysis.

## Confirmed pieces so far

### Kick

Sequence: three clean kick hits.

Observed:

```text
note 36, velocity 91
note 36, velocity 94
note 36, velocity 90
note-off velocity 64
channel 9
```

Canonical fact:

```text
kick.main -> note 36
confidence: high
```

`note 35` remains an unverified compatibility/alternate kick note until observed.

### Snare head

Sequence: solid snare head hits.

Observed:

```text
note 38
velocities observed: 56, 57, 58, 59, 60, 61, 68, 70, 75, 76, 80, 81, 83, 84, 87, 94
note-off velocity 64
channel 9
CC16 varied across head position/strike context
CC88 alternated 0/64
```

Canonical fact:

```text
snare.head -> note 38
confidence: high
```

### Snare rim / rimshot

Sequences: solid rim hits and full rimshot hits.

Observed rim/rimshot events:

```text
note 40
rim-like velocities: ~55-90
full rimshot velocities: ~97-127
note-off velocity 64
CC16 generally high, often 118-127 for strong rim/rimshot events
CC88 varied, usually 0 or 64 with occasional low values
```

Canonical facts:

```text
snare.rimshot/full -> note 40, high velocity, usually high CC16
snare.rim/solid rim -> note 40, medium velocity, high CC16
confidence: high for note 40, medium for separating rim vs rimshot by predicates
```

### Snare cross-stick / side-stick

Sequence: soft cross-stick hits.

Observed:

```text
note 38
low velocities: approximately 8-40
CC16 mostly 0, with occasional small/medium values
CC88 varied 0/64
```

Canonical fact:

```text
snare.crossstick_soft -> note 38 + low velocity predicate
confidence: medium
```

Important implication:

```text
cross-stick is not distinguishable from snare head by note number alone in this TD-50 setup.
```

Static DrumGizmo XML midimaps cannot express this distinction. It requires Pikl predicate support and runtime mapper/event-transform support.

`note 37` has not been observed during snare captures and should not be treated as authoritative for this rig unless later captured.

### Tom 1

Sequence: tom 1 head, then tom 1 rim.

Observed head:

```text
note 48
velocities: 119, 127, 127
CC18: 0
```

Observed rim:

```text
note 50
velocities: 67, 70, 71
CC18: 0
```

Canonical facts:

```text
tom.1.head -> note 48
tom.1.rim -> note 50
confidence: high
```

### Tom 2

Sequence: tom 2 head, then tom 2 rim.

Observed head:

```text
note 45
velocities: 107, 116, 127
CC18: 0
```

Observed rim:

```text
note 47
velocities: 73, 84, 88
CC18: 127
```

Canonical facts:

```text
tom.2.head -> note 45
tom.2.rim -> note 47, CC18 127
confidence: high
```

### Tom 3

Sequence: tom 3 head, then tom 3 rim.

Observed head:

```text
note 43
velocities: 114, 117, 119
CC18: 0
```

Observed rim:

```text
note 58
velocities: 82, 86, 91
CC18: 0, 6, 8
```

Canonical facts:

```text
tom.3.head -> note 43
tom.3.rim -> note 58
confidence: high
```

## Hi-hat capture

Sequence:

```text
1. closed bow
2. closed edge
3. open bow
4. open edge
5. foot chick
6. pedal movement, including partial open/close movement without fully closing
```

### Observed notes

Closed bow:

```text
CC4: 90
note 46
velocities: 90, 101, 118
```

Closed edge:

```text
CC4: 90
note 26
velocities: 51, 60, 63
```

Open bow:

```text
CC4: 0
note 46
velocities: 79, 82, 83
```

Open edge:

```text
CC4: 0
note 26
velocities: 48, 59, 61
```

Foot chick:

```text
note 44
velocities: 98, 121, 121
CC4 moves toward 90 around the chick
```

Pedal movement:

```text
CC4 sweeps between 0 and 90
```

Observed sweep examples:

```text
83, 73, 59, 43, 27, 11, 0
16, 33, 49, 67, 78, 90
```

### Hi-hat orientation

For this TD-50/VH setup, CC4 appears inverted relative to the common assumption that higher means more open:

```text
CC4 ~= 90 -> closed
CC4 ~= 0  -> open
```

Canonical facts:

```text
hihat.bow -> note 46 + CC4 openness predicate
hihat.edge -> note 26 + CC4 openness predicate
hihat.pedal_chick -> note 44
CC4 closed ~= 90
CC4 open ~= 0
confidence: high
```

### Hi-hat implication

The current note-only profile model is insufficient for authoritative hi-hat mapping.

Current assumptions such as:

```text
hihat.closed -> note 42
hihat.semi_open -> note 80
hihat.open -> note 46
```

are not authoritative for this rig. During the labeled capture:

```text
note 42 was not observed
note 80 was not observed
note 46 represented bow hits across openness states
note 26 represented edge hits across openness states
note 44 represented pedal chick
```

A correct TD-50 profile needs predicates such as:

```text
input hihat_bow {
  intent hihat.bow
  notes [46]
  cc4_open 0
  cc4_closed 90
}

input hihat_edge {
  intent hihat.edge
  notes [26]
  cc4_open 0
  cc4_closed 90
}

input hihat_pedal_chick {
  intent hihat.pedal_chick
  notes [44]
}
```

The runtime mapper must bucket CC4 into open/semi-open/closed and emit kit-specific target notes. Static DrumGizmo XML midimaps alone cannot express this correctly.

## Ride capture

Sequence:

```text
1. ride bow
2. ride edge
3. ride bell
4. ride bow chokes
5. ride edge chokes
```

### Observed notes

Ride bow:

```text
note 51
velocities: 61, 66, 75
polyphonic aftertouch note 51 value 0 before hits
CC17 varied: 33, 53, 55
CC88: 0 or 64
```

Canonical fact:

```text
ride.bow -> note 51
confidence: high
```

Ride edge:

```text
note 59
velocity observed: 45
```

A mixed/possibly secondary edge-zone event was also observed:

```text
note 51 velocity 34
CC18 127
note 58 velocity 26
```

For now, the primary authoritative edge fact is:

```text
ride.edge -> note 59
confidence: high
```

The `note 51 + note 58 + CC18 127` pattern needs more targeted capture before it should change the canonical profile.

Ride bell:

```text
note 53
velocities: 29, 30, 36
CC88: 0 or 64
polyphonic aftertouch note 53 value 0 before hits
```

Canonical fact:

```text
ride.bell -> note 53
confidence: high
```

### Ride choke behavior

Ride bow choke and ride edge choke did not emit a separate choke note. They emitted polyphonic aftertouch sweeps across the ride note group.

Observed bow choke pattern after a bow hit:

```text
note 51 velocity 50
polyphonic aftertouch on notes 51, 59, 53:
20, 37, 54, 72, 92, 113, 127, then falling/release values
```

Observed edge choke pattern after edge hits:

```text
note 59 velocity 69
polyphonic aftertouch on notes 51, 59, 53 up to 127 and back down

note 59 velocity 54
polyphonic aftertouch on notes 51, 59, 53 up to 127 and back down
```

Canonical facts:

```text
ride.choke -> polyphonic aftertouch over ride note group [51, 59, 53]
ride.choke pressure range observed: low values through 127
confidence: high
```

### Ride predicate requirements

A note-only mapping can express bow/edge/bell hits:

```text
ride.bow -> note 51
ride.edge -> note 59
ride.bell -> note 53
```

It cannot express ride choke.

A correct TD-50 profile needs an aftertouch predicate such as:

```text
input ride_choke {
  intent ride.choke
  aftertouch_notes [51, 59, 53]
  aftertouch_min 20
  aftertouch_max 127
}
```

The exact threshold should remain tunable. The capture proves the mechanism, not the final cutoff. The runtime mapper must consume polyphonic aftertouch and translate it into a kit-specific choke/stop behavior when the target kit supports one.

Static DrumGizmo XML midimaps cannot represent this ride choke behavior by themselves.

## Crash 1 capture

Sequence performed:

```text
1. crash 1 bow
2. crash 1 edge
3. crash 1 bell
4. crash 1 choke
```

Crash 1 is not a simple single-note cymbal in this TD-50 setup. It emits compound note patterns plus CC/aftertouch context.

### Crash 1 bow

Observed bow cluster:

```text
CC18: 54, 48, 61
note 27 velocities: 121, 127, 127
note 49 velocities: 122, 127, 127
```

Canonical fact:

```text
crash.1.bow -> compound notes [27, 49] with CC18 midrange context
confidence: high for note pair, medium for CC18 range
```

### Crash 1 edge

Observed edge cluster:

```text
CC18: 127, 127, 0
note 27 velocities: 83, 83, 74
note 55 velocities: 66, 68, 59
```

Canonical fact:

```text
crash.1.edge -> compound notes [27, 55], often with high CC18
confidence: high for note pair, medium for CC18 range
```

### Crash 1 bell

Observed bell cluster:

```text
note 28 velocities: 27, 32, 29
note 49 velocities: 42, 44, 43
```

Canonical fact:

```text
crash.1.bell -> compound notes [28, 49]
confidence: high
```

### Crash 1 choke

Observed choke attempts:

```text
note 55 + note 27 hit events
polyphonic aftertouch note 49 value 127
polyphonic aftertouch note 55 value 127
release back to 0
```

Canonical fact:

```text
crash.1.choke -> polyphonic aftertouch over notes [49, 55], high value up to 127
confidence: high
```

### Crash 1 predicate requirements

Crash 1 cannot be represented authoritatively with note-only DrumGizmo XML.

A correct TD-50 Pikl profile needs compound-event predicates such as:

```text
input crash1_bow {
  intent crash.1.bow
  notes [27, 49]
  cc18_range 48..61
}

input crash1_edge {
  intent crash.1.edge
  notes [27, 55]
  cc18_high true
}

input crash1_bell {
  intent crash.1.bell
  notes [28, 49]
}

input crash1_choke {
  intent crash.1.choke
  aftertouch_notes [49, 55]
  aftertouch_min 100
}
```

The exact CC18/aftertouch thresholds should remain tunable. The capture proves the mechanism and required predicate class, not the final cutoff.

The current note-only `profiles/devices/td50.pikl` Crash 1 entries are provisional compatibility approximations until compound predicates are implemented.

## Current design implications

1. Pikl must support event predicates beyond `notes [...]`:
   - velocity ranges
   - CC predicates/ranges
   - polyphonic aftertouch/choke predicates
   - compound note patterns for multi-zone cymbals
2. The runtime mapper must consume canonical Pikl profiles, not only generated DrumGizmo XML.
3. Generated XML remains useful as a compatibility artifact, but it cannot be authoritative for hi-hat, cross-stick, ride choke, or Crash 1 compound-zone behavior.
4. `profiles/devices/td50.pikl` should not claim note 37, 42, or 80 as authoritative until those notes are observed or explicitly marked as compatibility fallbacks.

## Crash 2 capture

Sequence performed:

```text
1. crash 2 bow
2. crash 2 edge
3. crash 2 choke
```

Crash 2 is simpler than Crash 1 and behaves like a dual-zone cymbal plus aftertouch choke.

### Crash 2 bow

Observed:

```text
note 57
velocities: 85, 99, 114
polyphonic aftertouch note 57 value 0 before hits
note-off velocity 64
```

Canonical fact:

```text
crash.2.bow -> note 57
confidence: high
```

### Crash 2 edge

Observed:

```text
note 52
velocities: 50, 66, 73, 93, 94
polyphonic aftertouch note 52 value 0 before hits
note-off velocity 64
```

Canonical fact:

```text
crash.2.edge -> note 52
confidence: high
```

This explains the earlier right-crash-edge issue: the note-only TD-50 profile had not mapped `note 52` as `crash.2.edge`.

### Crash 2 choke

Observed:

```text
polyphonic aftertouch note 57 value 127
polyphonic aftertouch note 52 value 127
release to 0
```

Canonical fact:

```text
crash.2.choke -> polyphonic aftertouch over [57, 52], high value up to 127
confidence: high
```

No separate choke note was observed. The earlier note-only assumption `crash.2.choke -> note 75` is not supported by this capture and should not be treated as authoritative for this rig.

### Crash 2 predicate requirements

A note-only generated XML map can represent:

```text
crash.2.bow -> note 57
crash.2.edge -> note 52
```

It cannot represent crash choke. A correct TD-50 Pikl profile needs an aftertouch predicate such as:

```text
input crash2_choke {
  intent crash.2.choke
  aftertouch_notes [57, 52]
  aftertouch_min 100
}
```

The runtime mapper must translate this aftertouch predicate into kit-specific choke/stop behavior where supported.

## Registration mapping inventory status

The known physical registration mappings captured so far:

```text
kick.main        -> note 36
snare.head       -> note 38
snare.rimshot    -> note 40
snare.crossstick -> note 38 + low velocity predicate, not note-distinct
tom.1.head       -> note 48
tom.1.rim        -> note 50
tom.2.head       -> note 45
tom.2.rim        -> note 47
tom.3.head       -> note 43
tom.3.rim        -> note 58
hihat.bow        -> note 46 + CC4 predicate
hihat.edge       -> note 26 + CC4 predicate
hihat.pedal      -> note 44
ride.bow         -> note 51
ride.edge        -> note 59
ride.bell        -> note 53
ride.choke       -> aftertouch over [51, 59, 53]
crash.1.bow      -> compound [27, 49] + CC18 predicate
crash.1.edge     -> compound [27, 55] + CC18 predicate
crash.1.bell     -> compound [28, 49]
crash.1.choke    -> aftertouch over [49, 55]
crash.2.bow      -> note 57
crash.2.edge     -> note 52
crash.2.choke    -> aftertouch over [57, 52]
```

Open caveats:

```text
note 37 not observed
note 42 not observed in hi-hat capture
note 80 not observed in hi-hat capture
note 75 not observed as crash 2 choke
```

Those notes should remain compatibility fallbacks or be removed from authoritative TD-50 facts unless future captures observe them.
