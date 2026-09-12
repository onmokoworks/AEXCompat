# Timed image inputs

The typed image request CLI commands (`--render-experimental-request`,
`--render-experimental-smart-request`, and their supported depth variants)
accept an optional `timed_layers` array alongside `assignments` and `timing`:

```json
{
  "schema_version": 1,
  "assignments": [],
  "timing": {"frame": 30, "fps": 30, "duration_frames": 300},
  "timed_layers": [
    {"slot": 8, "time": 29, "time_scale": 30, "image": "frames/29.png"},
    {"slot": 8, "time": 30, "time_scale": 30, "image": "frames/30.png"}
  ]
}
```

Choose a real layer parameter slot from inspection; the example slot is not
universal. Times are rational values, independent of the current frame. At most
64 samples are accepted, and static plus timed secondary images must also fit
the transport's 64-image limit. Duplicate equivalent slot/time pairs and zero
time scales are rejected. An unprovided time for a timed-only slot is not
silently substituted; an explicitly supplied static layer can act as fallback.

Relative `timed_layers[].image` paths resolve from the request file's directory.
This differs from existing relative `assignments[].layer` bundle paths, which
resolve from the request directory's parent. Absolute image paths are accepted.

Host context, dependency declarations, render settings and CPU selection retain
their existing meaning. Omit the array for ordinary single-image requests.
The Harness GUI also accepts this field through **Load debug request** and
preserves it through **Save debug request**. The imported sample count is shown
with a clear action. Loading an ordinary request replaces/clears prior samples;
changing the selected AEX clears them as well. Both manual rendering and live
updates use the timed one-shot path, not the static resident-session path.
Automatic dependency search roots, host context and custom UI actions are retained.
Audio requests and declarative render fixtures do not accept timed samples.

Supplying samples does not prove an effect rendered correctly. Verify decoded
output and the effect's response; sample delivery alone is not AE equivalence.
