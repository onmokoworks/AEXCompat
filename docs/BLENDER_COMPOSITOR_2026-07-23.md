# Blender Compositor adapter (Issue #387)

## Compatibility matrix

| Blender | `bpy.types.CompositorNode` | `bpy.types.CompositorNodeOFX` | `CompositorNodeTree` | Result |
| --- | ---: | ---: | ---: | --- |
| 3.6.12 | yes | no | yes | addon node/register/create supported; AEX render unverified |
| 4.5.9 LTS | yes | no | yes | addon node/register/create supported; AEX render unverified |

The API result above was obtained from the installed executables with:

```powershell
& 'C:\Program Files\Blender\blender-3.6.12-windows-x64\blender.exe' --background --factory-startup --python-expr "import bpy; print({'version': bpy.app.version_string, 'CompositorNode': hasattr(bpy.types, 'CompositorNode'), 'CompositorNodeOFX': hasattr(bpy.types, 'CompositorNodeOFX'), 'CompositorNodeTree': hasattr(bpy.types, 'CompositorNodeTree')})"
& 'C:\Program Files\Blender\blender-4.5.9-windows-x64\blender.exe' --background --factory-startup --python-expr "import bpy; print({'version': bpy.app.version_string, 'CompositorNode': hasattr(bpy.types, 'CompositorNode'), 'CompositorNodeOFX': hasattr(bpy.types, 'CompositorNodeOFX'), 'CompositorNodeTree': hasattr(bpy.types, 'CompositorNodeTree')})"
```

Both versions returned `CompositorNodeOFX: False`. The addon uses the public Python `CompositorNode` base and does not call Blender private APIs or claim to provide an OFX node.

## Implemented boundary

`blender_addon/aexcompat_blender` registers a transport descriptor node and an `aexcompat.bake_image` operator. The operator reads a named Blender Image datablock, converts it to bounded RGBA8, launches `tools/blender_aexcompat_session.py` out of process, stores the returned pixels in a packed generated Image, and connects only native `CompositorNodeImage -> CompositorNodeComposite` nodes. This baked-image bridge is the safe host integration because Blender does not expose a Python callback for executing a custom compositor node.

The wrapper has two explicit non-AEX modes: `identity_no_aex` for transport validation and `fixture_invert_no_aex` for proving a real pixel transform through the worker and native Image node. Neither mode opens, hashes, loads, or renders an AEX. Their responses retain `failure_class=aex_not_loaded`, `aex_render_performed=false`, and `host_success=false`. The fixture transform proves the host/image transport path only; it is not AEX or OFX compatibility.

## Reproduction

```powershell
$smoke = Join-Path $env:TEMP 'aexcompat-blender-smoke-387'
New-Item -ItemType Directory -Force $smoke | Out-Null
foreach ($target in @(@{exe='C:\Program Files\Blender\blender-3.6.12-windows-x64\blender.exe'; tag='36'}, @{exe='C:\Program Files\Blender\blender-4.5.9-windows-x64\blender.exe'; tag='45'})) {
  $out = Join-Path $smoke $target.tag
  New-Item -ItemType Directory -Force $out | Out-Null
  & $target.exe --background --factory-startup --python tools/blender_aexcompat_smoke.py -- --output-dir $out
  $blend = Join-Path $out 'aexcompat_blender_smoke.blend'
  $expr = "import sys; sys.path.insert(0, r'C:\path\to\AEXCompat\blender_addon'); import aexcompat_blender; aexcompat_blender.register(); import bpy; bpy.ops.wm.open_mainfile(filepath=r'$blend')"
  & $target.exe --background --factory-startup --python-expr $expr --python tools/blender_aexcompat_smoke.py -- --reload --output-dir $out
}
```

The JSON evidence records register/create/direct transport evaluate, fixture pixel transform, packed-image save/reload, native `CompositorNodeImage -> CompositorNodeComposite` rendering, PNG dimensions/SHA, and a separate safe native-only background compositor operator result. The custom node is deliberately absent from the render graph: an earlier connected-node probe crashed both installed hosts with `EXCEPTION_ACCESS_VIOLATION`. The result is therefore classified as `custom_python_node_execution_unverified` and the crash is treated as a Blender host blocker, not as addon or AEX success. Any AEX load failure, session timeout, worker crash, or addon error remains a separate classification.

## Explicit blockers

- `CompositorNodeOFX` is absent in both tested installations.
- The current repository's AEX/OFX facade is no-load/mock-only, so no real AEX can be loaded from this adapter.
- A Python-defined compositor node can be registered and serialized, but real compositor-engine execution of its Python method is not proven by the public API; the implementation uses the baked-image bridge instead.
- Connecting the Python-defined node to the compositor executor was probed once on each installed version and caused `EXCEPTION_ACCESS_VIOLATION`; the repeatable smoke intentionally avoids that crash path and records it as an external host blocker.
- The fixture invert path is deliberately named and reported as non-AEX evidence. Replacing it with the common RenderSession/AEX backend is a dependency boundary, not hidden in Blender-specific code.
- #385's common OpenFX/RenderSession implementation and #386's AEX load audit are dependencies. This issue does not change either dependency.
