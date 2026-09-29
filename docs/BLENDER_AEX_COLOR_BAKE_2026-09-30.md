# Blender named ARGB8 color bake on macOS (#1664)

The Blender compositor descriptor now lists enabled, visible AEX color parameters alongside numeric parameters. Refresh the AEX list, choose a color by name and slot, select its row in the chosen-override list, then edit the four **ARGB8** integer channels (alpha, red, green, blue; each 0–255) in the field below that list. Up to 16 distinct numeric/color selections can share one bake. The wrapper resolves them from one AEX description and passes one typed parameter list to one Classic or SmartFX frame render. Invalid colors, duplicates, mismatched parameter kinds, and inconsistent response identity fail before publishing a baked image. The previous single numeric slot and scalar-only list remain supported.

Blender 4.5.8 LTS on this Mac used the locally held, license-free `OLMKiraKira.aex` (SHA-256 `60997c0c52207c15844a46289435231fa6b0a885f63778404e02cea6e03899f7`) on a 32×32 straight-alpha image with a white center on black. The input SHA-256 was `49e7a862773bbf36b18028b662c8b14a42a7bf22a642d1d19f4cd6a64a8c22be`. Four SmartFX bakes produced distinct verified packed RGBA8 images:

| Settings | SHA-256 |
| --- | --- |
| Defaults | `733591c8752bf14882b57009f9bc80a54f505b1d247ea228acc96aca641bc4e6` |
| `Vertical Length` (#10) = 100 | `c3d72dcb2fc319a4d7c2c0964f089afea14612b0e777b25e560007871ce49110` |
| `Vertical Color` (#11) = ARGB8 `[255,255,0,0]` | `b49ed5afae5a5f997b139e21aeb38ac97d56e08a54eb58f0d009568dca1fa7a5` |
| Both | `d2c61c02ee58500cede54c63864b893addf4debcbc06b5140e481da8d9c54a6d` |

The mixed result changed 2,070 bytes versus the default. The native Blender compositor wrote a PNG, and a separate Blender process reopened the saved `.blend` with both selected values and all packed image hashes intact. Existing single-picker and scalar-only multi-picker OLM smoke tests still passed. The proprietary AEX, `.blend`, PNG, and smoke JSON stay outside the repository. After Effects was not started. This checks actual AEX pixel response, not AE pixel parity or direct execution of the custom Python node inside Blender's compositor.

To reproduce, set `AEXCOMPAT_PLUGIN_ROOT` to the directory containing the legally held OLM AEX, and `AEXCOMPAT_HARNESS` and `AEXCOMPAT_GUEST_WORKER` to the Release binaries. Run Blender with `--python tools/blender_aexcompat_color_parameter_smoke.py -- --output-dir target/issue1664/blender-smoke`, then reopen the saved `.blend` and rerun the script with `--reload`.
