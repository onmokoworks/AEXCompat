# AE Snapshot Schema - 2026-05-31

Purpose: define the first clean `AviUtlas -> After Effects` export lane before
any implementation of `export-ae-jsx`.

Status note: after this schema was written, the user clarified that direct
`.aex` hosting and external `.aep` / `.aepx` editing are now explicit goals.
This schema remains the fallback/validation route for generated JSX and
AE-mediated project saving; see
`analysis/AEX_DIRECT_HOST_AND_AEP_EDIT_STRATEGY_2026-05-31.md` for the newer
direct-host/editing strategy. The external project-editing contract is now
split into `analysis/AEPX_JSX_PATCH_TOOL_SPEC_2026-05-31.md`,
`analysis/AEPX_PATCH_REQUEST_SCHEMA_2026-05-31.json`, and
`analysis/JSX_TRANSACTION_REQUEST_SCHEMA_2026-05-31.json`.

The preferred route is:

1. AviUtlas parses or evaluates a project into an independent snapshot JSON.
2. AviUtlas generates a JSX importer that consumes that snapshot.
3. After Effects creates/saves the `.aep` project itself.

This keeps AviUtlas away from direct `.aep` parsing/generation and direct `.ffx`
binary parsing.

## Cleanroom Status

- Observed: local AE experiments already use JSX/AEGP/Electron and named-pipe
  controller patterns.
- Observed: user `.ffx`, `.aep`, and `.aex` assets exist locally, but remain
  private fixtures.
- Inferred: generated snapshot JSON plus generated JSX is the lowest-risk public
  path for AE export.
- Unverified: visual fidelity for ExEdit effects represented by AE native
  effects, third-party presets, or placeholders.

## Top-Level Shape

```json
{
  "schema_version": 1,
  "generator": {
    "name": "AviUtlas",
    "version": "unreleased",
    "generated_at": "YYYY-MM-DDTHH:MM:SSZ"
  },
  "source": {
    "kind": "exo",
    "path_hint": "verification/aviutl110_bk_copy/script/93/aup_exo/light_leaks.exo",
    "publication_status": "local-only"
  },
  "project": {
    "name": "light_leaks",
    "frame_rate": 30.0,
    "width": 1280,
    "height": 720,
    "duration_frames": 300,
    "audio_sample_rate": 44100
  },
  "assets": [],
  "comps": [],
  "warnings": []
}
```

## Source Metadata

| Field | Type | Meaning |
| --- | --- | --- |
| `source.kind` | string | `exo`, `exa`, `aup`, `aup2`, `aviutlas`, or `synthetic` |
| `source.path_hint` | string/null | Local path hint for traceability; never required by generated JSX |
| `source.publication_status` | string | `public-candidate`, `local-only`, or `unknown-license` |
| `source.cleanroom_notes` | array | Optional observed/inferred/unverified notes |

## Assets

Assets are references, not copied file payloads.

```json
{
  "id": "asset_001",
  "kind": "image",
  "path": "relative/or/user/supplied/path.png",
  "missing_policy": "placeholder",
  "publication_status": "local-only"
}
```

Allowed `kind` values:

- `image`
- `video`
- `audio`
- `sequence`
- `solid`
- `text-placeholder`
- `unknown`

## Compositions

```json
{
  "id": "comp_main",
  "name": "Main",
  "width": 1280,
  "height": 720,
  "frame_rate": 30.0,
  "duration_frames": 300,
  "background": [0, 0, 0, 0],
  "layers": []
}
```

## Layers

```json
{
  "id": "layer_001",
  "name": "Text 1",
  "kind": "text",
  "source_ref": null,
  "start_frame": 1,
  "end_frame": 120,
  "enabled": true,
  "blend": {
    "aviutl_number": 0,
    "name": "normal",
    "ae_mode": "normal",
    "fidelity": "mapped"
  },
  "transform": {},
  "content": {},
  "effects": [],
  "markers": [],
  "warnings": []
}
```

Allowed `kind` values:

- `text`
- `shape`
- `media`
- `solid`
- `camera`
- `null`
- `placeholder`

## Transform

