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
