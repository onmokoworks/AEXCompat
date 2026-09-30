# BEE scene-object ABI behind the effect layer handle (issue #1210)

Observation record for the private After Effects scene-object layout that
Adobe-bundled `Timecode.aex` reads through the `AEGP_LayerH` it obtains from
`AEGP PF Interface Suite::AEGP_GetEffectLayer`, and for the host facade that
now stands behind that handle (`minihost/src/worker_bee_scene_facade.*`).
Observations (static decompile, dynamic capture) are separated from the
inferences drawn for the implementation. Time-ordered log at the end.

Scope: only what Timecode's RENDER reaches. Every other field of the objects
below is unobserved and stays zero in the facade; every other vtable slot is
an identifying trap (see "Host facade").

## 1. Why a public-suite implementation is not enough (observation)

`Timecode.aex` (AE 2026 26.3, SHA-256 `ea4c45df…ce9f2c`) imports BEE.dll,
U.dll and TXT.dll directly (dumpbin /imports):

| DLL | import |
| --- | --- |
| BEE.dll | `BEE_GetSourceTimeFormat(BEE_Layer const*, bool, int*, T_TimeFormatInfo*)` |
| BEE.dll | `BEE_Item::GetParentProject()` (returns `BEE_Project*&`) |
| BEE.dll | `BEE_Item::GetFlags()` (returns `std::atomic<int>&`) |
| BEE.dll | `BEE_LayerToSourceTime(BEE_Layer const*, T_Time const*, T_Time*, TDB_ParamBag const*)` |
| BEE.dll | `BEE_GetProjectTimeFormat(BEE_Project const*, T_TimeFormatInfo&)` |
| U.dll | `T_GeneralFormatTime`, `T_FrameRate2Duration`, `T_Time::operator*(int)` |
| TXT.dll | `TXT_GetFontServer`, `TXT_Font_Usage_Logger::LogFontUsageForFont` |

RENDER (`Timecode.aex+0x61a0`) acquires `AE Timecode Helper Suite` v1 (fatal
gate, no slot ever called), then `AEGP PF Interface Suite` v1 slot 0
(`AEGP_GetEffectLayer`) and uses the returned handle as a `BEE_AVLayer*`
(the previous session's decompile, confirmed here by the dynamic capture in
section 3). No public suite callback stands between the plug-in and those
reads, so the handle itself has to carry the layout.

## 2. Static observation: BEE.dll / U.dll (Ghidra 12.0.3, headless)