```json
{
  "position": {
    "x": 0.0,
    "y": 0.0,
    "z": 0.0
  },
  "anchor": {
    "x": 0.0,
    "y": 0.0,
    "z": 0.0
  },
  "scale": {
    "x": 100.0,
    "y": 100.0,
    "z": 100.0
  },
  "rotation": {
    "x": 0.0,
    "y": 0.0,
    "z": 0.0
  },
  "opacity": 100.0,
  "keyframes": []
}
```

Keyframes use frame numbers as the canonical interchange unit:

```json
{
  "property": "transform.position.x",
  "frames": [
    {
      "frame": 1,
      "value": 0.0,
      "interpolation": "hold",
      "source": "observed"
    }
  ]
}
```

### Timing Contract

- Snapshot frames are one-based.
- Generated JSX should convert frame numbers with
  `seconds = (frame - 1) / comp.frame_rate`, so snapshot frame `1` maps to AE
  time `0.0`.
- `start_frame` and `end_frame` are inclusive in the snapshot. JSX should set a
  layer's `inPoint` from `(start_frame - 1) / frame_rate` and `outPoint` from
  `end_frame / frame_rate`.
- `duration_frames` is the frame count, not the final one-based frame number.

## Text Content

```json
{
  "text": "sample",
  "font": {
    "family": "MS Gothic",
    "size": 48.0,
    "style": "regular"
  },
  "fill": [255, 255, 255, 255],
  "stroke": {
    "enabled": false,
    "color": [0, 0, 0, 255],
    "width": 0.0
  },
  "paragraph": {
    "align": "left"
  }
}
```

## Shape Content

```json
{
  "shape_kind": "rectangle",
  "size": {
    "x": 100.0,
    "y": 100.0
  },
  "fill": [255, 255, 255, 255],
  "stroke": {
    "enabled": false,
    "color": [0, 0, 0, 255],
    "width": 0.0
  }
}
```

Allowed `shape_kind` values:

- `background`
- `ellipse`
- `rectangle`
- `triangle`
- `polygon`
- `star`
- `custom`
- `placeholder`

## Effects

Effects must state fidelity explicitly.

```json
{
  "id": "effect_001",
  "source_name": "ぼかし",
  "source_kind": "exedit-filter",
  "target": "layer",
  "fidelity": "approximate",
  "ae_strategy": "native-effect",
  "params": {
    "範囲": 8.0
  },
  "warnings": [
    "Native oracle reference frame not yet available"
  ]
}
```

Allowed `fidelity` values:

- `mapped`
- `approximate`
- `placeholder`
- `opaque`
- `blocked`

Allowed `ae_strategy` values:

- `native-effect`
- `expression`
- `preset-ref`
- `precompose`
- `placeholder-layer`
- `metadata-only`

## Opaque Preset Reference

`.ffx` files are not parsed by AviUtlas. They can be represented only as
user-supplied references to apply inside AE.

```json
{
  "id": "effect_preset_001",
  "source_name": "user preset",
  "source_kind": "ffx",
  "fidelity": "opaque",
  "ae_strategy": "preset-ref",
  "preset_ref": {
    "path": "C:/Users/Example/Documents/Adobe/After Effects 2025/User Presets/example.ffx",
    "apply_timing": "after-layer-create",
    "user_supplied": true
  },
  "warnings": [
    "Preset content is opaque and local-only"
  ]
}
```

If `preset_ref.user_supplied == true` but `preset_ref.path == null`, the first
JSX export slice should skip applying the preset and emit a non-fatal warning.
File-picker behavior can be added later behind an explicit opt-in.

Implementation status (2026-06-01): `export_ae_jsx` now validates `.ffx`
references fail-closed before JSX generation. `source_kind: "ffx"` requires
`ae_strategy: "preset-ref"`, `fidelity: "opaque"`, a `preset_ref` object,
`preset_ref.user_supplied: true`, and `preset_ref.path` as either `null` or a
string. `null` path remains a non-fatal skipped-preset warning; malformed
`preset_ref` data is fatal.

The same exporter also validates fields it consumes while generating JSX:
RGBA arrays must have four integer channels in `0..=255`; transform
`position`/`anchor`/`scale`/`rotation` vectors must expose numeric `x/y/z`;
opacity and keyframe values must be numeric; keyframe and marker frames must be
positive; and keyframe interpolation must be one of `hold`, `linear`, or
`bezier`.

