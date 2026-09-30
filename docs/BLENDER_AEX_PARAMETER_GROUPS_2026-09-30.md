# AEX parameter groups in macOS Blender (#1699)

The Blender parameter picker now shows each editable parameter's AEX group path beside its name and slot. This distinguishes names reused by different groups, such as `Outer Blur / Strength` and `Inner Blur / Strength`. Selecting a parameter keeps the path in the `.blend`, while the render request still contains only the typed slot and value. Flat plug-ins keep their existing labels.

The description wrapper walks `group_start` and `group_end` records in order. It retains at most eight levels with nonempty labels of at most 128 characters. If the group tree is malformed or unbalanced, the entire catalog falls back to flat labels; the original enabled, visible, and type checks still decide which parameters are editable. The Blender addon and JSON Schema validate any supplied group path before displaying it.

On Blender 4.5.8 LTS, the smoke found 217 editable OLMColorKey parameters and distinguished `Edge Thin / Amount` from `Edge Blur / Amount`. It selected slot 14, baked a real SmartFX frame, and matched a direct typed render. That ColorKey input was unchanged by the chosen value, so it is evidence of a working grouped request, not of a visible ColorKey effect. The same smoke selected license-free OLMRadialBlur's `Outer Blur / Strength` at 500: its SmartFX bake differed from the input by 12,176 RGBA8 bytes and matched a direct render. Both selected labels, values, and packed images survived `.blend` save/reload. The flat OLMToonDilate description remained flat.

Run from the repository root with locally built Release binaries and the installed OLM directory:

```sh
AEXCOMPAT_PLUGIN_ROOT=/path/to/OLM \
AEXCOMPAT_HARNESS="$PWD/broker/target/release/aexcompat-harness" \
AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker" \
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup -t 2 \
  --python tools/blender_aexcompat_group_smoke.py -- --output-dir target/issue1699/blender

/Applications/Blender.app/Contents/MacOS/Blender --background -t 2 \
  target/issue1699/blender/grouped-parameter-bake.blend \
  --python tools/blender_aexcompat_group_smoke.py -- \
  --output-dir target/issue1699/blender --reload
```

The output directory is ignored by Git. The evidence records plug-in, worker, and harness hashes; no plug-in or local project file is committed. After Effects is not launched.
