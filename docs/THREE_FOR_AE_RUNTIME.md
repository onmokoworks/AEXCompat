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
Runtime startup is not currently automatic in AEXCompat.

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
coverage of every scene, or proof of ThreeRenderer compatibility. Other scene
controls, timing, color depths, actual GUI operation and automatic runtime
lifecycle management remain separate verification work. Preserve the original
empty baseline instead of replacing it with the successful external-host run.
