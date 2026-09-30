# Blender scalar parameter bake on macOS (#1658)

The Blender addon can bake one frame with one changed numeric AEX parameter. In its descriptor node, select **Render AEX (macOS)**, choose **Classic** or **SmartFX**, set **Parameter number** to the slot reported by the headless AEX description, and enter **Parameter value**. Slot 0 keeps the plug-in defaults. The bake operator copies these settings from the node. The addon uses Blender's public Python API and writes a packed Image into the native compositor graph; the custom Python node itself is not a compositor executor.

For an override, the bundled wrapper calls the harness `--describe-aex` command, selects one canonical parameter record by slot, validates its scalar kind and value, and passes that record to the existing render fixture. It records the description SHA-256, the staged AEX SHA-256, and whether that identity matches the source observed for rendering. Identity changes are recorded, not used as a plug-in launch gate. The success packet reports the render path and applied slot/value; an unknown slot, out-of-range value, non-scalar kind, failed description, or malformed description does not produce a rendered response. The no-override Classic behavior remains available.

The macOS smoke uses the license-free OLM `OLMToonDilate.aex` (SHA-256 `c05db8c118029ff3216d3cae8e6423e2eb41ca8f56de2fb3668db81b9b8c32b3`) with a 64×64 straight-alpha checker input (RGBA8 SHA-256 `b8ea23bff74bcb44f5457bc9f7e5831558803db1ea65cf995d9c0e033d2b434b`). SmartFX with default `Search Radius=2` produced packed RGBA8 SHA-256 `cc8e7869f4cb95428fbc592c22ca251200e76aa8c6a9f38a536d8dc9ac98587e`; slot 1 set to 80 produced `0bcc07de1631aa0395013f35790f719bd754f61e4bb2847a86fdc2365d3d8d63`. The two images differ in 1,296 bytes. Blender 4.5.8 LTS packed both results, rendered a 64×64 native compositor PNG from the changed image, and saved a `.blend` file. Reload confirmed both baked images, the packed source image, and the node's SmartFX/slot/value settings. The description and render identities matched; source, harness (`161b7c78eea1ba384cefef5867ea5d39bf2b9602781e038760d18973d41cd23a`), and worker (`fd5ac922b4ec1be898750c0249fbbc68ada80a6f52a454c93b8b13d15918df86`) hashes were stable across the run. After Effects was not started, and neither the AEX nor the local `.blend`/images are committed.

Run the smoke with the local Release harness/guest worker and a legally held OLM AEX in `AEXCOMPAT_PLUGIN_ROOT`:

```sh
AEXCOMPAT_PLUGIN_ROOT="/path/to/plugin-directory" \
AEXCOMPAT_HARNESS="$PWD/broker/target/release/aexcompat-harness" \
AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker" \
/Applications/Blender.app/Contents/MacOS/Blender -b -t 2 \
  --python tools/blender_aexcompat_parameter_smoke.py \
  -- --output-dir target/issue1658/blender-smoke
```

The wrapper describes the plug-in at each override bake; this is bounded by the request timeout and currently adds one setup pass. The descriptor node exposes one numeric slot at a time. Automatic named controls, multiple simultaneous overrides, and direct custom-node execution in the compositor remain outside this slice.
