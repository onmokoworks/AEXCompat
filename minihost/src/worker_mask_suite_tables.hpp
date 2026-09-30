#pragma once

namespace aexcompat::l2_detail {

// Table types come from the in-namespace ABI header
// (worker_l2_suite_abi.hpp); consumers that need the complete types keep
// including it the way l2_main.cpp does. These forward declarations only
// publish the table identities.
struct PfMaskSuite1;
struct MaskSuite;
struct MaskSuite5;
struct StreamSuite;
struct StreamSuite4;
struct KeyframeSuite;
struct KeyframeSuite4;
struct DynamicStreamSuite;
struct MaskOutlineSuite;

extern PfMaskSuite1 g_pf_mask_suite1;
extern MaskSuite g_mask_suite;
extern MaskSuite5 g_mask_suite5;
extern StreamSuite g_stream_suite;
extern StreamSuite4 g_stream_suite4;
extern KeyframeSuite g_keyframe_suite;
extern KeyframeSuite4 g_keyframe_suite4;
// `AEGP_KeyframeSuite3` is the same 20 members in the same order as
// `AEGP_KeyframeSuite4`; five of them take `AEGP_StreamValue*` where v4 takes
// `AEGP_StreamValue2*`, and those two structs are `{AEGP_StreamRefH; union}`
// differing only in `AEGP_MarkerValH markerH` vs `AEGP_MarkerValP markerP`,
// both pointer-sized. On x64 the tables are byte-identical, and this host's
// keyframe values are mask outlines that never reach the marker member, so v3
// is served from a copy of the v4 table rather than a second hand-written one
// that could drift (issue #1285).
extern KeyframeSuite4 g_keyframe_suite3;
extern DynamicStreamSuite g_dynamic_stream_suite;
extern MaskOutlineSuite g_mask_outline_suite;

bool keyframe_suite5_abi_wiring_valid();

}  // namespace aexcompat::l2_detail
