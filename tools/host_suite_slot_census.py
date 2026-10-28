#!/usr/bin/env python3
"""Reproduce the X06 source census, not a behavioral test or an SDK ABI oracle.

Extraction is intentionally specific to the audited wiring at cb3bb768 plus X06.
Annotations are reviewed interpretations of callback bodies, not inferred from
suite presence. Re-review them before using this script on a different baseline.
No SDK headers, plug-in bytes, or machine-local paths are embedded in the output.
"""
from pathlib import Path
import re
import collections

ROOT = Path(__file__).resolve().parents[1]
N = 'minihost/src/'
G = 'guest/crates/aex-guest-worker/src/'
W = N + 'worker_host_suite_wiring.cpp'
S = N + 'worker_aegp_scene.cpp'
I = G + 'x64/iterate_and_suites.rs'
U = G + 'x64/ui_session.rs'
rows = []


def read(path):
    return (ROOT / path).read_text(encoding='utf-8')


def ref(path, needle=None):
    text = read(path)
    offset = text.find(needle) if needle else 0
    if offset < 0:
        raise ValueError(f'Missing evidence locator: {path}: {needle}')
    line = text.count('\n', 0, offset) + 1
    return f'[{Path(path).name}:{line}](../{path}#L{line})'


def add(group, suite, version, slot, callback, win, guest, evidence):
    rows.append((group, suite, version, slot, callback, win, guest, evidence))


def callbacks(path, table):
    text = read(path)
    body = re.search(r'\b' + re.escape(table) + r'\s*\{([^}]+)\}', text, re.S).group(1)
    return re.findall(r'&([\w:]+)', body)


def struct_members(path, name):
    body = re.search(r'struct\s+' + name + r'\s*\{(.*?)\n\};', read(path), re.S).group(1)
    return re.findall(r'\b(\w+)\s*;', body)


# Exact numeric versions are acquisition versions, not C++ struct suffixes.
p = N + 'worker_pf_param_suites.hpp'
for suite, symbol in [('PF PointParamSuite', 'floating_point_from_point'), ('PF AngleParamSuite', 'floating_point_from_angle'), ('PF ColorParamSuite', 'floating_point_from_color')]:
    guest = 'no: suite absent [P1]' if 'Angle' in suite else 'partial: bounded parameter conversion [P1]'
    add('Parameters and ANSI', suite, 1, 0, symbol, 'yes', guest,
        ref(p, symbol) + '; ' + ref(I, 'fn emulate_color_param_value') if 'Color' in suite else ref(p, symbol) + '; ' + ref(I, 'fn emulate_acquire_suite'))
for version, name in [(2, 'ParamUtilsSuite1'), (3, 'ParamUtilsSuite3')]:
    for slot, name in enumerate(struct_members(p, name)):
        guest = 'partial: UI dispatch only [P1]' if version == 3 and slot == 0 else ('no: UI callback error [P1]' if version == 3 else 'no: suite absent [P1]')
        add('Parameters and ANSI', 'PF Param Utils Suite', version, slot, name, 'partial: bounded parameter/keyframe state', guest, ref(p, 'struct ParamUtilsSuite') + '; ' + ref(U, 'fn emulate_ui_callback'))
ansi = 'atan atan2 ceil cos exp fabs floor fmod hypot log log10 pow sin sqrt tan sprintf strcpy asin acos'.split()
for version in [1, 2]:
    for slot, name in enumerate(ansi + ([] if version == 1 else ['unknown/null', 'strcpy_bounded'])):
        win = 'no: null pointer' if slot == 19 else 'yes: bounded runtime'
        guest = ('no: null pointer' if slot == 19 else 'partial: literals/%% only; varargs return -1 [P2]' if slot == 15 else 'yes: bounded copy' if slot in [16, 20] else 'partial: host libm; exact cross-OS result unproved [P2]')
        if version == 1:
            guest += '; acquisition added X06'
        add('Parameters and ANSI', 'PF ANSI Suite', version, slot, name, win, guest, ref(N+'worker_host_suite_catalog.cpp', 'const void* provide_ansi1') + '; ' + ref(I, 'fn install_pf_ansi_suite_v2') + '; ' + ref(G+'x64/callbacks.rs', 'fn emulate_ansi_sprintf_literal') if slot == 15 else ref(N+'worker_host_suite_catalog.cpp', 'const void* provide_ansi1') + '; ' + ref(I, 'fn install_pf_ansi_suite_v2'))
