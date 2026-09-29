# macOS Blender real AEX bake (#1654)

The Blender addon now offers `render_aex` alongside its two explicitly non-AEX transport modes. It bakes one packed RGBA8 Blender Image through the existing macOS headless fixture harness and guest worker, stores the verified output as a packed Image, and connects only native `CompositorNodeImage -> CompositorNodeComposite` nodes. The Python descriptor node is never placed in the compositor executor.

## Setup

Build the native tools from this checkout:

```sh
cargo build --release --manifest-path broker/Cargo.toml -p aexcompat-harness
cargo build --release --manifest-path guest/Cargo.toml -p aex-guest-worker
```

Set `AEXCOMPAT_HARNESS` and `AEXCOMPAT_GUEST_WORKER` to those two binaries, and `AEXCOMPAT_PLUGIN_ROOT` to a directory containing legally obtained AEX files. The addon's Plugin source field is a relative path below that root. The addon package can be installed separately from the source checkout because it bundles `session_wrapper.py`; the three paths remain explicit. `AEXCOMPAT_SESSION_PYTHON` can select a Python interpreter for the wrapper.

The first real-render slice accepts one Classic ARGB8 frame, packed RGBA8 input rows, straight alpha, no parameter overrides, and a frame time of 0–3600 seconds rounded to milliseconds. Render timeout is bounded at 180 seconds. The request validates the AEX path inside the configured root. The response checks the fixture report, case identity, plugin SHA-256, input checkpoint pixels, final artifact dimensions/stride/channel order, SHA-256, and matching report metadata before the addon creates or replaces a Blender Image. Failures do not publish a new Image. AEX, harness, and guest worker file hashes are recorded before and after the render with a `files_unchanged` flag; this is evidence, not an admission gate. When it is false, the result cannot establish stable file identity. These checks do not establish AE pixel parity or OFX support.

## Mac smoke

With the three paths configured, run:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --python tools/blender_aexcompat_real_smoke.py -- \
  --output-dir target/issue1654/blender --plugin DistanceGradation.aex
```

For save/reload verification, register the addon before opening the saved `.blend`, then run the same smoke script with `--reload`. The smoke writes a local JSON evidence file and PNG under the ignored output directory; it does not package or publish the AEX binary.

On this Mac, Blender 4.5.8 LTS baked a 64×64 frame through the locally held OLM `DistanceGradation.aex`. The AEX SHA-256 was `a1d317c0e18371494bc9c9933684593ca903eb6f3fe262ec06d5147b4c0bcbae`; the guest worker SHA-256 was `87439bb8682fa5fd79e21a844323d1b12b96d85136001721e83346030375dfde`, and the harness SHA-256 was `365a8f43ae2d54a0be7119b90ea222b6c10bc21dd2ae61c6ab067314d2c93a85`. Input SHA-256 was `79c9e782e5496df7f82d55a616deb87b783d5709b4fc3561edc11ecc393a110b`, output SHA-256 was `0fbba07a833d4dcfc7024eaf313661a0ba8f80a05c6d29b8801c612e10e60dee`, and 12,256 of 16,384 bytes changed. The native compositor PNG rendered, and a separate Blender process reopened the saved project with the packed output SHA unchanged. After Effects was not started.

The installed host is 4.5.8; 3.6 and 4.5.9 were not rechecked in this change. Blender color management may transform the compositor PNG; the SHA above is for the packed RGBA8 Image before that display transform. Per-plugin parameter UI, multi-frame resident rendering, premultiplied input, deep formats, connected Python compositor node execution, and AE reference parity remain outside this slice.
