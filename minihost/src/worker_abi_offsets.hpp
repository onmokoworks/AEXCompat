#pragma once

#include "generated/aex_abi_contract.hpp"

#include <cstddef>

// Supplemental Windows x64 layout offsets used by the minihost. Generated
// fields remain in aex_abi_contract.hpp; only missing fields are defined here.
namespace aexcompat::worker_abi_offsets {

// PF_LayerDef: origin follows the pixel-aspect ratio and platform fields;
// dephault is the trailing layer-parameter default, relative to the layer union member.
inline constexpr std::size_t kLayerOriginX = 104;
inline constexpr std::size_t kLayerOriginY = 108;
inline constexpr std::size_t kLayerDefault = 116;

// PF_ParamDef.u members not present in the generated contract. These offsets
// are relative to the union member (add PARAM_U_OFFSET for a whole definition).
// PF_PointDef holds two Fixed values; PF_Point3DDef holds three doubles.
inline constexpr std::size_t kFixedSliderPrecision = 88;
inline constexpr std::size_t kPathDefault = 8;
inline constexpr std::size_t kAngleDefault = 4;
inline constexpr std::size_t kPointValueY = 4;
inline constexpr std::size_t kPointDefaultX = 12;
inline constexpr std::size_t kPointDefaultY = 16;
inline constexpr std::size_t kPoint3DValueX = 0;
inline constexpr std::size_t kPoint3DValueY = 8;
inline constexpr std::size_t kPoint3DValueZ = 16;
inline constexpr std::size_t kPoint3DDefaultX = 24;
inline constexpr std::size_t kButtonLabel = 8;

// PF_RationalScale stores its denominator after the 32-bit numerator.
inline constexpr std::size_t kRationalDenominator = 4;

// PF_SoundWorld starts with PF_SoundFormatInfo (rate, channels, format,
// sample size), then num_samples and the aligned sample-data pointer.
// All offsets below are relative to PF_SoundWorld.
inline constexpr std::size_t kSoundRate = 0;
inline constexpr std::size_t kSoundChannels = 8;
inline constexpr std::size_t kSoundFormat = 10;
inline constexpr std::size_t kSoundSampleSize = 12;
inline constexpr std::size_t kSoundNumSamples = 16;
inline constexpr std::size_t kSoundData = 24;

// PF_Point uses horizontal/x then vertical/y int32 fields. PF_DrawEventInfo
// contains an update rectangle followed by depth; offsets are member-relative.
inline constexpr std::size_t kPointHorizontal = 0;
inline constexpr std::size_t kPointVertical = 4;
inline constexpr std::size_t kDrawDepth = 16;

// PF_EventCallbacks pointer slots, relative to EVENT_CALLBACKS_OFFSET.
// The intervening matrix-transform slots are not used by these dispatch paths.
inline constexpr std::size_t kEventCallbacksRefcon = 0;
inline constexpr std::size_t kEventCallbacksLayerToComp = 8;
inline constexpr std::size_t kEventCallbacksCompToLayer = 16;
inline constexpr std::size_t kEventCallbacksSourceToFrame = 40;
inline constexpr std::size_t kEventCallbacksFrameToSource = 48;

// Additional SmartFX members: PF_PreRenderCallbacks.guid_mix_in and
// PF_PreRenderOutput.delete_pre_render_data_func, relative to their structs.
inline constexpr std::size_t kPreCallbacksGuidMixIn = 8;
inline constexpr std::size_t kPreOutputDeletePreRenderData = 48;

// PF_ExtDependenciesExtra.check_type precedes the generated dependencies handle.
inline constexpr std::size_t kExternalDependenciesCheckType = 0;

// PF_ArbParamsExtra.u.scan_func fields are relative to the whole extra,
// matching the other ARB_* generated arm offsets (refcon is at union start).
inline constexpr std::size_t kArbScanRefcon = 8;
inline constexpr std::size_t kArbScanBuffer = 16;
inline constexpr std::size_t kArbScanSize = 24;
inline constexpr std::size_t kArbScanOutput = 32;

}  // namespace aexcompat::worker_abi_offsets
