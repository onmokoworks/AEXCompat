# DaVinci Resolve OFX surface

Issue #388 adds a small Windows OFX binary and a host-side lifecycle smoke. It
uses the official OpenFX ABI names and struct prefixes without copying the
OpenFX SDK into this repository. The ABI provenance is the Academy Software
Foundation OpenFX repository and its BSD-3-Clause license.

The package shape is:

```text
AEXCompatResolve.ofx.bundle/
  Contents/
    Win64/
      AEXCompatResolve.ofx
```

The plugin exports the standard `OfxGetNumberOfPlugins`, `OfxGetPlugin`, and
`OfxSetHost` functions. `OfxPluginMain` is also exported as an audit alias for
the plugin's `OfxPlugin::mainEntry`; it is not treated as a replacement for the
standard discovery exports.

The lifecycle smoke drives load, describe, describe-in-context, create-instance,
render, destroy-instance, and unload through a local property/image/parameter
suite host. Describe and lifecycle are real binary calls. The control render is
also a real OFX image-suite call: it darkens an RGBA8 fixture's RGB channels,
preserves premultiplied alpha, honors a non-packed render window and rowbytes,
and records input/output SHA-256 plus pixel diff. This proves the host-facing
pixel path without claiming AEX compatibility.

The AEX RenderSession gate remains separately fail-closed because #385/#386 have
not supplied that bounded route on this branch. The JSON contract therefore
records `control_render.state=verified_local_fixture` alongside
`aex_render_gate.state=blocked`; the fixture is not an identity or AEX success.

## Host evidence

The machine has DaVinci Resolve 18.6.6.7 at the standard Blackmagic install
location. `Resolve.exe`, `OFXLoader.exe`, and the internal
`Plugins/openfx.plugin` are PE32+ binaries. The standard external path
`%ProgramFiles%/Common Files/OFX/Plugins` exists but has zero entries. No
external package was installed and no real Resolve discovery/render result is
claimed.

The observed executable and internal plugin SHA-256 values are fixed in
`contracts/resolve_ofx_surface.json`. The `OFXLoader.exe` file has no usable
product-version metadata in this installation.

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
