# ThreeForAE external renderer

ThreeForAE is an image effect, not an AEGP. Its installed default scene can
return a transparent frame with a successful native return when the external
renderer is not running. Do not count that frame as a correctly rendered scene.

## Runtime contract

The separately installed `ae-three-renderer` project supplies an Electron
renderer-host. Its named-pipe endpoint is `\\.\pipe\ae-three-renderer-v1`;
frames use the project's protocol and shared-memory transport. The plugin's
Bypass control copies input without needing the external renderer.

The host must be ready before opening the render session. Process existence
alone is insufficient: in the local reproduction, Electron existed before
the pipe appeared. Pipe existence is only a readiness hint, not proof that
the protocol or a requested frame works. Verify the resulting pixels too.

Do not weaken worker isolation, change firewall/security settings, or launch
arbitrary executables inferred from plugin strings to make this case pass.
For registered installations, the shared render-session path starts the renderer
automatically when its pipe is absent. It uses the existing private-desktop,
memory-limited, kill-on-close process Job. An already-running renderer is borrowed
and is never terminated by the session. A broker reservation excludes concurrent
managed sessions, but cannot coordinate unrelated clients (see limitations below).

## One-time installation registration

The installer can invoke `tools/register_three_renderer.py` with its known
`--runtime-root` and `--plugin`, once per installed image AEX. For example,
using placeholders for the installer-supplied absolute paths:

```text
python tools/register_three_renderer.py --runtime-root <renderer-host> --plugin <ThreeForAE.aex>
python tools/register_three_renderer.py --runtime-root <renderer-host> --plugin <ThreeRenderer.aex>
```

This records the exact associations in
`%LOCALAPPDATA%/AEXCompat/render-services/three-v1/<path-key>.json`. The key is the
SHA-256 of the canonical path with Windows namespace prefix removed, forward
slashes and ASCII lowercase, encoded as UTF-8; it is not a hash of AEX bytes.
Only the selected path's record is read, so malformed metadata cannot block
unrelated effects. The former global `three-v1.json` is not used or deleted.
GUI and CLI sessions read
the same record; rendering does not prompt for a runtime folder. The tool validates
the built Electron executable, main entry and renderer page, but does not start
them, build another project, install dependencies, or change the AEX binaries.
Use a normal Windows Python installation: packaged Store Python may redirect
LocalAppData and publish into a location the native application cannot see.

Publication is create-only and atomic; an existing registration is left untouched
and reported as an error. Updating/removing an existing installation record is
not yet implemented by this tool. The runtime location is installation metadata,
not a plugin hash allowlist, and runtime/AEX hashes are not launch approval gates.
This tool is an installer integration point, not automatic discovery of arbitrary
unregistered development checkouts or proof that a third-party installer calls it.

## Local reproduction

Use an existing, inspected renderer installation and its own documented
host-only entry point. Do not use a combined launcher that starts After Effects.
No AE application is needed for this test.

For an already built installation, the tested entry point was its bundled
`node_modules/electron/dist/electron.exe` with `out/main/index.js` as the
argument and the `renderer-host` directory as the working directory. The
tested host loaded `out/renderer/index.html`; ensure `ELECTRON_RENDERER_URL`
does not redirect it to another page and `ELECTRON_RUN_AS_NODE` is unset in
that child environment. Do not install dependencies or rebuild another
project as an implicit part of rendering.

1. Record AEX, worker, harness and external-runtime identities.
2. With the renderer absent, run a shipping `--render-fixture` case using
   Scene=2 (Test Grid), Bypass=0 and other inspected defaults.
3. Start only the known renderer-host and verify its pipe is available.
4. Run the same fixture into a new output directory. Separately set Bypass=1
   on a structured input and verify exact native-ARGB input/output equality.
5. Validate output dimensions, byte extent, digest, alpha and scene contents.
6. Stop only the host process tree created for the experiment. Never terminate
   a renderer that another session already owned.

## Observed boundary

At 256x144, ARGB8, SmartFX and time0, the absent-host Test Grid had zero
visible pixels. With the host ready, all 36,864 pixels were visible. The
decoded image showed a perspective ground grid with red, green and blue axes,
consistent with the local Test Grid scene definition. Bypass was byte-identical
to the structured input in both conditions.

This is conditional runtime/scene evidence, not AE-reference equivalence,
coverage of every scene, or proof of ThreeRenderer compatibility. Automatic
managed startup and cleanup, idle-host borrowing, and the HarnessApp resident
GUI controller's Grid/Bypass path have since been exercised with ThreeForAE.
Other scene controls, timing, color depths and actual mouse-driven GUI operation
remain separate verification work. Preserve the original empty baseline instead
of replacing it with the successful external-host run.

### Known external-client contention gap

The renderer protocol accepts only one active client. A non-broker client can
occupy it while Windows still reports an available named pipe; the server then
accepts and disconnects subsequent clients. A controlled reproduction produced
an all-transparent image and CLI exit code zero in this situation. That is not
verified successful rendering. The broker reservation alone does not fix this.
An ACK probe followed by disconnect would still race with another client; an
end-to-end connection ownership or explicit AEX error-propagation contract is
needed. Do not treat all transparent images as errors to hide this specific gap,
and do not terminate an existing host or client to make a render pass.