BEE.dll (24,676,360 bytes) and U.dll (1,392,648 bytes) from AE 2026
`Support Files` were imported into the AEXCompat Ghidra project and the
export symbols decompiled without full analysis (`tools/ghidra/DumpFromSymbols.java`,
`tools/ghidra/DumpVtable.java`; usage in `tools/ghidra/README.md`). BEE.dll
exports its virtual functions by name, so the vtable slots are named directly
from the export table: the appendix is the export-symbol walk from each
vtable address (`BEE.dll+0xf79128` etc.) until the first entry that does not
point into executable memory (the next vtable's RTTI locator).

### 2.1 Field accessors (exported, non-virtual)

| export | body |
| --- | --- |
| `BEE_Item::GetParentProject()` | `return this + 0x38` (a `BEE_Project*&`) |
| `BEE_Item::GetFlags()` | `return this + 0x4c` (`std::atomic<int>&`) |
| `BEE_Item::GetType()` | `return this + 0x48` (`short&`; 4 = comp, 7 = footage) |
| `BEE_Project::GetTimeDisplay()` | `return this + 0x84` (`BEE_TimeDisplayFormat&`, 16 bytes) |
| `BEE_Project::GetBitDepth()` | `*(uint8_t*)(this + 0x94)` |
| `BEE_Project::GetShowCheckerboardThumbnails()` | `*(uint8_t*)(this + 0x95)` |
| `BEE_Project::GetCurrentExpressionEngine()` | `this + 0x2e8` |
| `BEE_FootageItem::GetPinSeqH()` | `**(this + 0x2f8)` (`shared_ptr<PIN_SeqSpec>` storage) |
| `BEE_CompItem::GetMax2DMotionBlurSamples()` | `*(int*)(this + 0x418)` |
| `BEE_CompItem::GetStdMotionBlurSamples()` | `*(int*)(this + 0x41c)` |
| `BEE_CompItem::GetDisplayDropframe()` | `*(uint8_t*)(this + 0x424)` |
| `BEE_AVLayer::IsLayerType(int)` (virtual, slot 79) | `return type == 0` |
| `BEE_AVLayer::pGetSourceItem(bag)` (virtual, slot 183) | tail-calls slot 184 with `(this, nullptr)` |
| `BEE_AVLayer::pGetConstSourceItem(bag)` (virtual, slot 184) | `bag ? vslot 244 : this[0x4e4]` (= `*(BEE_Item**)(this + 0x2720)`) |

### 2.2 Export contracts on the RENDER path

- `BEE_GetProjectTimeFormat(project, fmt&)`: zero-fills 24 bytes, calls
  `BEE_GetProjectSettings(project, &tdf, 0, 0, 0)` (copies the 16 bytes at
  `project + 0x84`), then `BEE_TimeDisplayFormat2T_TimeFormatInfo(&tdf, &fmt)`.
  Nothing else of the project is read.
- `BEE_TimeDisplayFormat2T_TimeFormatInfo(tdf, fmt)`: `fmt[0] = tdf[0]==0`,
  `fmt[1] = tdf[2]==0`, `fmt[2] = tdf[1]==1`, `fmt.i32@4 = tdf.i32@4`,
  `fmt.u8@0xc = tdf[0xc]`, `fmt.i32@8 = tdf[3] ? tdf.i32@8 : 0`, `fmt[3] = tdf[3]`,
  `fmt.i64@0x10 = 0`.
- `BEE_GetSourceTimeFormat(layer, use_parent_comp, int* fps, fmt*)`: item =
  `use_parent_comp ? *(BEE_Item**)(layer + 0x260) : vslot184(layer, nullptr)`.
  If item is null, returns 0 without touching the outputs. Otherwise
  `BEE_GetSourceMediaInfo(item, &info, &has)`; when that returns 0 and `has`,
  the fps getter (`BEE.dll+0x475aa0`: comp → `*(int*)(item + 0x2bc)`,
  footage → `*(int*)(pinseq + 0x1f0)`, else 0) overrides `*fps` when nonzero,
  and `fmt[1] = !info.dropframe`, `fmt.i32@4 = (*fps + 0x8000) >> 16`,
  `fmt.T_Time@0x10 = info.T_Time@0`.
- `BEE_GetSourceMediaInfo(item, info, has)`: refuses (`Up_ReportErrString`,
  returns 2) unless `*(uint16_t*)(item + 8) == 0xBEE1`. Comp (type 4):
  `BEE_GetCompSettings(item, &cs)` (which itself checks the 0xBEE1 tag),
  `info.T_Time@0 = info.T_Time@8 = cs[2]`, `info.T_Time@0x10 = cs[2] + cs[1]`,
  `info.u8@0x38 = cs.u8@0x48`, `info.u8@0x39 = 1`, `has = 1`. Footage (type
  7) reads the `PIN_SeqSpec` behind `item + 0x2f8` (fields +0x1f0, +0x1f4,
  +0x1f5, +0x1f8, and `+0x28 -> {+0xc T_Time, +0x18 dvacore wstring, +0x38}`) and
  `item + 0x268 / + 0x270` (T_Time). Not reached on the observed path.
- `BEE_GetCompSettings(item, cs)`: `cs[0] = item`, `cs[1] = T_Time@0x298`,
  `cs[2] = T_Time@0x2a0`, `cs.i16@0x18 = @0x2b8`, `cs.i16@0x1a = @0x2ba`,
  `cs.i32@0x1c = @0x2bc`, `cs[4] = @0x268`, `cs.i32@0x28 = @0x264`,
  `cs.i32@0x2c = @0x260`, `cs[6] = @0x300`, `cs[7] = @0x308`,
  `cs.i32@0x40 = @0x418`, `cs.i32@0x44 = @0x41c`, `cs.u8@0x48 = @0x424`.
- `BEE_LayerToSourceTime(layer, in, out, bag)`: `if (vslot79(layer, 0) &&
  vslot56(layer, &TIME_REMAP_name)) { stream = vslot65(layer, name, 1); ...
  HasNoKeys → identity }` else `*out = *in`. The time-remap branch (stream
  objects, `__RTDynamicCast`) is only entered when both answers are true.
- U.dll `T_GeneralFormatTime(T_Time*, fmt*, type, fps_fixed, bool, char*, size)`
  formats through `T_TimeToComponents`; the only global it consults is
  `T_GetMaxFPS()` (mutex-guarded static, 999 in AE). No AE-only initialization
  was found on this path.

### 2.3 Vtable sizes (from the RTTI-adjacent layout)

BEE_AVLayer 246 slots (`BEE.dll+0xf79128`), BEE_CompItem 15
(`+0xfdf9b8`), BEE_FootageItem 15 (`+0xfe07b8`), BEE_Project 8 (`+0xfe4b20`).
Full slot-name tables in the appendix.

## 3. Dynamic observation: Timecode RENDER in AE 2026 (Frida)

Capture: `tools/capture-ae-reference.ps1` (AfterFX.com `-m -noui -r`, AE
26.3x87), 256x144 PNG footage on one comp layer, `ADBE Timecode` with default
parameters, frame 0, 30 fps, 300 frames, 8 bpc; Frida attached to the
AfterFX.com process, hooks on the BEE/U exports above (filtered to callers
inside Timecode.aex), on the layer vtable slots, and on `Timecode.aex+0x61a0`
(`tools/frida/timecode_bee_scene_hook.js`; how it was attached is in
`tools/frida/README.md`).
Reference output: `00:00:00:00` in a black box (`output_png_sha256
e7bd0b90…67eac`, input `db1b8e9b…48ac7d`).

Call sequence and values (one RENDER, `in_data->current_time = 0`,
`time_scale = 30720`, `time_step = 307200`):

1. `BEE_Item::GetParentProject(*(layer + 0x260))` — the argument is the
   **parent comp item** (`BEE_CompItem`, tag `0xBEE1`, type 4, flags `0x20`);
   returns `&item->project` (`BEE_Project`).
2. `BEE_GetProjectTimeFormat(project, &fmt)` — `project + 0x84` =
   `01 01 00 00 | 1e000000 | 10000000 | 02 000000` → `fmt` =
   `00 01 01 00 | 1e000000 | 00000000 | 02000000 | 00…`.
3. `BEE_GetSourceTimeFormat(layer, true, &fps = 30<<16, &fmt)` →
   `BEE_GetSourceMediaInfo(comp item)` → `BEE_GetCompSettings` (ret 0):
   `cs = {item, T_Time{307200,30720}, T_Time{0,1}, w=256 h=144 fps=0x1e0000,
   {1,1}, i32 0x10001, i32 0, {180,360}, {0,360}, 128, 16, dropframe 0}`;
   `info.T_Time@0 = {0,1}`, `has = 1`; fps getter → `0x1e0000`; result
   `fmt = 01 01 00 00 | 1e000000 | 00000000 | 02000000 | 00000000 01000000`.
4. vslot 183 `BEE_AVLayer::pGetSourceItem(layer, nullptr)` (twice) → the
   **source item** (`BEE_FootageItem`, tag `0xBEE1`, type 7, flags `0x30`,
   same project). `BEE_Item::GetFlags` → `0x30`; bit `0x10` set, so Timecode
   skips `BEE_GetSourceTimeFormat(layer, false, …)`.
5. `BEE_LayerToSourceTime(layer, {0,30720}, &out, nullptr)`: vslot 79
   `IsLayerType(0)` → true; vslot 56 `TDB_StreamGroup::CanStore(TIME_REMAP)`
   → 1; vslot 65 `GetStream(TIME_REMAP, 1)` → stream; `HasNoKeys` → identity;
   `out = {0,30720}`, ret 0.
6. U.dll `T_GeneralFormatTime({0,30720}, fmt, 0, 30<<16, true, buf, 128)` →
   `"00:00:00:00"`. `T_GetMaxFPS()` = 999.
7. RENDER returns 0.

Raw object bytes read for the record (AE 2026, this comp; meaning of the
unnamed words is not claimed):

- `BEE_AVLayer` (RTTI `.?AVBEE_AVLayer@@`) `+0x240..0x2a0`:
  `0…0 | +0x258 i32 7 | +0x260 comp item ptr | +0x268 0 | +0x26c ffffffff |
  +0x270 {0,30720} | +0x278 {307200,30720} | +0x280 {0,30720} | +0x288 {1,1} |
  +0x290 i32 7 | 0…`. Source item pointer at `+0x2720`.
- `BEE_CompItem` `+0x260..0x2c0`: `0 | 0x00010001 | {1,1} | {1024,30720} |
  {0,600} | {30720,0} | {600,ffffffff} | {600, …} | +0x298 {307200,30720} |
  +0x2a0 {0,1} | ff000000 | … | +0x2b8 i16 256, +0x2ba i16 144, +0x2bc 0x1e0000`;
  `+0x300..0x310: 180, 360, 0, 360`; `+0x418 128, +0x41c 16, +0x424 0`.
- `BEE_Project` (RTTI `.?AVBEE_Project@@`) `+0x84`:
  `01 01 00 00 1e 00 00 00 10 00 00 00 02 00 00 00`.
- `BEE_FootageItem`: `+0x2f8 -> shared_ptr<PIN_SeqSpec>`; the PIN block had
  `+0x1f0 = 0` (still image: no fps), `+0x1f4 = 1`, `+0x1f5 = 0`.

## 4. Host facade (implementation, `worker_bee_scene_facade.*`)

The handle `AEGP_GetEffectLayer` returns (`&g_layer`) is now a
`bee_facade::LayerObject` (0x2740 bytes) with the observed layout, linked to
one comp `ItemObject` (0x430 bytes), one footage `ItemObject` and one
`ProjectObject` (0xa0 bytes), all process-lifetime statics.
`prepare_effect_layer` runs at every hand-out and publishes the host's scene
contract (the same values the AEGP item/comp suites report):

| object / field | value | source |
| --- | --- | --- |
| layer `+0x000` vtable | 246 slots | facade vtable |
| layer `+0x260` | comp item | fixed graph |
| layer `+0x2720` | footage item | fixed graph |
| layer vslot 79 `IsLayerType(t)` | `t == 0` | BEE.dll body |
| layer vslot 56 `CanStore(name)` | false | host layers hold no dynamic streams; makes `BEE_LayerToSourceTime` return the input time, the same result AE produced through the has-no-keys branch (AE itself answered true and then found no keys, so a caller consulting `CanStore` for its own purposes gets a different answer than AE) |
| layer vslot 65 `GetStream(name, mode)` | null | only reachable after CanStore |
| layer vslot 183 / 184 | footage item | field `+0x2720`, as BEE.dll's own body for a null parameter bag; the bag argument is ignored (BEE.dll would consult slot 244 for a non-null bag) |
| item `+0x008` | `0xBEE1` | BEE.dll item check |
| item `+0x038` | project | |
| item `+0x048` | 4 (comp) / 7 (footage) | |
| item `+0x04c` | `0x20` (comp) / `0x30` (footage) | observed values; footage bit `0x10` keeps the source-time-format lookup off, as in AE for the still-footage layer |
| comp `+0x298` duration | `{300, 30}` | `AEGP_GetItemDuration` |
| comp `+0x2a0` display start | `{0, 1}` | observed |
| comp `+0x2b8/+0x2ba` | render context width/height (int16, clamped to 32767; the item suite itself admits up to 32768, so that one value diverges) | `AEGP_GetItemDimensions` |
| comp `+0x2bc` | fps `30 << 16` | `AEGP_GetCompFramerate` |
| comp `+0x418/+0x41c` | 128 / 16 | AE defaults observed |
| comp `+0x424` | 0 | no drop-frame display |
| project `+0x84` | `01 01 00 00 | fps | 16 | 02` | observed block, fps from the contract |
| every other vtable slot | identifying trap | records `BEE_AVLayer vtable` / `BEE_CompItem vtable` / `BEE_FootageItem vtable` / `BEE_Project vtable` slot N in `unsupported_suite_calls` (`stage:suite_slot_unsupported` line), writes `stage:bee_facade_trap object=<BEE_AVLayer|BEE_CompItem|BEE_FootageItem|BEE_Project> slot=N caller=<module>+0x<rva>` (the caller is what names the BEE.dll export path or the plug-in), then raises `0xE0428000 + range + slot` (layer 0x000, comp item 0x100, footage item 0x140, project 0x180). When raised on the dispatch thread and not swallowed by a handler in between, the selector SEH containment fails the frame with `stage:selector_seh ... code=0xe0428xxx site=other_module module=KERNELBASE.dll` (RaiseException's own address); the identification is the recorded slot and caller, not the site |
| `AE Timecode Helper Suite` v1 | 32 diagnosed unsupported slots (record + return 4) | acquire/release gate only; 32 is a bound with no slot ever observed called, and a caller indexing past it would land in the next table of the catalog and be attributed to that suite |

Not a trap: the footage-item media path. `BEE_GetSourceMediaInfo` on the
footage item (reached only through `BEE_GetSourceTimeFormat(layer, false, …)`,
which Timecode skips because of flag `0x10`) dereferences the
`shared_ptr<PIN_SeqSpec>` storage at `item + 0x2f8`, which the facade leaves
zero; a plug-in that takes that branch faults inside BEE.dll and is reported
as an access violation at BEE.dll, not as a named slot.

Every field the observation did not reach is zero. Handles issued through the
scene registry (borrowed handles, comp layer enumeration) are unchanged;
extending the facade to those is issue #1264.

### 4.1 Result

`render_sweep --filter Timecode` (AE 2026 Effects folder, 304 seen / 1
matched, 256x144 ARGB8, t=0, 1 frame): `frame_error:516` before,
`rendered` after; `output_sha256 != input_sha256`, not `rendered_empty`,
`unsupported_suite_calls = []` (no trap taken), the world dump shows
`00:00:00:00` in a black box at the same position as AE.

The facade publishes the host contract's fixed 30 fps / 300 frames (what
`AEGP_GetCompFramerate` / `AEGP_GetItemDuration` report) regardless of the
`in_data` time base a render request carries; a request with another
`time_scale` / `time_step` therefore formats against 30 fps. That is the
existing host contract, not a facade property.

Remaining AE-equivalence gap (observation, not addressed here): the glyphs
are about 16 % larger than AE's (host text extents 218x29 px vs AE 187x25 px
for the same string, box height 44 vs 56). Timecode obtains its font family
and style from `AEGP Persistent Data Suite` ("Font Preferences" / "Standard
Font Family|Style") with the plug-in's string table as fallback ("Arial",
"Regular" on Windows) and draws through TXT.dll's font server; the host
answers the persistent-data lookup with no stored preference, so the fallback
family is used, while AE used the machine's preference. Filed as issue #1263.