add('Parameters and ANSI', 'PF Effect UI Suite', 1, 0, 'SetOptionsButtonName', 'yes: records bounded name', 'yes: records bounded name', ref(W, '"PF Effect UI Suite"')+'; '+ref(I, 'fn emulate_set_options_button_name'))

# Native Utility @3/@11 are actually zero-filled outside the two callbacks.
for version, count, register, window in [(3,9,7,8),(5,19,7,8),(7,25,7,8),(11,31,8,9),(13,33,9,10)]:
    for slot in range(count):
        name = 'RegisterWithAEGP' if slot == register else 'GetMainHWND' if slot == window else 'unnamed ABI slot'
        win = 'yes: plugin id 1' if slot == register else 'yes: desktop HWND' if slot == window else 'no: null pointer' if version in [3,11] else 'no: diagnosed error 4'
        guest = 'yes: plugin id 1' if slot == register else 'partial: null headless HWND, differs from native [U1]' if slot == window else 'no: diagnosed error 4 [U1]'
        if version == 5:
            guest += '; acquisition added X06'
        add('Memory, utility, preferences and compute cache', 'AEGP Utility Suite', version, slot, name, win, guest, ref(N+'worker_aegp_utility_suite.cpp','UtilitySuite g_utility_suite =')+'; '+ref(I,'fn install_aegp_utility_suites'))
for slot, name in enumerate(struct_members(N+'worker_handle_runtime.hpp', 'AegpMemorySuite')):
    guest = 'yes: owned bounded handles' if slot < 6 else 'no: returns 4; no per-slot telemetry [U2]'
    add('Memory, utility, preferences and compute cache', 'AEGP Memory Suite', 1, slot, name, 'yes: owned bounded handles/statistics', guest, ref(N+'worker_handle_runtime.hpp','struct AegpMemorySuite')+'; '+ref(G+'x64/engine.rs','let mut aegp_memory_suite'))
for version in [3,4]:
    for slot, name in enumerate(struct_members(N+'worker_aegp_persistent_data_suite.hpp','Suite'+str(version))):
        guest = 'no: suite absent [D1]' if version == 4 else 'partial: in-memory token/string/long maps; bounds differ [D1]' if slot in [0,3,13,14] else 'no: diagnosed error 3 [D1]'
        add('Memory, utility, preferences and compute cache', 'AEGP Persistent Data Suite', version, slot, name, 'partial: bounded process-local blob; no disk persistence', guest, ref(N+'worker_aegp_persistent_data_suite.hpp','struct Suite'+str(version))+'; '+ref(I,'fn install_persistent_data_suite_v3'))
for slot,name in enumerate(struct_members(N+'worker_aegp_compute_cache.hpp','AEGP_ComputeCacheSuite1')):
    guest = 'partial: stores executable callbacks; 64 classes/255-byte IDs [C1]' if slot == 0 else 'no: diagnosed error 1 [C1]'
    add('Memory, utility, preferences and compute cache','AEGP Compute Cache',1,slot,name,'yes: bounded owned cache',guest,ref(W,'aexcompat::worker_runtime::compute_cache::kSuiteName')+'; '+ref(N+'worker_aegp_compute_cache.hpp','struct AEGP_ComputeCacheSuite1')+'; '+ref(I,'fn install_aegp_compute_cache_suite'))