Additional implementation status (2026-06-01): fields consumed by layer
creation now fail closed as well. `layer.enabled`, when present, must be a
boolean; `layer.blend`, when present, must be an object; and the first exporter
slice accepts only the currently emitted AE blend modes `normal` and `add` via
`layer.blend.ae_mode`. Unsupported blend modes remain schema errors instead of
being silently downgraded to normal.

The exporter also fail-closes optional text/shape fields consumed by the emitted
JSX: `text.content.font`, when present, must be an object with non-empty
`family` and positive `size`, and `text.content.stroke.enabled` /
`shape.content.stroke.enabled`, when present, must be boolean.
Optional marker text consumed by JSX is guarded the same way:
`marker.name` and `marker.comment`, when present, must be non-empty strings
before generating `MarkerValue` text or comments.

Generated JSX now applies scalar `transform.opacity` and
`transform.rotation.z` keyframes directly. It also applies axis keyframes for
`transform.position.x/y/z`, `transform.anchor.x/y/z`, and
`transform.scale.x/y/z` by cloning the current AE vector value and replacing the
target axis before `setValueAtTime`. Unsupported transform keyframe properties
remain fatal at generation time rather than being silently ignored. For emitted
keyframes, `hold`, `linear`, and `bezier` interpolation names are mapped to AE
`KeyframeInterpolationType` values and applied with `setInterpolationTypeAtKey`;
this is a host-script contract check, not an AE-render parity claim.

Generated JSX must be UTF-8 and must escape all embedded JavaScript /
ExtendScript string literals, including paths, quotes, backslashes, newlines,
and Unicode separator characters. Prefer serializing snapshot data with a JSON
encoder rather than hand-built string literals.

Warnings should be collected into an import summary and/or written to a
dedicated warnings layer/marker. Placeholder, opaque-preset, and oracle-blocked
warnings are non-fatal; schema/version errors are fatal. Avoid modal `alert()`
spam in the first exporter.

## JSX Importer Responsibilities

The generated JSX should:

1. Validate `schema_version`.
2. Create the main comp and nested comps.
3. Create solids, text layers, shape layers, and media layers.
4. Apply transforms and frame-based keyframes.
5. Apply mapped AE native effects where `fidelity` allows.
6. Apply `.ffx` only when `preset_ref.user_supplied == true` and
   `preset_ref.path` is non-null.
7. Skip null-path `.ffx` references with a non-fatal warning.
8. Emit warnings for placeholders, blocked effects, and layer kinds represented
   as null placeholders in the first exporter slice.
9. Never call shell/system commands unless the user explicitly opts in.

## Non-Goals For The First Export

- Direct `.aep` parsing or binary generation.
- Direct `.ffx` parsing.
- Loading `exedit.auf` inside AE.
- Claiming ExEdit pixel parity for AE-native approximations.
- Copying local user presets/projects into generated artifacts.

Implementation verification (2026-06-02): the first `snapshot JSON -> JSX`
slice is implemented by `aviutl-rs/examples/export_ae_jsx.rs` and is covered by
targeted tests for the synthetic snapshot schema, exporter contract, and media
import boundary. Verified commands:
`cargo test --test ae_snapshot_schema --no-default-features`,
`cargo test --example export_ae_jsx --no-default-features`, and
`cargo test --test ae_jsx_export_media_contract --no-default-features`.
This remains a generated-JSX contract claim only; AE launch, JSX execution,
project save, `.aep/.aepx` mutation, `.ffx` parsing, `.aex` loading, media copy,
and render parity are still non-goals or manual/oracle-gated work.

## First Implementation Slice Checklist

1. Use `analysis/AE_SNAPSHOT_EXAMPLE_2026-05-31.json` as the seed fixture. It
   contains one text layer, one shape layer, one placeholder effect, and one
   opaque `.ffx` preset reference with no user media or Adobe binary payload.
2. Keep `aviutl-rs/tests/ae_snapshot_schema.rs` green while evolving the
   interchange contract.
3. Generate a JSX file from that snapshot without reading AE binaries or user
   projects.
4. Keep runtime AE execution as an explicit manual smoke step until the local
   AEGP/JSX security boundary is documented.