## 5. Log

- 2026-08-17 (this record): static decompile of the five BEE exports and
  the U.dll formatting export; dynamic Frida capture in AE 26.3 (one run,
  default parameters); facade implemented; Timecode renders on the host.
  Not covered: `BEE_GetSourceTimeFormat(false)` footage path (PIN_SeqSpec),
  the time-remap stream branch of `BEE_LayerToSourceTime`, any other
  BEE-importing effect (ColorShift, Contrast, Exposure, Flare, Lumetri,
  OCIO*, ProfileToProfile, PSL_Adjustments, ShapeBlur — whether they read
  AEGP handles as BEE objects at all is unverified).

## Appendix: vtable slot names (BEE.dll exports)

Unnamed entries are BEE.dll-internal functions without an export symbol
(module-relative address given).

### BEE_AVLayer vtable (BEE.dll+0xf79128)

| slot | offset | symbol |
| --- | --- | --- |
| 0 | 0x0000 | `(unnamed, BEE.dll+0x25e060)` |
| 1 | 0x0008 | `BEE_AVLayer::CloneStream` |
| 2 | 0x0010 | `(unnamed, BEE.dll+0xc52dd8)` |
| 3 | 0x0018 | `BEE_Layer::GetCapsuleStreamID` |
| 4 | 0x0020 | `(unnamed, BEE.dll+0xc53024)` |
| 5 | 0x0028 | `(unnamed, BEE.dll+0xc5317a)` |
| 6 | 0x0030 | `(unnamed, BEE.dll+0xc52fd6)` |
| 7 | 0x0038 | `(unnamed, BEE.dll+0xc52fdc)` |
| 8 | 0x0040 | `(unnamed, BEE.dll+0xc52dde)` |
| 9 | 0x0048 | `(unnamed, BEE.dll+0xc52efe)` |
| 10 | 0x0050 | `(unnamed, BEE.dll+0xc5318c)` |
| 11 | 0x0058 | `(unnamed, BEE.dll+0xc53192)` |
| 12 | 0x0060 | `(unnamed, BEE.dll+0xc53198)` |
| 13 | 0x0068 | `BEE_Layer::IsLoading` |
| 14 | 0x0070 | `BEE_Layer::ProbeListenerState` |
| 15 | 0x0078 | `(unnamed, BEE.dll+0xc531aa)` |
| 16 | 0x0080 | `(unnamed, BEE.dll+0xc531b0)` |
| 17 | 0x0088 | `(unnamed, BEE.dll+0xc52ffa)` |
| 18 | 0x0090 | `(unnamed, BEE.dll+0xc53000)` |
| 19 | 0x0098 | `(unnamed, BEE.dll+0xc531c2)` |
| 20 | 0x00a0 | `(unnamed, BEE.dll+0xc531c8)` |
| 21 | 0x00a8 | `(unnamed, BEE.dll+0xc52eda)` |
| 22 | 0x00b0 | `(unnamed, BEE.dll+0xc52ee0)` |
| 23 | 0x00b8 | `BEE_Layer::GetProject` |
| 24 | 0x00c0 | `(unnamed, BEE.dll+0xc52c9a)` |
| 25 | 0x00c8 | `(unnamed, BEE.dll+0xc531e0)` |
| 26 | 0x00d0 | `(unnamed, BEE.dll+0xc531e6)` |
| 27 | 0x00d8 | `(unnamed, BEE.dll+0xc531ec)` |
| 28 | 0x00e0 | `(unnamed, BEE.dll+0xc531f2)` |
| 29 | 0x00e8 | `(unnamed, BEE.dll+0xc531f8)` |
| 30 | 0x00f0 | `(unnamed, BEE.dll+0xc531fe)` |
| 31 | 0x00f8 | `(unnamed, BEE.dll+0xc53204)` |
| 32 | 0x0100 | `(unnamed, BEE.dll+0xc53012)` |
| 33 | 0x0108 | `(unnamed, BEE.dll+0xc52f16)` |
| 34 | 0x0110 | `(unnamed, BEE.dll+0xc5320a)` |
| 35 | 0x0118 | `(unnamed, BEE.dll+0xc52d4e)` |
| 36 | 0x0120 | `(unnamed, BEE.dll+0xc53210)` |
| 37 | 0x0128 | `(unnamed, BEE.dll+0xc52fe8)` |
| 38 | 0x0130 | `(unnamed, BEE.dll+0xc52fee)` |
| 39 | 0x0138 | `(unnamed, BEE.dll+0xc5321c)` |
| 40 | 0x0140 | `(unnamed, BEE.dll+0xc53222)` |
| 41 | 0x0148 | `(unnamed, BEE.dll+0xc53030)` |
| 42 | 0x0150 | `(unnamed, BEE.dll+0xc53036)` |
| 43 | 0x0158 | `(unnamed, BEE.dll+0xc5322e)` |
| 44 | 0x0160 | `(unnamed, BEE.dll+0xc53234)` |
| 45 | 0x0168 | `(unnamed, BEE.dll+0xc52e74)` |
| 46 | 0x0170 | `(unnamed, BEE.dll+0xc52e7a)` |
| 47 | 0x0178 | `(unnamed, BEE.dll+0xc52e86)` |
| 48 | 0x0180 | `(unnamed, BEE.dll+0xc52e8c)` |
| 49 | 0x0188 | `(unnamed, BEE.dll+0xc52e92)` |
| 50 | 0x0190 | `(unnamed, BEE.dll+0xc52ea4)` |
| 51 | 0x0198 | `(unnamed, BEE.dll+0xc52eaa)` |
| 52 | 0x01a0 | `(unnamed, BEE.dll+0xc52ebc)` |
| 53 | 0x01a8 | `(unnamed, BEE.dll+0xc52ec2)` |
| 54 | 0x01b0 | `(unnamed, BEE.dll+0xc52fe2)` |
| 55 | 0x01b8 | `(unnamed, BEE.dll+0xc52ef8)` |
| 56 | 0x01c0 | `TDB_StreamGroup::CanStore` |
| 57 | 0x01c8 | `BEE_AVLayer::FillInCanonicalUILayout` |
| 58 | 0x01d0 | `BEE_Layer::NotifyStreamStored` |
| 59 | 0x01d8 | `(unnamed, BEE.dll+0xc52f0a)` |
| 60 | 0x01e0 | `(unnamed, BEE.dll+0xc52f10)` |
| 61 | 0x01e8 | `(unnamed, BEE.dll+0xc53018)` |
| 62 | 0x01f0 | `BEE_Layer::GetPreferredTimeScale` |
| 63 | 0x01f8 | `BEE_AVLayer::EvaluateExpression` |
| 64 | 0x0200 | `(unnamed, BEE.dll+0xc52e80)` |
| 65 | 0x0208 | `TDB_NamedStreamGroup::GetStream` |
| 66 | 0x0210 | `(unnamed, BEE.dll+0xc52e9e)` |
| 67 | 0x0218 | `(unnamed, BEE.dll+0xc52eb0)` |
| 68 | 0x0220 | `(unnamed, BEE.dll+0xc52eb6)` |
| 69 | 0x0228 | `(unnamed, BEE.dll+0xc52ec8)` |
| 70 | 0x0230 | `(unnamed, BEE.dll+0xc52ece)` |
| 71 | 0x0238 | `(unnamed, BEE.dll+0xc52ed4)` |
| 72 | 0x0240 | `(unnamed, BEE.dll+0xc52ee6)` |
| 73 | 0x0248 | `(unnamed, BEE.dll+0xc52eec)` |
| 74 | 0x0250 | `(unnamed, BEE.dll+0xc52ef2)` |
| 75 | 0x0258 | `BEE_AVLayer::UserCanHideAndShowChildren` |
| 76 | 0x0260 | `BEE_AVLayer::RenderGuidDirectlyMixesIn` |
| 77 | 0x0268 | `BEE_AVLayer::GetRenderGuidWithRO` |
| 78 | 0x0270 | `BEE_AVLayer::IsROInGuidCache` |
| 79 | 0x0278 | `BEE_AVLayer::IsLayerType` |
| 80 | 0x0280 | `BEE_AVLayer::GetLayerType` |
| 81 | 0x0288 | `BEE_Layer::GetSoloLayerType` |
| 82 | 0x0290 | `BEE_AVLayer::SourceIsComp` |
| 83 | 0x0298 | `BEE_AVLayer::CmdCopyMarkersFromSource` |
| 84 | 0x02a0 | `BEE_Layer::SetupContextualLayerDefaults` |
| 85 | 0x02a8 | `BEE_Layer::CmdPostLayerFlagChange` |
| 86 | 0x02b0 | `BEE_AVLayer::HasUnlimitedInOutRange` |
| 87 | 0x02b8 | `BEE_Layer::GetLayerFlags` |
| 88 | 0x02c0 | `BEE_Layer::SetLayerFlags` |
| 89 | 0x02c8 | `BEE_AVLayer::GetDefaultLayerFlags` |
| 90 | 0x02d0 | `BEE_Layer::GetDefaultLabelID` |
| 91 | 0x02d8 | `BEE_AVLayer::GetDefaultTypeMagic` |
| 92 | 0x02e0 | `BEE_AVLayer::AllowsSampleQuality` |
| 93 | 0x02e8 | `BEE_AVLayer::GetQuality` |
| 94 | 0x02f0 | `BEE_AVLayer::SetQuality` |
| 95 | 0x02f8 | `BEE_AVLayer::GetXferMode` |
| 96 | 0x0300 | `BEE_AVLayer::SetXferMode` |
| 97 | 0x0308 | `BEE_AVLayer::GetTrackMatteLayerID` |
| 98 | 0x0310 | `BEE_AVLayer::SetTrackMatteLayerID` |
| 99 | 0x0318 | `BEE_AVLayer::GetLoop` |
| 100 | 0x0320 | `BEE_AVLayer::SetLoop` |
| 101 | 0x0328 | `BEE_AVLayer::GetSourceID` |
| 102 | 0x0330 | `BEE_AVLayer::GetMaskParade` |
| 103 | 0x0338 | `BEE_AVLayer::GetMaskParade` |
| 104 | 0x0340 | `BEE_AVLayer::GetFilterParade` |
| 105 | 0x0348 | `BEE_AVLayer::GetFilterParade` |
| 106 | 0x0350 | `BEE_AVLayer::GetMaskFreeXform` |
| 107 | 0x0358 | `BEE_AVLayer::GetMaskCacheDataPtr` |
| 108 | 0x0360 | `BEE_AVLayer::GetMaskCachePtr` |
| 109 | 0x0368 | `BEE_AVLayer::GetOriginPoint` |
| 110 | 0x0370 | `BEE_AVLayer::SetOriginPoint` |
| 111 | 0x0378 | `BEE_AVLayer::HasAnchorPoint` |
| 112 | 0x0380 | `BEE_AVLayer::IsAnchorPointAbsolute` |
| 113 | 0x0388 | `BEE_Layer::GetLightType` |
| 114 | 0x0390 | `BEE_Layer::SetLightType` |
| 115 | 0x0398 | `BEE_Layer::GetCameraType` |
| 116 | 0x03a0 | `BEE_Layer::SetCameraType` |
| 117 | 0x03a8 | `BEE_Layer::GetFilmSizeUnits` |
| 118 | 0x03b0 | `BEE_Layer::SetFilmSizeUnits` |
| 119 | 0x03b8 | `BEE_Layer::GetFilmSize` |
| 120 | 0x03c0 | `BEE_Layer::SetFilmSize` |
| 121 | 0x03c8 | `BEE_Layer::ReplaceGuides` |
| 122 | 0x03d0 | `BEE_Layer::CreateNewGuides` |
| 123 | 0x03d8 | `BEE_Layer::DisposeGuides` |
| 124 | 0x03e0 | `BEE_Layer::ReplaceComment` |
| 125 | 0x03e8 | `BEE_Layer::DisposeComment` |
| 126 | 0x03f0 | `BEE_Layer::GetDefaultInPoint` |
| 127 | 0x03f8 | `BEE_Layer::GetDefaultOutPoint` |
| 128 | 0x0400 | `BEE_Layer::GetDefaultTimeOffset` |
| 129 | 0x0408 | `BEE_AVLayer::ReplaceMaskFreeXform` |
| 130 | 0x0410 | `BEE_AVLayer::AcquireMaskFreeXform` |
| 131 | 0x0418 | `BEE_AVLayer::DisposeMaskFreeXform` |
| 132 | 0x0420 | `BEE_AVLayer::GetDimensions` |
| 133 | 0x0428 | `BEE_AVLayer::GetLayerDepth` |
| 134 | 0x0430 | `BEE_Layer::HasBoundingBox` |
| 135 | 0x0438 | `BEE_Layer::ArtisanSupportsBoundingBoxDepth` |
| 136 | 0x0440 | `BEE_Layer::DefaultSizedBoundingBoxCompTime` |
| 137 | 0x0448 | `BEE_AVLayer::GetBoundingBoxCompTime` |
| 138 | 0x0450 | `BEE_Layer::GetBoundingBoxPtr` |
| 139 | 0x0458 | `BEE_Layer::GetRelativeDimensions` |
| 140 | 0x0460 | `BEE_Layer::GetRelativeFloatRect` |
| 141 | 0x0468 | `BEE_Layer::SupportsDynamicBounds` |
| 142 | 0x0470 | `BEE_AVLayer::GetSourceFloatRect` |
| 143 | 0x0478 | `BEE_Layer::GetSourceFloatRectForExpression` |
| 144 | 0x0480 | `BEE_AVLayer::GetCheckoutDimensions` |
| 145 | 0x0488 | `BEE_AVLayer::GetSourceRectAccountingForSubLayerMotionBlur` |
| 146 | 0x0490 | `BEE_AVLayer::HasSource` |
| 147 | 0x0498 | `BEE_AVLayer::HasData` |
| 148 | 0x04a0 | `BEE_AVLayer::HasDataOnly` |
| 149 | 0x04a8 | `BEE_AVLayer::IsDataAnimatable` |
| 150 | 0x04b0 | `BEE_Layer::IsDataAnimated` |
| 151 | 0x04b8 | `BEE_AVLayer::IsSpreadsheet` |
| 152 | 0x04c0 | `BEE_AVLayer::HasAudio` |
| 153 | 0x04c8 | `BEE_AVLayer::HasVideo` |
| 154 | 0x04d0 | `BEE_AVLayer::CanHaveEffects` |
| 155 | 0x04d8 | `BEE_AVLayer::CanHaveExpressionOrPseudoEffects` |
| 156 | 0x04e0 | `BEE_AVLayer::CanHaveMasks` |
| 157 | 0x04e8 | `BEE_AVLayer::CanFrameBlend` |
| 158 | 0x04f0 | `BEE_AVLayer::CanFlatten` |
| 159 | 0x04f8 | `BEE_AVLayer::CanChangeFlattenSwitch` |
| 160 | 0x0500 | `BEE_AVLayer::CanShow3DSwitch` |
| 161 | 0x0508 | `BEE_AVLayer::CanChange3DSwitch` |
| 162 | 0x0510 | `BEE_AVLayer::CanUseParenting` |
| 163 | 0x0518 | `BEE_AVLayer::HasOwnGeometrics` |
| 164 | 0x0520 | `BEE_AVLayer::SubLayersLookAtCamera` |
| 165 | 0x0528 | `BEE_Layer::ReloadDataStreams` |
| 166 | 0x0530 | `BEE_Layer::AddDataStreams` |
| 167 | 0x0538 | `BEE_Layer::AddDataSpreadsheetStreams` |
| 168 | 0x0540 | `BEE_Layer::GetDataSpreadsheet_NumRows` |
| 169 | 0x0548 | `BEE_Layer::CreateDataStreams` |
| 170 | 0x0550 | `BEE_AVLayer::GetSubLayerLocalMatrix3D` |
| 171 | 0x0558 | `BEE_Layer::GetStreamDimension` |
| 172 | 0x0560 | `BEE_AVLayer::UpdateStreamVisibility` |
| 173 | 0x0568 | `BEE_Layer::FixVersionDependantDefaultStreamValues` |
| 174 | 0x0570 | `BEE_AVLayer::CollectLayerDependencies` |
| 175 | 0x0578 | `BEE_Layer::CloneAndAttach` |
| 176 | 0x0580 | `BEE_Layer::ReadPlus` |
| 177 | 0x0588 | `BEE_AVLayer::PostReadFixup` |
| 178 | 0x0590 | `BEE_AVLayer::ReadFilterParade` |
| 179 | 0x0598 | `BEE_AVLayer::ReadPreV55MaskParade` |
| 180 | 0x05a0 | `BEE_Layer::SupportsMaterialParade` |
| 181 | 0x05a8 | `BEE_Layer::WritePlus` |
| 182 | 0x05b0 | `BEE_AVLayer::pSetSourceID` |
| 183 | 0x05b8 | `BEE_AVLayer::pGetSourceItem` |
| 184 | 0x05c0 | `BEE_AVLayer::pGetConstSourceItem` |
| 185 | 0x05c8 | `BEE_AVLayer::pSetSourceItem` |
| 186 | 0x05d0 | `BEE_Layer::CmdPreParamChange` |
| 187 | 0x05d8 | `BEE_Layer::CmdPreParamPasteChange` |
| 188 | 0x05e0 | `BEE_AVLayer::CmdPostParamPasteChange` |
| 189 | 0x05e8 | `BEE_AVLayer::CmdParamChanged` |
| 190 | 0x05f0 | `BEE_AVLayer::CmdPreParamBaseFlagsChange` |
| 191 | 0x05f8 | `BEE_Layer::RemovingIndexedGroup` |
| 192 | 0x0600 | `BEE_AVLayer::TouchStream` |
| 193 | 0x0608 | `BEE_Layer::HasEffectApplied` |
| 194 | 0x0610 | `BEE_Layer::HasEffectApplied_AtIndex` |
| 195 | 0x0618 | `BEE_Layer::OnLeavingComp` |
| 196 | 0x0620 | `BEE_Layer::Appearing` |
| 197 | 0x0628 | `BEE_AVLayer::FillInDiskData` |
| 198 | 0x0630 | `BEE_AVLayer::SetupFromDiskData` |
| 199 | 0x0638 | `BEE_Layer::MixInGuidForTransform` |
| 200 | 0x0640 | `BEE_AVLayer::MixInGuidForTransformNonRecursive` |
| 201 | 0x0648 | `BEE_Layer::Init` |
| 202 | 0x0650 | `BEE_AVLayer::SetMaskFreeXform` |
| 203 | 0x0658 | `BEE_AVLayer::GetSampleQuality` |
| 204 | 0x0660 | `BEE_AVLayer::GetExplicitSampleQuality` |
| 205 | 0x0668 | `BEE_AVLayer::UsesLayerStyles` |
| 206 | 0x0670 | `BEE_AVLayer::CanConvertStyles` |
| 207 | 0x0678 | `BEE_AVLayer::GetLayerOverrides` |
| 208 | 0x0680 | `BEE_AVLayer::GetLayerOverrides` |
| 209 | 0x0688 | `BEE_AVLayer::GetLayerSets` |
| 210 | 0x0690 | `BEE_AVLayer::GetLayerSets` |
| 211 | 0x0698 | `BEE_AVLayer::IsExtrudable` |
| 212 | 0x06a0 | `BEE_AVLayer::Is3DPlane` |
| 213 | 0x06a8 | `BEE_AVLayer::Is3DText` |
| 214 | 0x06b0 | `BEE_AVLayer::Is3DVector` |
| 215 | 0x06b8 | `BEE_AVLayer::Is3DCube` |
| 216 | 0x06c0 | `BEE_AVLayer::Is3DSphere` |
| 217 | 0x06c8 | `BEE_AVLayer::Is3DCylinder` |
| 218 | 0x06d0 | `BEE_AVLayer::CanSubLayersRenderSeparately` |
| 219 | 0x06d8 | `BEE_AVLayer::SetSubLayersRenderSeparately` |
| 220 | 0x06e0 | `BEE_AVLayer::SubLayersRenderSeparately` |
| 221 | 0x06e8 | `BEE_AVLayer::SharedXformTimeCacheFactory` |
| 222 | 0x06f0 | `BEE_AVLayer::SubLayerRenderCount` |
| 223 | 0x06f8 | `BEE_AVLayer::GetSubLayerRenderBounds` |
| 224 | 0x0700 | `BEE_AVLayer::GetSubLayerXform2D` |
| 225 | 0x0708 | `BEE_AVLayer::GetSubLayerXform2D` |
| 226 | 0x0710 | `BEE_AVLayer::GetSubLayerXform2D` |
| 227 | 0x0718 | `BEE_AVLayer::GetSubLayerXform2D` |
| 228 | 0x0720 | `BEE_AVLayer::GetSubLayerXform3D` |
| 229 | 0x0728 | `BEE_AVLayer::GetSubLayerOpacity` |
| 230 | 0x0730 | `BEE_AVLayer::GetSubLayerBlur` |
| 231 | 0x0738 | `BEE_AVLayer::GetSubLayerSourceRectFromCache` |
| 232 | 0x0740 | `BEE_AVLayer::GetBoundsFromCache` |
| 233 | 0x0748 | `BEE_AVLayer::CmdResetStreamsForSubLayerDimensionChange` |
| 234 | 0x0750 | `BEE_AVLayer::GetLayerFront` |
| 235 | 0x0758 | `BEE_AVLayer::GetLayerBack` |
| 236 | 0x0760 | `BEE_AVLayer::GetSubLayerRects` |
| 237 | 0x0768 | `BEE_AVLayer::HasSubLayerXform` |
| 238 | 0x0770 | `BEE_AVLayer::Rasterize2DGraph` |
| 239 | 0x0778 | `BEE_AVLayer::CanRasterizeOverPreviousLayer` |
| 240 | 0x0780 | `BEE_AVLayer::NewXformCache2D` |
| 241 | 0x0788 | `BEE_AVLayer::UpdateCommon3DStreamGroupVisibility` |
| 242 | 0x0790 | `BEE_AVLayer::MaskShapeAffectsRendering` |
| 243 | 0x0798 | `BEE_AVLayer::pGetAlternateSourceItem` |
| 244 | 0x07a0 | `BEE_AVLayer::pGetConstAlternateSourceItem` |
| 245 | 0x07a8 | `BEE_AVLayer::GetFullPath` |