cs = struct_members(N+'worker_color_settings_runtime.hpp','AegpColorSettingsSuite6')
color_notes = {
0:'no: clears output, returns 4', 1:'partial: host view; linear-sRGB flag',
2:'partial: sRGB/linear-sRGB transform; imported RGB rejected',
3:'partial: owned working profile; optional external ACE',4:'partial: validated RGB ICC profiles',
5:'yes: owned ICC-byte copy',6:'partial: profile description handle',7:'yes: owned profile disposal',
8:'partial: built-in gamma; imported RGB rejected',9:'yes: validated RGB profile flag',
10:'partial: owned working profile assignment',11:'partial: reports OCIO off',
12:'partial: empty UTF-16 handle',13:'partial: empty UTF-16 handle',14:'partial: empty UTF-16 handle',15:'partial: two empty UTF-16 handles',
16:'partial: reports disabled',17:'partial: fixed interpolation',18:'partial: fixed luminance',19:'partial: built-in working-space GUID'}
for version,count in [(3,10),(4,11),(6,14),(7,20)]:
    for slot,name in enumerate(cs[:count]):
        add('Color settings, ACE and conversion','PF Color Settings Suite',version,slot,name,color_notes[slot],'no: suite absent [K1]',ref(N+'worker_color_settings_runtime.hpp','struct AegpColorSettingsSuite6')+'; '+ref(N+'worker_color_settings_runtime.cpp','int32_t __cdecl color_'+name))
for slot in range(32):
    name = 'to_working' if slot == 0 else 'from_working' if slot == 2 else 'unknown ABI / guard slot'
    add('Color settings, ACE and conversion','AEFX ACE Suite',1,slot,name,'partial: PF8/PF16 depth mapping only' if slot in [0,2] else 'no: diagnosed unsupported; signature unknown','no: suite absent [K2]',ref(N+'worker_aefx_ace_suite.hpp','struct Suite1')+'; '+ref(N+'worker_aefx_ace_suite.cpp','suite1()'))
for suite in ['PF Color Suite','PF Color16 Suite','PF ColorFloat Suite']:
    for slot,name in enumerate('RGBtoHLS HLStoRGB RGBtoYIQ YIQtoRGB Luminance Hue Lightness Saturation'.split()):
        add('Color settings, ACE and conversion',suite,1,slot,name,'partial: implemented host color math','no: suite absent [K3]',ref(N+'worker_pf_suites_internal.hpp','struct PfColorCallbacks')+'; '+ref(N+'worker_pf_suites.cpp','g_color_suite8'))

# Scene resolver precedes the static catalog; preserve mode-dependent rows.
ui_slots = {
('AEGP PF Interface Suite',1):{0:'effect layer token',1:'owned effect handle'},
('AEGP Layer Suite',11):{14:'injected current time'},
('AEGP Effect Suite',3):{8:'dispose owned effect'},
('AEGP Effect Suite',4):{8:'dispose owned effect'},
('AEGP Stream Suite',7):{5:'new parameter stream',7:'dispose stream',13:'scalar value at injected time only',14:'dispose value'},
('AEGP Stream Suite',10):{5:'new parameter stream',7:'dispose stream',13:'scalar value at injected time only',14:'dispose value'},
('AEGP Dynamic Stream Suite',2):{4:'hidden flag only',5:'set hidden flag only'},
('AEGP Dynamic Stream Suite',3):{4:'hidden flag only',5:'set hidden flag only'},
('AEGP Dynamic Stream Suite',5):{5:'hidden flag only',6:'set hidden flag only'},
}

def guest_scene(suite, version, slot):
    slots = ui_slots.get((suite, version))
    if slots is None:
        return 'no: suite absent [S1]'
    if slot in slots:
        return 'partial: UI dispatch only, '+slots[slot]+' [S1]'
    return 'no: UI callback error; absent in render [S1]'


