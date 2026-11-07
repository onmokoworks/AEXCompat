# Declarative render fixtures

`aexcompat-harness --headless --render-fixture <aex> <fixture.json> <output-directory>`
runs one classic or SmartFX render and atomically publishes the final raw/EXR artifact plus
the requested native-world checkpoints. The AEX path and its observed hash stay outside the
fixture. All image paths in the fixture are traversal-free paths relative to `fixture.json`.
The same command and document contract are supported by the Windows harness and by the
Apple Silicon macOS harness; macOS executes the x86-64 AEX through the resident guest worker.
On macOS, input and secondary-layer checkpoints are read back from the worlds materialized in
guest memory after the render rather than reconstructed from the source images on the host.

The v1 document is strict: unknown top-level, timing, or checkpoint fields are rejected.
`parameters` uses the complete parameter records returned by `--inspect-experimental`; a
layer parameter's `layer_path` is also relative to the fixture. This permits scalar, choice,
color, angle, 2D/3D point, primary-layer, and secondary-layer cases without a second sidecar.

Both v1 and v2 accept an optional top-level `mask_scene`, using the same strict
`MaskScene` contract as experimental requests' `host_context.mask_scene`:

```json
"mask_scene": {"masks": [{"open": false, "vertices": [
  {"x": 16, "y": 16, "tangent_out": {"x": 0, "y": 0}},
  {"x": 80, "y": 16}, {"x": 80, "y": 48}, {"x": 16, "y": 48}
]}]}
```

Each vertex has `x`/`y` and optional `tangent_in`/`tangent_out` objects with
`x`/`y` offsets (omitted tangents are zero). The shared request validator allows
at most 8 masks, 2..64 vertices per open path or 3..64 per closed path, and at
most 128 vertices in total. Coordinates and tangent components must be finite
and within -32768..32768; the encoded transport is limited to 8192 bytes.
Unknown mask, vertex, and tangent fields are rejected. An empty `masks` array
is valid. Masks and `parameters[].layer_path` images can be used together on
Windows classic and SmartFX paths at argb8, argb16, and argb32f.

The fixture digest remains SHA-256 of the original JSON bytes, so mask data is
included without changing existing fixture digests. Mask-bearing v1 fixtures
also bind the fixture case identity into final artifact comparison metadata;
v2 already does so for every case. Omitting `mask_scene` preserves the previous
rendering and metadata behavior. No broader `host_context` fields are exposed
by this fixture addition.

```json
{
  "schema": "aexcompat.render_fixture",
  "schema_version": 1,
  "primary_layer": "primary.png",
  "parameters": [],
  "pixel_format": "argb32f",
  "render_path": "smart",
  "premultiplication": "straight",
  "timing": { "current_time": 0, "time_step": 1, "total_time": 1, "time_scale": 1 },
  "final_artifact": "exr",
  "checkpoints": [
    { "id": "input_world", "stage": "smart-input" },
    { "id": "output_world", "stage": "smart-output" }
  ]
}
```

Allowed stages are `<classic|smart>-input`, `<classic|smart>-output`, and
`<classic|smart>-layer-slotN`. Every selected checkpoint is written beneath
`checkpoints/<id>/` as `output.bin` plus strict `output.json`. The raw file is packed ARGB,
has no row padding, and preserves native component words: PF8 bytes, PF16 AE-range integer
words (0..32768), or PF32 IEEE-754 words. Thus PF16 raw remains the byte-exact authority;
it is never promoted to float EXR.

The final `exr` option is accepted only for PF32 and retains the existing uncompressed
scanline FLOAT32, Preserve RGB, explicit premultiplication, working-space `None`, software
render contract. Checkpoint metadata uses `aexcompat.render_raw` schema v2 and adds
`checkpoint_identity` (id, stage, and fixture digest) and binds the same object into
`comparison_identity`; all ordinary raw/EXR artifacts retain their exact v1 schema.

The requested output directory is renamed into place only after the render and every
checkpoint validate and commit successfully. A render failure or missing checkpoint removes
the staged fixture output, so a partial set cannot be mistaken for success.
