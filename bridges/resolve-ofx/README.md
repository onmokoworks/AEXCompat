# DaVinci Resolve OFX surface

Issue #388 adds a small Windows/macOS OFX binary and a host-side lifecycle smoke. It
uses the official OpenFX ABI names and struct prefixes without copying the
OpenFX SDK into this repository. The ABI provenance is the Academy Software
Foundation OpenFX repository and its BSD-3-Clause license.

The package shape is:

```text
AEXCompatResolve.ofx.bundle/
  Contents/
    Win64/
      AEXCompatResolve.ofx
    MacOS/
      AEXCompatResolve.ofx
    Info.plist
```

The plugin exports the standard `OfxGetNumberOfPlugins`, `OfxGetPlugin`, and
`OfxSetHost` functions. `OfxPluginMain` is also exported as an audit alias for
the plugin's `OfxPlugin::mainEntry`; it is not treated as a replacement for the
standard discovery exports.

The lifecycle smoke drives load, describe, describe-in-context, create-instance,
render, destroy-instance, and unload through a local property/image/parameter
suite host. Describe and lifecycle are real binary calls. The control render is
also a real OFX image-suite call: it darkens RGBA8 and 32-bit float RGBA
fixtures' RGB channels,
preserves premultiplied alpha, honors a non-packed render window and rowbytes,
and records input/output SHA-256 plus pixel diff. The float fixture also rejects
mismatched source/output depths without writing output. This proves the host-facing
pixel path without claiming AEX compatibility.

The AEX RenderSession gate remains separately fail-closed because #385 has not
supplied that bounded route on this branch. The JSON contract therefore
records `control_render.state=verified_local_fixture` alongside
`aex_render_gate.state=blocked`; the fixture is not an identity or AEX success.

## Host evidence

The earlier Windows machine had DaVinci Resolve 18.6.6.7 at the standard Blackmagic install
location. `Resolve.exe`, `OFXLoader.exe`, and the internal
`Plugins/openfx.plugin` are PE32+ binaries. The standard external path
`%ProgramFiles%/Common Files/OFX/Plugins` exists but has zero entries. No
external package was installed and no real Resolve discovery/render result is
claimed.

The observed executable and internal plugin SHA-256 values are fixed in
`contracts/resolve_ofx_surface.json`. The `OFXLoader.exe` file has no usable
product-version metadata in this installation.

On macOS arm64, Resolve 21.0.4.0005 discovered the staged MacOS bundle through
`OFX_PLUGIN_PATH`, exposed the Strength control, and rendered a local checker
frame from `fixtures/control-input.png` at Strength 0.5. Resolve supplied 32-bit float RGBA images with rowbytes
30720 for a 1920×1080 frame. Exported PNGs with the effect off and on differ
in all 1,166,400 content pixels; the sidebars remain unchanged. The exported
PNGs are RGB previews, so they do not prove the host alpha channel. The native
float fixture separately verifies alpha and padding. File hashes, frame
dimensions, and the changed region are recorded in the JSON contract. No AEX
render was claimed.

The bounded render path accepts positive rowbytes. Negative rowbytes, although
valid in OpenFX, return `kOfxStatErrFormat`; the tested Resolve host provided
positive rowbytes. The earlier Windows binary hash remains historical evidence,
separate from the current macOS source/binary identity.

## Build and smoke

From a Visual Studio developer PowerShell:

```powershell
cmake -S bridges/resolve-ofx -B target/resolve-ofx -G Ninja
cmake --build target/resolve-ofx --config Release
target/resolve-ofx/resolve_ofx_smoke.exe target/resolve-ofx/AEXCompatResolve.ofx
```

The smoke succeeds only when all lifecycle actions succeed, the control render
changes the expected RGB bytes, preserves alpha and padding, and returns
`kOfxStatOK`. It also emits the distinct `aex_render_claim` blocker. A no-op
render or an AEX success claim is not considered success.

On macOS:

```sh
cmake -S bridges/resolve-ofx -B target/resolve-ofx -DCMAKE_BUILD_TYPE=Release
cmake --build target/resolve-ofx --config Release
cmake --install target/resolve-ofx --prefix target/resolve-ofx/stage
target/resolve-ofx/resolve_ofx_smoke target/resolve-ofx/stage/AEXCompatResolve.ofx.bundle/Contents/MacOS/AEXCompatResolve.ofx
```

To run a local Resolve discovery test, set `OFX_PLUGIN_PATH` to the stage
directory only for the Resolve process. The bundle need not be installed into
the system OFX directory.