scene = read(S)
factory = read(N+'worker_entry_wiring.cpp')
legacy = re.findall(r'reinterpret_cast<void\*>\(&([\w:]+)\)', re.search(r'scene_factory.legacy_stream_callbacks = \{\{(.*?)\}\};',factory,re.S).group(1))
fmap = {int(i):name for i,name in re.findall(r'scene_factory.keyframe_callbacks\[(\d+)\]\s*=\s*reinterpret_cast<void\*>\(&([\w:]+)\)',factory)}
branches = list(re.finditer(r'  if \(named\("([^"]+)"\)([^\{]+)\{',scene))
for bi,match in enumerate(branches):
    suite = match.group(1)
    versions = [int(v) for v in re.findall(r'version == (\d+)',match.group(2))]
    end = branches[bi+1].start() if bi+1<len(branches) else scene.index('  return SceneSuiteAcquireResult::not_handled;',match.end())
    body = re.sub(r'//[^\n]*','',scene[match.end():end])
    count = int(re.search(r'unsupported_suite_slots<UnsupportedSuiteId::\w+, (\d+)>',body).group(1))
    implemented = {}
    for assignment in re.finditer(r'(?:\b\w+\[(\d+)\]|reinterpret_cast<void\*\*>\(&\w+\)\[(\d+)\])\s*=\s*([^;]+);',body):
        slot = int(assignment.group(1) or assignment.group(2))
        expr = assignment.group(3).strip()
        callback = re.search(r'reinterpret_cast<void\*>\(\s*&([\w:]+)\)',expr)
        if callback:
            implemented[slot] = callback.group(1)
        elif expr.startswith('factory.legacy_stream_callbacks['):
            implemented[slot] = legacy[int(re.search(r'\[(\d+)\]',expr).group(1))]
        elif expr == 'factory.comp_bg_color':
            implemented[slot] = 'aegp_get_comp_bg_color'
        elif expr == 'factory.effect_param_union':
            implemented[slot] = 'aegp_get_effect_param_union_by_index_v3'
    if suite == 'AEGP Item Suite':
        if versions == [14]:
            implemented.update({2:'aegp_get_active_item',5:'aegp_get_item_type',7:'aegp_get_item_name',9:'aegp_get_item_id',14:'aegp_get_item_duration',15:'aegp_get_item_current_time',20:'aegp_set_item_current_time'})
        elif versions == [10]:
            implemented.update({2:'aegp_get_active_item',5:'aegp_get_item_type'})
    if suite == 'AEGP Collection Suite':
        implemented.update({1:'aegp_dispose_collection',2:'aegp_get_collection_count',3:'aegp_get_collection_item'})
    if suite == 'AEGP Keyframe Suite':
        implemented.update({i:v for i,v in fmap.items() if i<count})
    mode = 'scene'
    if 'comp_idle_roundtrip_mode' in match.group(2):
        mode += '+comp-idle' if 'active_idle_roundtrip_mode' not in match.group(2) else '+active/comp-idle'
    for version in versions:
        for slot in range(count):
            callback = implemented.get(slot,'unnamed ABI slot')
            win = 'partial: bounded scene callback' if slot in implemented else 'no: diagnosed error 4'
            if suite == 'AEGP Item Suite' and version == 10 and slot == 16:
                win = 'partial: geometry only outside menu/command/idle modes'
            if 'reject_' in callback:
                win = 'no: explicit rejection'
            if suite == 'AEGP Stream Suite' and ('stream_value' in callback):
                win += '; arbitrary values unsupported [A1]'
            add('Scene object suites',suite,version,slot,callback,win,guest_scene(suite,version,slot),ref(S,match.group(0).strip())+f' ({mode}); '+ref(U,'fn emulate_ui_stream_call'))