### BEE_CompItem vtable (BEE.dll+0xfdf9b8)

| slot | offset | symbol |
| --- | --- | --- |
| 0 | 0x0000 | `BEE_Item::pGetNewXMPMetaManager` |
| 1 | 0x0008 | `(unnamed, BEE.dll+0x39c9e0)` |
| 2 | 0x0010 | `BEE_Item::GetXMPMetaManager` |
| 3 | 0x0018 | `BEE_Item::CanServeAsXMPMetaProvider` |
| 4 | 0x0020 | `BEE_Item::CmdSetName` |
| 5 | 0x0028 | `BEE_Item::pSetName` |
| 6 | 0x0030 | `BEE_Item::GetName` |
| 7 | 0x0038 | `BEE_CompItem::pActuallyPreRenderXMP` |
| 8 | 0x0040 | `BEE_CompItem::pActuallyRenderXMP` |
| 9 | 0x0048 | `BEE_CompItem::GetRenderGuidWithRO` |
| 10 | 0x0050 | `BEE_CompItem::IsROInGuidCache` |
| 11 | 0x0058 | `BEE_Item::GetItemMixInGuidWithRO` |
| 12 | 0x0060 | `BEE_Item::GetItemGuidWithRO` |
| 13 | 0x0068 | `BEE_Item::GetStreamFactory` |
| 14 | 0x0070 | `BEE_CompItem::GetExpressionRef` |

