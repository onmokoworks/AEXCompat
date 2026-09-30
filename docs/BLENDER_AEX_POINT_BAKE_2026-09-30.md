# Two-component AEX point editing in Blender on macOS (#1697)

The Blender descriptor now lists enabled, visible AEX `point` parameters with exactly two finite components. Refresh the AEX parameter list, add the named point, edit **Point (X, Y)**, then bake. The saved `.blend` retains the selected coordinates. The point is sent as a typed `components: [x, y]` override through the existing macOS fixture and guest transport; the response records the applied components and plug-in identity. Coordinates are raw AEX point components, with no assumed conversion to pixels or AE spatial semantics. The separate AE oracle question remains #923.

The local Blender 4.5.8 smoke uses license-free `OLMRadialBlur.aex` on its SmartFX path. It selects `Center` (slot 2) and `Strength` (slot 4). With Strength 500, changing Center from (50, 50) to (20, 80) changes 8,451 output bytes on a 64×64 RGBA8 gradient. The selected point bake matches a direct request with the same two overrides. It verifies malformed requests and forged applied values are rejected, renders the packed result through Blender's native compositor, saves the `.blend`, and reopens it to verify selected coordinates and packed image hashes. The `classic` route produced the same output for all tested point/strength values, so this evidence is specifically for SmartFX. It does not establish After Effects pixel parity.

Run from the repository root with locally built Release binaries and the installed OLM directory:

```sh
AEXCOMPAT_PLUGIN_ROOT=/path/to/OLM \
AEXCOMPAT_HARNESS="$PWD/broker/target/release/aexcompat-harness" \
AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker" \
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup -t 2 \
  --python tools/blender_aexcompat_point_parameter_smoke.py -- --output-dir target/issue1697/blender

/Applications/Blender.app/Contents/MacOS/Blender --background -t 2 \
  target/issue1697/blender/point-parameter-bake.blend \
  --python tools/blender_aexcompat_point_parameter_smoke.py -- \
  --output-dir target/issue1697/blender --reload
```

The output directory is ignored by Git. The evidence packet records plug-in, worker, and harness hashes; no plug-in bytes or private paths are committed. After Effects is not launched.