# The static mask provider is a different behavior for the same Stream/Keyframe
# identities. List it separately instead of silently unioning implementations.
maskpath = N+'worker_mask_suite_tables.cpp'
for suite,version,table in [('AEGP Stream Suite',11,'g_stream_suite'),('AEGP Stream Suite',9,'g_stream_suite4'),('AEGP Keyframe Suite',5,'g_keyframe_suite'),('AEGP Keyframe Suite',4,'g_keyframe_suite4'),('AEGP Keyframe Suite',3,'g_keyframe_suite4'),('AEGP Dynamic Stream Suite',5,'g_dynamic_stream_suite')]:
    for slot,name in enumerate(callbacks(maskpath,table)):
        win = 'no: explicit rejection' if name.startswith(('unsupported_','reject_')) else 'partial: bounded mask/stream model'
        add('Mask stream fallback',suite,version,slot,name,win,guest_scene(suite,version,slot),ref(maskpath,table+'{')+'; '+ref(W,'"'+suite+'"')+' (mask mode; scene resolver takes precedence); '+ref(U,'fn emulate_ui_stream_call'))
for slot in range(14):
    add('Mask stream fallback','AEGP Dynamic Stream Suite',2,slot,'aegp_set_dynamic_stream_flag_v2' if slot == 5 else 'unnamed ABI slot','partial: legacy flag adapter' if slot == 5 else 'no: diagnosed error 4',guest_scene('AEGP Dynamic Stream Suite',2,slot),ref(N+'worker_host_suite_catalog.cpp','const void* provide_dynamic_stream2')+'; '+ref(U,'fn emulate_ui_stream_call'))
for slot,name in enumerate(struct_members(N+'worker_aegp_pf_interface_suite.hpp','PfInterfaceSuite')):
    add('Scene object suites','AEGP PF Interface Suite',1,slot,name,'partial: bounded scene identity/time/camera',guest_scene('AEGP PF Interface Suite',1,slot),ref(N+'worker_aegp_pf_interface_suite.hpp','struct PfInterfaceSuite')+'; '+ref(U,'fn emulate_ui_stream_call'))

# Guest-only aliases: the source allocates 128 pointers, without declaring an
# ABI extent for these acquisition versions. Do not invent an SDK count/name.
for suite,version in [('AEGP Stream Suite',10),('AEGP Dynamic Stream Suite',3)]:
    for slot in range(128):
        add('Guest-only UI aliases (physical table capacity)',suite,version,slot,'guest UI slot (ABI extent unverified)','no: acquisition version absent',guest_scene(suite,version,slot),ref(U,'fn ui_suite_address')+'; '+ref(U,'const UI_SLOTS'))

# Rendering assembly uses arrays in exact order. Several historical *_reject
# symbols now delegate to working runtimes; only sound unconditionally rejects.
wiring = read(W)
assembly_specs = [
('AEGP World Suite',3,'aegp_world_new_owned'),
('AEGP Layer Render Options Suite',1,'new_layer_render_options'),
('AEGP Layer Render Options Suite',2,'new_layer_render_options'),
('AEGP Render Options Suite',1,'render_options_new_from_item'),
('AEGP Render Options Suite',4,'render_options_new_from_item'),
('AEGP Render Suite',2,'render_checkout_frame_reject'),
('AEGP Render Suite',5,'render_checkout_frame_reject'),
('AEGP Render Suite',8,'render_checkout_frame_reject'),
('AEGP Render Asyc Manager Suite',1,'checkout_item_frame_async'),
]
used = collections.Counter()
for suite,version,first in assembly_specs:
    candidates = list(re.finditer(r'\{reinterpret_cast<void\*>\(&'+first+r'\).*?\}',wiring,re.S))
    match = candidates[used[first]]
    used[first] += 1
    cb = re.findall(r'reinterpret_cast<void\*>\(&([\w:]+)\)',match.group(0))
    versions = [(version,cb)]
    if suite == 'AEGP World Suite':
        versions.append((2,cb[:7]+cb[8:]))
    for v,names in versions:
        for slot,name in enumerate(names):
            win = 'no: clears output and returns 4' if name == 'render_sound_reject' else 'partial: bounded world/options/receipt runtime [R1]'
            ev = ref(W,match.group(0))+'; '+ref(N+'worker_l2_render_abi.cpp','int32_t __cdecl render_checkout_frame_reject')
            add('Worlds and rendering',suite,v,slot,name,win,'no: suite absent [R1]',ev)