### BEE_FootageItem vtable (BEE.dll+0xfe07b8)

| slot | offset | symbol |
| --- | --- | --- |
| 0 | 0x0000 | `BEE_FootageItem::pGetNewXMPMetaManager` |
| 1 | 0x0008 | `(unnamed, BEE.dll+0x3d0580)` |
| 2 | 0x0010 | `BEE_Item::GetXMPMetaManager` |
| 3 | 0x0018 | `BEE_FootageItem::CanServeAsXMPMetaProvider` |
| 4 | 0x0020 | `BEE_Item::CmdSetName` |
| 5 | 0x0028 | `BEE_FootageItem::pSetName` |
| 6 | 0x0030 | `BEE_FootageItem::GetName` |
| 7 | 0x0038 | `BEE_Item::pActuallyPreRenderXMP` |
| 8 | 0x0040 | `BEE_FootageItem::pActuallyRenderXMP` |
| 9 | 0x0048 | `BEE_FootageItem::GetRenderGuidWithRO` |
| 10 | 0x0050 | `BEE_FootageItem::IsROInGuidCache` |
| 11 | 0x0058 | `BEE_Item::GetItemMixInGuidWithRO` |
| 12 | 0x0060 | `BEE_Item::GetItemGuidWithRO` |
| 13 | 0x0068 | `BEE_Item::GetStreamFactory` |
| 14 | 0x0070 | `BEE_FootageItem::SetPinSeqPtr` |

### BEE_Project vtable (BEE.dll+0xfe4b20)

| slot | offset | symbol |
| --- | --- | --- |
| 0 | 0x0000 | `(unnamed, BEE.dll+0x410410)` |
| 1 | 0x0008 | `BEE_Project::GetColorProfilePool` |
| 2 | 0x0010 | `BEE_Project::GetColorSettings` |
| 3 | 0x0018 | `BEE_Project::GetColorSettings` |
| 4 | 0x0020 | `BEE_Project::IsModifyable` |
| 5 | 0x0028 | `BEE_Project::IsAnywhereProduction` |
| 6 | 0x0030 | `BEE_Project::GetAnywhereSandboxIdentifier` |
| 7 | 0x0038 | `BEE_Project::IsAELibInProcess` |
