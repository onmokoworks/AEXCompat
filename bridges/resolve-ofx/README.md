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
suite host. Describe and lifecycle are real binary calls. The fake host uses the official
alpha string independently of our header and checks that SyncPrivateData and
an unknown action return ReplyDefault (14), rather than ReplyYes (12). The control render is
also a real OFX image-suite call: it darkens RGBA8 and 32-bit float RGBA
fixtures' RGB channels,
preserves premultiplied alpha, honors a non-packed render window and rowbytes,
and records input/output SHA-256 plus pixel diff. The float fixture also rejects
mismatched source/output depths without writing output. This proves the host-facing
pixel path without claiming AEX compatibility.

On macOS, setting `AEXCOMPAT_RESOLVE_AEX_PATH` enables the real AEX path from
#385. The OFX module packs host rows into bounded RGBA8, converts premultiplied
float/byte pixels to straight alpha, and sends one frame through the verified
macOS worker. The source path must be relative to `AEXCOMPAT_PLUGIN_ROOT`.
OpenFX time is an output-frame coordinate; the instance's project frame rate
converts it to milliseconds before the AEX request. Missing or invalid frame
rate fails without writing the host image.
The worker's validated packet supplies plug-in/worker identities and output
hashes. A failed worker or a missing executable leaves the host output
untouched. When no AEX source is configured, the original control render
remains available.

The bridge launches the configured Python interpreter and runner without a
shell. For a local Resolve run, set these process-local variables before
starting Resolve:

```text
OFX_PLUGIN_PATH=<staged bundle parent>
AEXCOMPAT_RESOLVE_AEX_PATH=DistanceGradation.aex
AEXCOMPAT_PLUGIN_ROOT=<directory containing that AEX>
AEXCOMPAT_HARNESS=<absolute Release harness path>
AEXCOMPAT_GUEST_WORKER=<absolute Release guest worker path>
AEXCOMPAT_RESOLVE_PYTHON=<absolute Python path with jsonschema installed>
AEXCOMPAT_RESOLVE_RUNNER=<absolute tools/resolve_ofx_macos_runner.py path>
AEXCOMPAT_RESOLVE_EVIDENCE_DIR=<existing directory for compact per-render JSON>
```

The evidence directory is optional. Its files contain hashes and geometry,
not frame contents. This first route is one process and one AEX render per
OFX callback; float inputs are quantized to RGBA8, and frame time is rounded
to milliseconds. It is not a persistent video session.

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
dimensions, and the changed region are recorded in the JSON contract. That
earlier Resolve run was control-only; a real Resolve AEX render must be
recorded separately below.

The new native OFX smoke used the locally held license-free
`DistanceGradation.aex` at 64×64, OpenFX frame 6 at 24 fps (250 ms). The host supplied padded float
rows of 1040 bytes; the output rows were 1056 bytes. The worker accepted
packed RGBA8, and 9,157 RGB float values changed in the OFX output. Opaque,
half-alpha, and zero-alpha samples retained premultiplied RGB and alpha;
padding was preserved. A deliberately missing runner returned
`kOfxStatErrUnsupported` without changing that output. The contract records
the host and worker frame hashes plus AEX/worker hashes. This native smoke
does not by itself prove that Resolve invoked the AEX route. Compact evidence
files report `worker_rendered` and do not claim successful host publication.

A separate real Resolve 21.0.4.0005 run loaded the rebuilt arm64 bundle and
connected MediaIn → AEXCompat Resolve OFX → MediaOut in a dedicated Fusion
composition. A 256×256 patterned image exported unchanged with the effect
disconnected, including a repeated export. With DistanceGradation connected,
all 65,536 RGB pixels changed to white. The RGB export exactly matches the
worker's opaque RGBA8 white output after dropping alpha. The unmodified AEX
default parameters produced this uniform image; this does not establish
gradient quality or parameter mapping. Resolve supplied float RGBA with
4096-byte rows. Separate callbacks at frames 0 and 6 of a 24 fps project
recorded 0 and 250 ms and produced matching exports. The new
`macos_aex_host_evidence` contract records these identities, PNG hashes, and
comparisons. RGB exports do not prove host alpha; the padded native fixture
remains the alpha evidence. The Windows real-host path remains unverified.

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