# A carrier is not the Unicorn engine. It is selected only by native-carrier;
# keep a separate per-slot inventory of its scoped suites rather than treating
# it as evidence for the default macOS path.
for suite,count in [('PF ColorParamSuite',1),('PF PointParamSuite',1),('AEGP Memory Suite',8)]:
    for slot in range(count):
        name = 'get_floating_point_value' if count == 1 else struct_members(N+'worker_handle_runtime.hpp','AegpMemorySuite')[slot]
        add('Optional native carrier (not default Unicorn)',suite,1,slot,name,'see main table','partial: carrier implementation; not run in X06',ref(G+'native_x64.rs','unsafe extern "win64" fn acquire_suite')+'; '+ref(G+'native_aegp_memory.rs'))
for v,count,reg,win in [(3,9,7,8),(7,25,7,8),(11,31,8,9),(13,33,9,10)]:
    for slot in range(count):
        name = 'RegisterWithAEGP' if slot == reg else 'GetMainHWND' if slot == win else 'unnamed ABI slot'
        status = 'yes: plugin id 1' if slot == reg else 'partial: null headless HWND' if slot == win else 'no: diagnosed error 4'
        add('Optional native carrier (not default Unicorn)','AEGP Utility Suite',v,slot,name,'see main table',status,ref(G+'native_x64.rs','fn native_utility_callbacks'))

