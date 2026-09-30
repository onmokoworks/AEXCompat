# Frida hooks used for After Effects observation records

Dynamic-observation scripts behind the `docs/` RE records. They attach to a
running After Effects and observe only: hooks are installed in-process
(`Interceptor.attach` patches the hooked entry points), but no argument,
return value or state is altered. Frida is not a repository dependency (`uv tool install frida-tools`,
or a scratch venv with `frida`).

| script | record | how it was run |
| --- | --- | --- |
| `timecode_bee_scene_hook.js` | `docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md` §3 (issue #1210) | `tools/capture-ae-reference.ps1` launched AE 2026 (`AfterFX.com -m -noui -r`, `ADBE Timecode`, 256x144 PNG, 30 fps, frame 0); a Python runner polled `frida.get_local_device().enumerate_processes()` for `AfterFX.com` (the `-noui` launch runs AE inside the `.com` process, no `AfterFX.exe` appears), attached, loaded this script, and wrote every `send()` payload as one JSON line; the script installs its hooks from `LdrLoadDll` so `Timecode.aex` is hooked before its first RENDER |

`timecode_bee_scene_hook.js` hooks, filtered to callers inside `Timecode.aex`:
`Timecode.aex+0x61a0` (RENDER: `in_data` time fields and the raw parameter
values), the BEE.dll exports `BEE_GetSourceTimeFormat`, `BEE_LayerToSourceTime`,
`BEE_GetProjectTimeFormat`, `BEE_Item::GetParentProject`, `BEE_Item::GetFlags`,
`BEE_GetSourceMediaInfo`, `BEE_GetCompSettings`, the internal fps getter
`BEE.dll+0x475aa0`, the layer vtable slots 56/65/79/183/184 (resolved from
the live object's vtable), and U.dll `T_GeneralFormatTime`,
`T_FrameRate2Duration`, `T_GetMaxFPS`. For each object it dumps RTTI name,
vtable address (module-relative) and the byte ranges the record quotes.
Module offsets are for the AE 2026 26.3 binaries; re-derive them for another
build (`tools/ghidra/`).
