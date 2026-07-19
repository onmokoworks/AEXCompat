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
struct KeyframeSuite;
struct DynamicStreamSuite;
struct MaskOutlineSuite;

extern PfMaskSuite1 g_pf_mask_suite1;
extern MaskSuite g_mask_suite;
extern MaskSuite5 g_mask_suite5;
extern StreamSuite g_stream_suite;
extern KeyframeSuite g_keyframe_suite;
extern DynamicStreamSuite g_dynamic_stream_suite;
extern MaskOutlineSuite g_mask_outline_suite;

bool keyframe_suite5_abi_wiring_valid();

}  // namespace aexcompat::l2_detail
