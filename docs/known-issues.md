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