INTRO = '''# Host-suite slot census — 2026-10-28 (X06)

## Scope and evidence meaning

**FACT:** source baseline `cb3bb76827b55f84dfc58cfcd06fb78553b6309b` plus the
X06 guest version additions. Windows means the native minihost, **not Adobe AE**.
macOS guest means the default Unicorn PE x64 engine; the optional native carrier
has its own appendix. Version numbers are AcquireSuite numbers, not C++ struct
suffixes. Slots are zero based (x64 pointer offset = slot × 8).

`yes` means a real implementation for the described bounded host contract;
`partial` means a stated subset, mode restriction or unresolved numerical parity;
`no` means absent acquisition, null pointer or explicit unsupported callback as
specified. None is a claim of all-input Adobe-equivalent output. Source links
are **static facts**, not newly executed Windows tests. Rows with an unnamed
slot deliberately retain the ABI index: these clean-room tables do not name the
unsupported function, and this audit does not invent a signature.

Every native version/slot in the requested PF parameter/ANSI/Effect UI, color,
AEGP memory/utility/preferences/cache and scene/stream/world/render providers is
listed. Related project/effect/collection/keyframe/PF-interface providers are
included because they supply the scene graph. Mask geometry, DRAWBOT, app,
iteration and unrelated PF suites are outside this ticket, except the two-slot
AEGP Iterate table returned by the scene resolver. Stream/Keyframe rows are
repeated **by provider** when scene and mask routing differ. Scene resolution
precedes the static catalog; a mode-gated scene table is not globally available.
`AEGP Render Asyc Manager Suite` is the actual misspelled acquisition string.

The two guest-only UI aliases publish 128 physical pointers each; these rows
explicitly say physical capacity, **not a verified SDK ABI extent**. Other UI
suites also reserve 128 pointers: their unused padding is not additional ABI
coverage. All UI-only rows are absent from ordinary render sessions and calls
outside an active UI dispatch fail. Errors in this UI path stop emulation and
set `callback_error`; they are not `unsupported_suite_calls` telemetry.

## Changes and behavioral boundary

X06 adds ANSI acquisition v1 using the existing v2 19-slot prefix and Utility
acquisition v5 with 19 entries, RegisterWithAEGP at 7 and GetMainHWND at 8.
No new approximation or success-only stub was added. Utility v5's other 17
entries diagnose their exact version/slot and return 4. The table is placed at
an unused offset beyond v13's 264-byte extent; old callback addresses stay fixed.

New tests invoke acquired pointers: one per ANSI v1 slot (19), one per newly
reachable Utility v5 implemented slot (2), and an all-version diagnostic/boundary
test (1). Numeric witnesses compare exact bytes at representable inputs, with
**no tolerance**. They prove wiring and ABI, not general libm bit equivalence.
ANSI sprintf is deliberately still partial: literals and `%%` work, `%d/%x`
return -1 without writing. Utility HWND is still zero in the guest versus a
Windows desktop HWND in the native host. Both limitations remain visible below.

## Important corrections to family-level assumptions

* Utility native v3/v11 have null entries outside the two named callbacks;
  v5/v7/v13 install diagnostic stubs. Guest unsupported entries are diagnostic.
* Native Memory implements all 8 slots; guest slots 6/7 return 4 without recording
  which slot was called. Presence alone overstates parity.
* The **wired** native compute cache is `worker_aegp_compute_cache.*`, not the
  older `worker_compute_cache_suite.*` implementation named in the brief. The
  latter remains source but is not the catalog provider. Native has real receipt,
  ownership, callback and teardown paths; guest only stores registrations.
* Native Color Settings slot 0 always returns 4. ACE's 32 entries are a defensive
  capacity; only 0/2 have observed signatures and depth-conversion behavior.
* Render symbols ending in `reject` must be read: item/layer checkout, async
  checkout/cancel, region, timestamps and GUID now delegate to real runtimes.
  Sound remains unsupported. Render v2 and Render Options v4 are mode gated;
  all render operations still require valid owned state.

## Arbitrary values: separate from parameter serialization

**FACT:** native `worker_aegp_compat_selftests.cpp` constructs an arbitrary
parameter (index 14/type 11), acquires its stream, then requires value retrieval
to return 4 without changing the sentinel-filled destination (lines around the
`arbitrary_before` comparison). This is negative-path coverage, not arbitrary
value support. Legacy Stream v4 slot 12 and v7/v9 slot 13 reach the corresponding
value callback; current Stream v8/v11 slot 13 has its own bounded scene path.

**FACT:** guest `ui_session.rs::emulate_ui_stream_call` supports only parameter
types 1/2/3/4/7/10 at the injected time; arbitrary type 11 is an explicit callback
error. `classic_ui.rs` COPY, FLAT_SIZE and FLATTEN drive original plug-in
arbitrary parameter selectors, which is a different lifecycle and does not
implement AEGP arbitrary stream ownership, get/set or keyframes. No arbitrary
stream parity test was added or claimed by X06. See follow-up A1.

## Follow-ups (INFERENCE: proposed work, not implemented)

| ID | Concrete scope / acceptance boundary |
|---|---|
| P1 | Bring ParamUtils v2/v3 and Angle v1 to render dispatch with correct obsolete/new state layouts, keyframe/time semantics and parameter identity; preserve UI cosmetic/value ownership separation. Differentially test all returned data and malformed/stale references. |
| P2 | Implement ANSI varargs Win64 formatting with bounded destination/error semantics; establish Windows-UCRT-versus-host-libm exact witnesses for every numeric operation, including nonfinite/domain/rounding boundaries. Existing CRT formatter is a lead, not evidence of identical PF sprintf semantics. |
| U1 | Specify headless main-window behavior and implement required Utility slots from verified ABI signatures. Native null v3/v11 slots also need explicit rejection/diagnostics; guest missing behavior is not fixed by table presence. |
| U2 | Implement Memory reporting toggle and stats from the existing handle registry (slots 6/7), including exact counters/plugin ownership/error writes; add per-slot unsupported telemetry until implemented. |
| D1 | Complete preferences v3 getters/enumeration/delete/data/floating values, bounds/default/type-conversion behavior; then v4 domain-separated tokens. Align guest's 4096-byte key/65536-byte string parsing with native bounds; do not alias v4 slot 0 to v3 (different ABI). Disk persistence/AE conversion remain separate oracle work (#886). |
| C1 | Add executable guest callback continuations for key/compute/size/delete; owned class generations, cached entries/receipts, checkout/checkin/unregister and teardown with stale/foreign rejection. Native `worker_aegp_compute_cache.*` is the model; guest has only class registrations and different bounds. |
| K1 | Owned ICC/profile handles and view/working-space state, color transform and OCIO contracts. Separate built-in sRGB/linear behavior from imported RGB/external ACE; exercise lifetime and output bytes against native and eventually AE. |
| K2 | Implement ACE known slots 0/2 with exact native PF8↔PF16 mapping/count/quality validation; other 30 signatures remain unobserved. Depth mapping is not profile management. |
| K3 | Port the 8 PF color callbacks in each of 8/16/float suites; exact fixed-point scaling, alpha preservation and invalid-input behavior require per-slot native witnesses. |
| A1 | Define owned arbitrary stream value payloads, plug-in selector continuation for copy/dispose/interpolate/serialize, get/set/keyframe ownership and rollback; preserve the existing native rejected-output witness until actual semantics exist. No invented value layout. |
| S1 | Add a general guest project/item/comp/layer/effect/stream registry with stable generations and borrowed-handle leases, scene time and parameter backing, matching each version's ABI. Start from `worker_aegp_scene_model.hpp`, `scene_acquire_suite` and `worker_entry_wiring.cpp` factory. The UI scalar tokens are not a render scene. Include mode precedence, legacy string ABI, stream type/value and negative paths per row. |
| R1 | Layer/item render options, source graph/time/ROI/depth transport, owned world/receipt lifetimes, cancellation/async continuation, external-render timestamps and GUIDs; use `worker_aegp_item_render_runtime.*`, `worker_aegp_layer_render_runtime.*`, `worker_l2_render_abi.cpp` and render selftests. Scene objects must exist before these tables can have real behavior. |

## Reproduction and limitations

Regenerate this source inventory with `python3 tools/host_suite_slot_census.py`.
This is documentation extraction plus reviewed annotations, **not a test that
asserts source strings**. On a new baseline re-review annotations/provider modes;
a successful run alone does not establish completeness or behavior.

Run the focused guest tests with `cargo test --manifest-path guest/Cargo.toml
-p aex-guest-worker --lib census_ -- --test-threads=1` (one build job, build/TMPDIR
outside paths with spaces). Additional existing ANSI, Utility, memory, parameter,
persistent and compute tests are useful regressions. Execution results and exact
commands are in the X06 worker report; no Windows native build or AE oracle was
run here. Registered OLM workers and plug-in kernels were not changed.

## Per-slot tables
'''

lines = [INTRO]
main_rows = [r for r in rows if r[0] != 'Optional native carrier (not default Unicorn)']
lines.append(f'Inventory: **{len(main_rows)} default-path/provider rows**, plus **{len(rows)-len(main_rows)} optional-carrier rows**. Counts include duplicate provider rows and guest-only physical-capacity rows; they are not a supported-slot score.\n')
for group in dict.fromkeys(row[0] for row in rows):
    lines += ['\n### '+group+'\n', '| Suite | Acquire version | Slot | Callback / identity | Windows native | macOS guest | Evidence / route |', '|---|---:|---:|---|---|---|---|']
    for row in rows:
        if row[0] != group:
            continue
        lines.append('| '+' | '.join(str(x).replace('|','\\|') for x in row[1:])+' |')
output = ROOT / 'analysis/HOST_SUITE_SLOT_CENSUS_2026-10-28.md'
output.write_text('\n'.join(lines)+'\n',encoding='utf-8')
print(f'{output.name}: {len(rows)} rows')
for group,n in collections.Counter(row[0] for row in rows).items():
    print(f'  {group}: {n}')
