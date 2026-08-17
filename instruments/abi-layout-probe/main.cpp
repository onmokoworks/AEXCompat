#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectUI.h"
#include "AE_AdvEffectSuites.h"
#include "adobesdk/DrawbotSuite.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <iostream>
#include <type_traits>

namespace {
static_assert(sizeof(void*) != 8 ||
                  offsetof(AEGP_EffectSuite4, AEGP_GetEffectParamUnionByIndex) ==
                      3 * sizeof(void*),
              "AEGP_GetEffectParamUnionByIndex must be x64 slot 3 (offset 24)");
static_assert(AEGP_InstalledEffectKey_NONE == 0);
static_assert(sizeof(AEGP_EffectSuite4) == 22 * sizeof(void*));
static_assert(sizeof(A_Time) == 8);
static_assert(sizeof(A_LRect) == 16);
static_assert(sizeof(AEGP_LayerRenderOptionsSuite1) == 14 * sizeof(void*));
static_assert(sizeof(AEGP_LayerRenderOptionsSuite2) == 15 * sizeof(void*));
static_assert(sizeof(AEGP_RenderOptionsSuite1) == 17 * sizeof(void*));
static_assert(sizeof(AEGP_RenderOptionsSuite4) == 23 * sizeof(void*));
static_assert(offsetof(AEGP_CompSuite10, AEGP_GetItemFromComp) ==
              1 * sizeof(void*));
static_assert(offsetof(AEGP_LayerSuite8, AEGP_GetLayerCurrentTime) ==
              14 * sizeof(void*));
using GetItemFromCompSignature =
    A_Err(SPAPI*)(AEGP_CompH, AEGP_ItemH*);
using GetLayerCurrentTimeSignature =
    A_Err(SPAPI*)(AEGP_LayerH, AEGP_LTimeMode, A_Time*);
static_assert(std::is_same_v<
              decltype(AEGP_CompSuite10::AEGP_GetItemFromComp),
              GetItemFromCompSignature>);
static_assert(std::is_same_v<
              decltype(AEGP_LayerSuite8::AEGP_GetLayerCurrentTime),
              GetLayerCurrentTimeSignature>);
#define ASSERT_SDK_SLOT(Suite, Member, Slot) \
  static_assert(offsetof(Suite, Member) == (Slot) * sizeof(void*))
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_NewFromLayer, 0);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_NewFromUpstreamOfEffect, 1);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_Duplicate, 2);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_Dispose, 3);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_SetTime, 4);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_GetTime, 5);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_SetTimeStep, 6);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_GetTimeStep, 7);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_SetWorldType, 8);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_GetWorldType, 9);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_SetDownsampleFactor, 10);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_GetDownsampleFactor, 11);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_SetMatteMode, 12);
ASSERT_SDK_SLOT(AEGP_LayerRenderOptionsSuite1, AEGP_GetMatteMode, 13);
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_NewFromLayer) ==
              0 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_NewFromUpstreamOfEffect) ==
              1 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_NewFromDownstreamOfEffect) ==
              2 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_Duplicate) ==
              3 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_Dispose) ==
              4 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_SetTime) ==
              5 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_GetTime) ==
              6 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_SetTimeStep) ==
              7 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_GetTimeStep) ==
              8 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_SetWorldType) ==
              9 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_GetWorldType) ==
              10 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_SetDownsampleFactor) ==
              11 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_GetDownsampleFactor) ==
              12 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_SetMatteMode) ==
              13 * sizeof(void*));
static_assert(offsetof(AEGP_LayerRenderOptionsSuite2, AEGP_GetMatteMode) ==
              14 * sizeof(void*));
#define ASSERT_RENDER_BASE(Suite) \
  ASSERT_SDK_SLOT(Suite, AEGP_NewFromItem, 0); \
  ASSERT_SDK_SLOT(Suite, AEGP_Duplicate, 1); \
  ASSERT_SDK_SLOT(Suite, AEGP_Dispose, 2); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetTime, 3); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetTime, 4); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetTimeStep, 5); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetTimeStep, 6); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetFieldRender, 7); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetFieldRender, 8); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetWorldType, 9); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetWorldType, 10); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetDownsampleFactor, 11); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetDownsampleFactor, 12); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetRegionOfInterest, 13); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetRegionOfInterest, 14); \
  ASSERT_SDK_SLOT(Suite, AEGP_SetMatteMode, 15); \
  ASSERT_SDK_SLOT(Suite, AEGP_GetMatteMode, 16)
ASSERT_RENDER_BASE(AEGP_RenderOptionsSuite1);
ASSERT_RENDER_BASE(AEGP_RenderOptionsSuite4);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_SetChannelOrder, 17);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_GetChannelOrder, 18);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_GetRenderGuideLayers, 19);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_SetRenderGuideLayers, 20);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_GetRenderQuality, 21);
ASSERT_SDK_SLOT(AEGP_RenderOptionsSuite4, AEGP_SetRenderQuality, 22);
#undef ASSERT_RENDER_BASE
#undef ASSERT_SDK_SLOT
static_assert(offsetof(AEGP_EffectSuite4, AEGP_GetNumInstalledEffects) ==
              11 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite4, AEGP_GetNextInstalledEffect) ==
              12 * sizeof(void*));
static_assert(offsetof(AEGP_EffectSuite4, AEGP_GetEffectCategory) ==
              15 * sizeof(void*));
static_assert(sizeof(void*) != 8 ||
                  offsetof(AEGP_LayerSuite5, AEGP_GetLayerSourceItem) ==
                      4 * sizeof(void*),
              "AEGP_GetLayerSourceItem must be x64 slot 4 (offset 32)");
static_assert(sizeof(AEGP_KeyframeSuite5) == 22 * sizeof(void*),
              "AEGP_KeyframeSuite5 must contain exactly 22 function slots");
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_InsertKeyframe) == 2 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeValue) == 5 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_GetNewKeyframeSpatialTangents) ==
              8 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeSpatialTangents) ==
              9 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeTemporalEase) ==
              10 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeTemporalEase) ==
              11 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeFlag) == 13 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeInterpolation) ==
              15 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_StartAddKeyframes) ==
              16 * sizeof(void*));
static_assert(offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeLabelColorIndex) ==
              21 * sizeof(void*));
static_assert(sizeof(PF_BatchSamplingSuite1) == 4 * sizeof(void*));
static_assert(offsetof(PF_BatchSamplingSuite1, begin_sampling) == 0 * sizeof(void*));
static_assert(offsetof(PF_BatchSamplingSuite1, end_sampling) == 1 * sizeof(void*));
static_assert(offsetof(PF_BatchSamplingSuite1, get_batch_func) == 2 * sizeof(void*));
static_assert(offsetof(PF_BatchSamplingSuite1, get_batch_func16) == 3 * sizeof(void*));
// in_data->utils handle callback offsets (issue #220). These pin the exact
// numeric offsets wired into kUtilityCallbackOffsets in
// worker_effect_bootstrap.cpp to the SDK PF_UtilCallbacks layout so a host or
// SDK header drift fails the build instead of handing plug-ins a null callback.
static_assert(offsetof(PF_UtilCallbacks, host_new_handle) == 160);
static_assert(offsetof(PF_UtilCallbacks, host_lock_handle) == 168);
static_assert(offsetof(PF_UtilCallbacks, host_unlock_handle) == 176);
static_assert(offsetof(PF_UtilCallbacks, host_dispose_handle) == 184);
static_assert(offsetof(PF_UtilCallbacks, app) == 200);
static_assert(offsetof(PF_UtilCallbacks, host_get_handle_size) == 440);
static_assert(offsetof(PF_UtilCallbacks, iterate_origin_non_clip_src) == 448);
static_assert(offsetof(PF_UtilCallbacks, iterate_generic) == 456);
static_assert(offsetof(PF_UtilCallbacks, host_resize_handle) == 464);
static_assert(offsetof(PF_UtilCallbacks, subpixel_sample16) == 472);
static_assert(offsetof(PF_UtilCallbacks, area_sample16) == 480);
// The ANSI block's remaining eight entries (issue #981). Eleven of the
// nineteen were emitted, so the host wired eleven and left the rest null;
// Basic_3D calls `fmod` and Bulge/Spherize call `floor` from FRAME_SETUP and
// each jumped to address 0, which the worker's SEH guard then reported as
// error 512 - the same shape as the 16-bit sampling pair in issue #777.
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, atan) == 208);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, atan2) == 216);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, exp) == 240);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, floor) == 256);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, fmod) == 264);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, log) == 280);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, log10) == 288);
static_assert(offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, tan) == 320);
// composite_rect, between end_sampling and blend (issue #1252): the contract
// left it out, so the host wired blend at 0x30 and left 0x28 null; Write_on's
// RENDER composites the input layer behind its strokes through it and jumped
// to address 0.
static_assert(offsetof(PF_UtilCallbacks, composite_rect) == 40);

template <typename T>
void field(const char* name, std::size_t offset, bool& first) {
  if (!first) std::cout << ',';
  first = false;
  std::cout << "\n    \"" << name << "\":{\"offset\":" << offset
            << ",\"size\":" << sizeof(T) << '}';
}
}  // namespace

int main() {
  bool first = true;
  std::cout << "{\n  \"schema_version\":1,\n"
               "  \"source_kind\":\"compiled_instrument_observation\",\n"
               "  \"source_path\":\"instruments/abi-layout-probe/main.cpp\",\n"
               "  \"sdk_root_committed\":false,\n"
               "  \"architecture\":\"x86_64-windows\",\n"
               "  \"sdk_boundary\":\"instrument_observation\",\n"
               "  \"pointer_size\":" << sizeof(void*) << ",\n"
               "  \"pf_in_data_size\":" << sizeof(PF_InData) << ",\n"
               "  \"pf_out_data_size\":" << sizeof(PF_OutData) << ",\n"
               "  \"pf_param_def_size\":" << sizeof(PF_ParamDef) << ",\n"
               "  \"pf_layer_def_size\":" << sizeof(PF_LayerDef) << ",\n"
               "  \"pf_slider_def_size\":" << sizeof(PF_SliderDef) << ",\n"
               "  \"pf_popup_def_size\":" << sizeof(PF_PopupDef) << ",\n"
               "  \"pf_checkbox_def_size\":" << sizeof(PF_CheckBoxDef) << ",\n"
               "  \"pf_float_slider_def_size\":" << sizeof(PF_FloatSliderDef) << ",\n"
               "  \"pf_ext_dependencies_extra_size\":" << sizeof(PF_ExtDependenciesExtra) << ",\n"
               "  \"pf_interact_callbacks_size\":" << sizeof(PF_InteractCallbacks) << ",\n"
               "  \"pf_param_union_size\":" << sizeof(PF_ParamDefUnion) << ",\n"
               "  \"pf_arbitrary_def_size\":" << sizeof(PF_ArbitraryDef) << ",\n"
               "  \"pf_arb_params_extra_size\":" << sizeof(PF_ArbParamsExtra) << ",\n"
               "  \"pf_custom_ui_info_size\":" << sizeof(PF_CustomUIInfo) << ",\n"
               "  \"pf_event_extra_size\":" << sizeof(PF_EventExtra) << ",\n"
               "  \"pf_event_union_size\":" << sizeof(PF_EventUnion) << ",\n"
               "  \"pf_context_size\":" << sizeof(PF_Context) << ",\n"
               "  \"pf_effect_window_info_size\":" << sizeof(PF_EffectWindowInfo) << ",\n"
               "  \"pf_adjust_cursor_info_size\":" << sizeof(PF_AdjustCursorEventInfo) << ",\n"
               "  \"pf_do_click_event_info_size\":" << sizeof(PF_DoClickEventInfo) << ",\n"
               "  \"pf_key_down_event_size\":" << sizeof(PF_KeyDownEvent) << ",\n"
               "  \"pf_adv_app_suite2_size\":" << sizeof(PF_AdvAppSuite2) << ",\n"
               "  \"drawbot_draw_suite_size\":" << sizeof(DRAWBOT_DrawbotSuite1) << ",\n"
               "  \"drawbot_supplier_suite_size\":" << sizeof(DRAWBOT_SupplierSuite1) << ",\n"
               "  \"drawbot_surface_suite_size\":" << sizeof(DRAWBOT_SurfaceSuite2) << ",\n"
               "  \"drawbot_path_suite_size\":" << sizeof(DRAWBOT_PathSuite1) << ",\n"
               "  \"pf_effect_custom_ui_suite1_size\":" << sizeof(PF_EffectCustomUISuite1) << ",\n"
               "  \"pf_effect_overlay_theme_suite1_size\":" << sizeof(PF_EffectCustomUIOverlayThemeSuite1) << ",\n"
               "  \"pf_app_suite4_size\":" << sizeof(PFAppSuite4) << ",\n"
               "  \"pf_util_callbacks_size\":" << sizeof(PF_UtilCallbacks) << ",\n"
               "  \"pf_batch_sampling_suite1_size\":" << sizeof(PF_BatchSamplingSuite1) << ",\n"
               "  \"pf_pixel_size\":" << sizeof(PF_Pixel) << ",\n"
               "  \"pf_pixel16_size\":" << sizeof(PF_Pixel16) << ",\n"
               "  \"pf_pixel_float_size\":" << sizeof(PF_PixelFloat) << ",\n"
               "  \"pf_sound_format_info_size\":" << sizeof(PF_SoundFormatInfo) << ",\n"
               "  \"pf_sound_world_size\":" << sizeof(PF_SoundWorld) << ",\n"
               "  \"pf_cmd_audio_render\":" << static_cast<long>(PF_Cmd_AUDIO_RENDER) << ",\n"
               "  \"pf_cmd_audio_setup\":" << static_cast<long>(PF_Cmd_AUDIO_SETUP) << ",\n"
               "  \"pf_cmd_audio_setdown\":" << static_cast<long>(PF_Cmd_AUDIO_SETDOWN) << ",\n"
               "  \"pf_world_flag_deep\":" << static_cast<unsigned long>(PF_WorldFlag_DEEP) << ",\n"
               "  \"pf_pre_render_extra_size\":" << sizeof(PF_PreRenderExtra) << ",\n"
               "  \"pf_pre_render_input_size\":" << sizeof(PF_PreRenderInput) << ",\n"
               "  \"pf_pre_render_output_size\":" << sizeof(PF_PreRenderOutput) << ",\n"
               "  \"pf_pre_render_callbacks_size\":" << sizeof(PF_PreRenderCallbacks) << ",\n"
               "  \"pf_smart_render_extra_size\":" << sizeof(PF_SmartRenderExtra) << ",\n"
               "  \"pf_smart_render_input_size\":" << sizeof(PF_SmartRenderInput) << ",\n"
               "  \"pf_smart_render_callbacks_size\":" << sizeof(PF_SmartRenderCallbacks) << ",\n"
               "  \"pf_gpu_device_setup_extra_size\":" << sizeof(PF_GPUDeviceSetupExtra) << ",\n"
               "  \"pf_gpu_device_setup_input_size\":" << sizeof(PF_GPUDeviceSetupInput) << ",\n"
               "  \"pf_gpu_device_setup_output_size\":" << sizeof(PF_GPUDeviceSetupOutput) << ",\n"
               "  \"pf_gpu_device_setdown_extra_size\":" << sizeof(PF_GPUDeviceSetdownExtra) << ",\n"
               "  \"pf_gpu_device_setdown_input_size\":" << sizeof(PF_GPUDeviceSetdownInput) << ",\n"
               "  \"pf_user_changed_param_extra_size\":" << sizeof(PF_UserChangedParamExtra) << ",\n"
               "  \"aegp_command_suite1_size\":" << sizeof(AEGP_CommandSuite1) << ",\n"
               "  \"aegp_register_suite5_size\":" << sizeof(AEGP_RegisterSuite5) << ",\n"
               "  \"aegp_item_suite9_size\":" << sizeof(AEGP_ItemSuite9) << ",\n"
               "  \"aegp_comp_suite11_size\":" << sizeof(AEGP_CompSuite11) << ",\n"
               "  \"aegp_comp_suite12_size\":" << sizeof(AEGP_CompSuite12) << ",\n"
               "  \"aegp_layer_suite5_size\":" << sizeof(AEGP_LayerSuite5) << ",\n"
               "  \"aegp_layer_suite8_size\":" << sizeof(AEGP_LayerSuite8) << ",\n"
               "  \"aegp_layer_suite9_size\":" << sizeof(AEGP_LayerSuite9) << ",\n"
               "  \"aegp_effect_suite4_size\":" << sizeof(AEGP_EffectSuite4) << ",\n"
               "  \"aegp_stream_suite6_size\":" << sizeof(AEGP_StreamSuite6) << ",\n"
               "  \"aegp_keyframe_suite5_size\":" << sizeof(AEGP_KeyframeSuite5) << ",\n"
               "  \"aegp_collection_suite2_size\":" << sizeof(AEGP_CollectionSuite2) << ",\n"
               "  \"aegp_collection_item_v2_size\":" << sizeof(AEGP_CollectionItemV2) << ",\n"
               "  \"aegp_stream_value2_size\":" << sizeof(AEGP_StreamValue2) << ",\n"
               "  \"fields\":{";
  field<decltype(PF_InData::version)>("in.version", offsetof(PF_InData, version), first);
  field<decltype(PF_InData::serial_num)>("in.serial_num", offsetof(PF_InData, serial_num), first);
  field<decltype(PF_InData::appl_id)>("in.appl_id", offsetof(PF_InData, appl_id), first);
  field<decltype(PF_InData::num_params)>("in.num_params", offsetof(PF_InData, num_params), first);
  field<decltype(PF_InData::pica_basicP)>("in.pica_basicP", offsetof(PF_InData, pica_basicP), first);
  field<decltype(PF_InData::inter)>("in.inter", offsetof(PF_InData, inter), first);
  field<decltype(PF_InData::utils)>("in.utils", offsetof(PF_InData, utils), first);
  field<decltype(PF_InData::effect_ref)>("in.effect_ref", offsetof(PF_InData, effect_ref), first);
  field<decltype(PF_InData::quality)>("in.quality", offsetof(PF_InData, quality), first);
  field<decltype(PF_InData::global_data)>("in.global_data", offsetof(PF_InData, global_data), first);
  field<decltype(PF_InData::sequence_data)>("in.sequence_data", offsetof(PF_InData, sequence_data), first);
  field<decltype(PF_ExtDependenciesExtra::dependencies_strH)>("external_dependencies.handle", offsetof(PF_ExtDependenciesExtra, dependencies_strH), first);
  field<decltype(PF_InData::frame_data)>("in.frame_data", offsetof(PF_InData, frame_data), first);
  field<decltype(PF_InData::current_time)>("in.current_time", offsetof(PF_InData, current_time), first);
  field<decltype(PF_InData::time_step)>("in.time_step", offsetof(PF_InData, time_step), first);
  field<decltype(PF_InData::total_time)>("in.total_time", offsetof(PF_InData, total_time), first);
  field<decltype(PF_InData::local_time_step)>("in.local_time_step", offsetof(PF_InData, local_time_step), first);
  field<decltype(PF_InData::time_scale)>("in.time_scale", offsetof(PF_InData, time_scale), first);
  field<decltype(PF_InData::field)>("in.field", offsetof(PF_InData, field), first);
  field<decltype(PF_InData::shutter_angle)>("in.shutter_angle", offsetof(PF_InData, shutter_angle), first);
  field<decltype(PF_InData::width)>("in.width", offsetof(PF_InData, width), first);
  field<decltype(PF_InData::height)>("in.height", offsetof(PF_InData, height), first);
  field<decltype(PF_InData::extent_hint)>("in.extent_hint", offsetof(PF_InData, extent_hint), first);
  field<decltype(PF_InData::output_origin_x)>("in.output_origin_x", offsetof(PF_InData, output_origin_x), first);
  field<decltype(PF_InData::output_origin_y)>("in.output_origin_y", offsetof(PF_InData, output_origin_y), first);
  field<decltype(PF_InData::downsample_x)>("in.downsample_x", offsetof(PF_InData, downsample_x), first);
  field<decltype(PF_InData::downsample_y)>("in.downsample_y", offsetof(PF_InData, downsample_y), first);
  field<decltype(PF_InData::pixel_aspect_ratio)>("in.pixel_aspect_ratio", offsetof(PF_InData, pixel_aspect_ratio), first);
  field<decltype(PF_InData::pre_effect_source_origin_x)>("in.pre_effect_source_origin_x", offsetof(PF_InData, pre_effect_source_origin_x), first);
  field<decltype(PF_InData::pre_effect_source_origin_y)>("in.pre_effect_source_origin_y", offsetof(PF_InData, pre_effect_source_origin_y), first);
  field<decltype(PF_InData::shutter_phase)>("in.shutter_phase", offsetof(PF_InData, shutter_phase), first);
  field<decltype(PF_InData::start_sampL)>("in.start_samp", offsetof(PF_InData, start_sampL), first);
  field<decltype(PF_InData::dur_sampL)>("in.duration_samples", offsetof(PF_InData, dur_sampL), first);
  field<decltype(PF_InData::total_sampL)>("in.total_samples", offsetof(PF_InData, total_sampL), first);
  field<decltype(PF_InData::src_snd)>("in.source_sound", offsetof(PF_InData, src_snd), first);
  field<decltype(PF_InteractCallbacks::checkout_param)>("inter.checkout_param", offsetof(PF_InteractCallbacks, checkout_param), first);
  field<decltype(PF_InteractCallbacks::checkin_param)>("inter.checkin_param", offsetof(PF_InteractCallbacks, checkin_param), first);
  field<decltype(PF_InteractCallbacks::add_param)>("inter.add_param", offsetof(PF_InteractCallbacks, add_param), first);
  field<decltype(PF_InteractCallbacks::abort)>("inter.abort", offsetof(PF_InteractCallbacks, abort), first);
  field<decltype(PF_InteractCallbacks::progress)>("inter.progress", offsetof(PF_InteractCallbacks, progress), first);
  field<decltype(PF_InteractCallbacks::register_ui)>("inter.register_ui", offsetof(PF_InteractCallbacks, register_ui), first);
  field<decltype(PF_InteractCallbacks::checkout_layer_audio)>("inter.checkout_layer_audio", offsetof(PF_InteractCallbacks, checkout_layer_audio), first);
  field<decltype(PF_InteractCallbacks::checkin_layer_audio)>("inter.checkin_layer_audio", offsetof(PF_InteractCallbacks, checkin_layer_audio), first);
  field<decltype(PF_InteractCallbacks::get_audio_data)>("inter.get_audio_data", offsetof(PF_InteractCallbacks, get_audio_data), first);
  field<decltype(PF_InteractCallbacks::reserved[0])>("inter.reserved_0", offsetof(PF_InteractCallbacks, reserved), first);
  field<decltype(PF_InteractCallbacks::reserved[0])>("inter.reserved_1", offsetof(PF_InteractCallbacks, reserved) + sizeof(void*), first);
  field<decltype(PF_InteractCallbacks::reserved[0])>("inter.reserved_2", offsetof(PF_InteractCallbacks, reserved) + 2 * sizeof(void*), first);
  field<decltype(PF_UtilCallbacks::begin_sampling)>("utils.begin_sampling", offsetof(PF_UtilCallbacks, begin_sampling), first);
  field<decltype(PF_UtilCallbacks::subpixel_sample)>("utils.subpixel_sample", offsetof(PF_UtilCallbacks, subpixel_sample), first);
  field<decltype(PF_UtilCallbacks::area_sample)>("utils.area_sample", offsetof(PF_UtilCallbacks, area_sample), first);
  field<decltype(PF_UtilCallbacks::end_sampling)>("utils.end_sampling", offsetof(PF_UtilCallbacks, end_sampling), first);
  field<decltype(PF_UtilCallbacks::host_new_handle)>("utils.host_new_handle", offsetof(PF_UtilCallbacks, host_new_handle), first);
  field<decltype(PF_UtilCallbacks::host_lock_handle)>("utils.host_lock_handle", offsetof(PF_UtilCallbacks, host_lock_handle), first);
  field<decltype(PF_UtilCallbacks::host_unlock_handle)>("utils.host_unlock_handle", offsetof(PF_UtilCallbacks, host_unlock_handle), first);
  field<decltype(PF_UtilCallbacks::host_dispose_handle)>("utils.host_dispose_handle", offsetof(PF_UtilCallbacks, host_dispose_handle), first);
  field<decltype(PF_UtilCallbacks::host_get_handle_size)>("utils.host_get_handle_size", offsetof(PF_UtilCallbacks, host_get_handle_size), first);
  field<decltype(PF_UtilCallbacks::iterate_origin_non_clip_src)>("utils.iterate_origin_non_clip_src", offsetof(PF_UtilCallbacks, iterate_origin_non_clip_src), first);
  field<decltype(PF_UtilCallbacks::iterate_generic)>("utils.iterate_generic", offsetof(PF_UtilCallbacks, iterate_generic), first);
  field<decltype(PF_UtilCallbacks::host_resize_handle)>("utils.host_resize_handle", offsetof(PF_UtilCallbacks, host_resize_handle), first);
  field<decltype(PF_ANSICallbacks::sin)>("utils.ansi_sin",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, sin), first);
  field<decltype(PF_ANSICallbacks::ceil)>("utils.ansi_ceil",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, ceil), first);
  field<decltype(PF_ANSICallbacks::cos)>("utils.ansi_cos",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, cos), first);
  field<decltype(PF_ANSICallbacks::fabs)>("utils.ansi_fabs",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, fabs), first);
  field<decltype(PF_ANSICallbacks::hypot)>("utils.ansi_hypot",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, hypot), first);
  field<decltype(PF_ANSICallbacks::pow)>("utils.ansi_pow",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, pow), first);
  field<decltype(PF_ANSICallbacks::sqrt)>("utils.ansi_sqrt",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, sqrt), first);
  field<decltype(PF_ANSICallbacks::sprintf)>("utils.ansi_sprintf",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, sprintf), first);
  field<decltype(PF_ANSICallbacks::strcpy)>("utils.ansi_strcpy",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, strcpy), first);
  field<decltype(PF_ANSICallbacks::asin)>("utils.ansi_asin",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, asin), first);
  field<decltype(PF_ANSICallbacks::acos)>("utils.ansi_acos",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, acos), first);
  field<decltype(PF_ANSICallbacks::atan)>("utils.ansi_atan",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, atan), first);
  field<decltype(PF_ANSICallbacks::atan2)>("utils.ansi_atan2",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, atan2), first);
  field<decltype(PF_ANSICallbacks::exp)>("utils.ansi_exp",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, exp), first);
  field<decltype(PF_ANSICallbacks::floor)>("utils.ansi_floor",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, floor), first);
  field<decltype(PF_ANSICallbacks::fmod)>("utils.ansi_fmod",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, fmod), first);
  field<decltype(PF_ANSICallbacks::log)>("utils.ansi_log",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, log), first);
  field<decltype(PF_ANSICallbacks::log10)>("utils.ansi_log10",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, log10), first);
  field<decltype(PF_ANSICallbacks::tan)>("utils.ansi_tan",
      offsetof(PF_UtilCallbacks, ansi) + offsetof(PF_ANSICallbacks, tan), first);
  field<decltype(PF_UtilCallbacks::colorCB)>("utils.color_callbacks", offsetof(PF_UtilCallbacks, colorCB), first);
  // composite_rect sits between end_sampling and blend. It was left out of the
  // emitted contract, so the host never wired it and Write_on's RENDER (paint
  // style "On Original Image") jumped to address 0 (issue #1252).
  field<decltype(PF_UtilCallbacks::composite_rect)>("utils.composite_rect", offsetof(PF_UtilCallbacks, composite_rect), first);
  field<decltype(PF_UtilCallbacks::blend)>("utils.blend", offsetof(PF_UtilCallbacks, blend), first);
  field<decltype(PF_UtilCallbacks::convolve)>("utils.convolve", offsetof(PF_UtilCallbacks, convolve), first);
  field<decltype(PF_UtilCallbacks::copy)>("utils.copy", offsetof(PF_UtilCallbacks, copy), first);
  field<decltype(PF_UtilCallbacks::fill)>("utils.fill", offsetof(PF_UtilCallbacks, fill), first);
  field<decltype(PF_UtilCallbacks::premultiply)>("utils.premultiply", offsetof(PF_UtilCallbacks, premultiply), first);
  field<decltype(PF_UtilCallbacks::premultiply_color)>("utils.premultiply_color", offsetof(PF_UtilCallbacks, premultiply_color), first);
  field<decltype(PF_UtilCallbacks::iterate)>("utils.iterate", offsetof(PF_UtilCallbacks, iterate), first);
  field<decltype(PF_UtilCallbacks::iterate16)>("utils.iterate16", offsetof(PF_UtilCallbacks, iterate16), first);
  field<decltype(PF_UtilCallbacks::iterate_origin)>("utils.iterate_origin", offsetof(PF_UtilCallbacks, iterate_origin), first);
  field<decltype(PF_UtilCallbacks::get_callback_addr)>("utils.get_callback_addr", offsetof(PF_UtilCallbacks, get_callback_addr), first);
  field<decltype(PF_UtilCallbacks::app)>("utils.app", offsetof(PF_UtilCallbacks, app), first);
  field<decltype(PF_UtilCallbacks::new_world)>("utils.new_world", offsetof(PF_UtilCallbacks, new_world), first);
  field<decltype(PF_UtilCallbacks::dispose_world)>("utils.dispose_world", offsetof(PF_UtilCallbacks, dispose_world), first);
  field<decltype(PF_UtilCallbacks::transfer_rect)>("utils.transfer_rect", offsetof(PF_UtilCallbacks, transfer_rect), first);
  field<decltype(PF_UtilCallbacks::transform_world)>("utils.transform_world", offsetof(PF_UtilCallbacks, transform_world), first);
  // The 16-bit sampling pair sits between host_resize_handle and fill16. Both
  // slots were left out of the emitted contract, so the host never wired them
  // and a 16-bit plug-in calling them jumped to address 0 (issue #777).
  field<decltype(PF_UtilCallbacks::subpixel_sample16)>("utils.subpixel_sample16", offsetof(PF_UtilCallbacks, subpixel_sample16), first);
  field<decltype(PF_UtilCallbacks::area_sample16)>("utils.area_sample16", offsetof(PF_UtilCallbacks, area_sample16), first);
  field<decltype(PF_UtilCallbacks::fill16)>("utils.fill16", offsetof(PF_UtilCallbacks, fill16), first);
  field<decltype(PF_UtilCallbacks::premultiply_color16)>("utils.premultiply_color16", offsetof(PF_UtilCallbacks, premultiply_color16), first);
  field<decltype(PF_UtilCallbacks::get_platform_data)>("utils.get_platform_data", offsetof(PF_UtilCallbacks, get_platform_data), first);
  field<decltype(PF_UtilCallbacks::get_pixel_data8)>("utils.get_pixel_data8", offsetof(PF_UtilCallbacks, get_pixel_data8), first);
  field<decltype(PF_UtilCallbacks::get_pixel_data16)>("utils.get_pixel_data16", offsetof(PF_UtilCallbacks, get_pixel_data16), first);
  field<decltype(PF_OutData::my_version)>("out.my_version", offsetof(PF_OutData, my_version), first);
  field<decltype(PF_OutData::start_sampL)>("out.start_samp", offsetof(PF_OutData, start_sampL), first);
  field<decltype(PF_OutData::dur_sampL)>("out.duration_samples", offsetof(PF_OutData, dur_sampL), first);
  field<decltype(PF_OutData::dest_snd)>("out.destination_sound", offsetof(PF_OutData, dest_snd), first);
  field<decltype(PF_OutData::global_data)>("out.global_data", offsetof(PF_OutData, global_data), first);
  field<decltype(PF_OutData::sequence_data)>("out.sequence_data", offsetof(PF_OutData, sequence_data), first);
  field<decltype(PF_OutData::frame_data)>("out.frame_data", offsetof(PF_OutData, frame_data), first);
  field<decltype(PF_OutData::width)>("out.width", offsetof(PF_OutData, width), first);
  field<decltype(PF_OutData::height)>("out.height", offsetof(PF_OutData, height), first);
  field<decltype(PF_OutData::origin)>("out.origin", offsetof(PF_OutData, origin), first);
  field<decltype(PF_OutData::out_flags)>("out.out_flags", offsetof(PF_OutData, out_flags), first);
  field<decltype(PF_OutData::num_params)>("out.num_params", offsetof(PF_OutData, num_params), first);
  field<decltype(PF_OutData::return_msg)>("out.return_msg", offsetof(PF_OutData, return_msg), first);
  field<decltype(PF_OutData::out_flags2)>("out.out_flags2", offsetof(PF_OutData, out_flags2), first);
  field<decltype(PF_ParamDef::uu)>("param.uu", offsetof(PF_ParamDef, uu), first);
  field<decltype(PF_ParamDef::ui_flags)>("param.ui_flags", offsetof(PF_ParamDef, ui_flags), first);
  field<decltype(PF_ParamDef::ui_width)>("param.ui_width", offsetof(PF_ParamDef, ui_width), first);
  field<decltype(PF_ParamDef::ui_height)>("param.ui_height", offsetof(PF_ParamDef, ui_height), first);
  field<decltype(PF_ParamDef::param_type)>("param.param_type", offsetof(PF_ParamDef, param_type), first);
  field<decltype(PF_ParamDef::name)>("param.name", offsetof(PF_ParamDef, name), first);
  field<decltype(PF_ParamDef::flags)>("param.flags", offsetof(PF_ParamDef, flags), first);
  field<decltype(PF_ParamDef::u)>("param.u", offsetof(PF_ParamDef, u), first);
  field<decltype(PF_ArbitraryDef::id)>("arbitrary.id", offsetof(PF_ArbitraryDef, id), first);
  field<decltype(PF_ArbitraryDef::dephault)>("arbitrary.default", offsetof(PF_ArbitraryDef, dephault), first);
  field<decltype(PF_ArbitraryDef::value)>("arbitrary.value", offsetof(PF_ArbitraryDef, value), first);
  field<decltype(PF_ArbitraryDef::refconPV)>("arbitrary.refcon", offsetof(PF_ArbitraryDef, refconPV), first);
  field<decltype(PF_ArbParamsExtra::which_function)>("arb_extra.which_function", offsetof(PF_ArbParamsExtra, which_function), first);
  field<decltype(PF_ArbParamsExtra::id)>("arb_extra.id", offsetof(PF_ArbParamsExtra, id), first);
  field<decltype(PF_ArbParamsExtra::u)>("arb_extra.u", offsetof(PF_ArbParamsExtra, u), first);
  field<decltype(PF_ArbParamsExtra::u.copy_func_params.refconPV)>("arb_copy.refcon", offsetof(PF_ArbParamsExtra, u.copy_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.copy_func_params.src_arbH)>("arb_copy.source", offsetof(PF_ArbParamsExtra, u.copy_func_params.src_arbH), first);
  field<decltype(PF_ArbParamsExtra::u.copy_func_params.dst_arbPH)>("arb_copy.destination", offsetof(PF_ArbParamsExtra, u.copy_func_params.dst_arbPH), first);
  field<decltype(PF_ArbParamsExtra::u.dispose_func_params.refconPV)>("arb_dispose.refcon", offsetof(PF_ArbParamsExtra, u.dispose_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.dispose_func_params.arbH)>("arb_dispose.value", offsetof(PF_ArbParamsExtra, u.dispose_func_params.arbH), first);
  field<decltype(PF_ArbParamsExtra::u.print_size_func_params.refconPV)>("arb_print_size.refcon", offsetof(PF_ArbParamsExtra, u.print_size_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.print_size_func_params.arbH)>("arb_print_size.value", offsetof(PF_ArbParamsExtra, u.print_size_func_params.arbH), first);
  field<decltype(PF_ArbParamsExtra::u.print_size_func_params.print_sizePLu)>("arb_print_size.output", offsetof(PF_ArbParamsExtra, u.print_size_func_params.print_sizePLu), first);
  field<decltype(PF_ArbParamsExtra::u.print_func_params.refconPV)>("arb_print.refcon", offsetof(PF_ArbParamsExtra, u.print_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.print_func_params.print_flags)>("arb_print.flags", offsetof(PF_ArbParamsExtra, u.print_func_params.print_flags), first);
  field<decltype(PF_ArbParamsExtra::u.print_func_params.arbH)>("arb_print.value", offsetof(PF_ArbParamsExtra, u.print_func_params.arbH), first);
  field<decltype(PF_ArbParamsExtra::u.print_func_params.print_sizeLu)>("arb_print.size", offsetof(PF_ArbParamsExtra, u.print_func_params.print_sizeLu), first);
  field<decltype(PF_ArbParamsExtra::u.print_func_params.print_bufferPC)>("arb_print.buffer", offsetof(PF_ArbParamsExtra, u.print_func_params.print_bufferPC), first);
  field<decltype(PF_ArbParamsExtra::u.flat_size_func_params.refconPV)>("arb_flat_size.refcon", offsetof(PF_ArbParamsExtra, u.flat_size_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.flat_size_func_params.arbH)>("arb_flat_size.value", offsetof(PF_ArbParamsExtra, u.flat_size_func_params.arbH), first);
  field<decltype(PF_ArbParamsExtra::u.flat_size_func_params.flat_data_sizePLu)>("arb_flat_size.output", offsetof(PF_ArbParamsExtra, u.flat_size_func_params.flat_data_sizePLu), first);
  field<decltype(PF_ArbParamsExtra::u.flatten_func_params.refconPV)>("arb_flatten.refcon", offsetof(PF_ArbParamsExtra, u.flatten_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.flatten_func_params.arbH)>("arb_flatten.value", offsetof(PF_ArbParamsExtra, u.flatten_func_params.arbH), first);
  field<decltype(PF_ArbParamsExtra::u.flatten_func_params.buf_sizeLu)>("arb_flatten.size", offsetof(PF_ArbParamsExtra, u.flatten_func_params.buf_sizeLu), first);
  field<decltype(PF_ArbParamsExtra::u.flatten_func_params.flat_dataPV)>("arb_flatten.buffer", offsetof(PF_ArbParamsExtra, u.flatten_func_params.flat_dataPV), first);
  field<decltype(PF_ArbParamsExtra::u.unflatten_func_params.refconPV)>("arb_unflatten.refcon", offsetof(PF_ArbParamsExtra, u.unflatten_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.unflatten_func_params.buf_sizeLu)>("arb_unflatten.size", offsetof(PF_ArbParamsExtra, u.unflatten_func_params.buf_sizeLu), first);
  field<decltype(PF_ArbParamsExtra::u.unflatten_func_params.flat_dataPV)>("arb_unflatten.buffer", offsetof(PF_ArbParamsExtra, u.unflatten_func_params.flat_dataPV), first);
  field<decltype(PF_ArbParamsExtra::u.unflatten_func_params.arbPH)>("arb_unflatten.output", offsetof(PF_ArbParamsExtra, u.unflatten_func_params.arbPH), first);
  field<decltype(PF_ArbParamsExtra::u.compare_func_params.refconPV)>("arb_compare.refcon", offsetof(PF_ArbParamsExtra, u.compare_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.compare_func_params.a_arbH)>("arb_compare.left", offsetof(PF_ArbParamsExtra, u.compare_func_params.a_arbH), first);
  field<decltype(PF_ArbParamsExtra::u.compare_func_params.b_arbH)>("arb_compare.right", offsetof(PF_ArbParamsExtra, u.compare_func_params.b_arbH), first);
  field<decltype(PF_ArbParamsExtra::u.compare_func_params.compareP)>("arb_compare.output", offsetof(PF_ArbParamsExtra, u.compare_func_params.compareP), first);
  field<decltype(PF_ArbParamsExtra::u.new_func_params.refconPV)>("arb_new.refcon", offsetof(PF_ArbParamsExtra, u.new_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.new_func_params.arbPH)>("arb_new.output", offsetof(PF_ArbParamsExtra, u.new_func_params.arbPH), first);
  field<decltype(PF_ArbParamsExtra::u.interp_func_params.refconPV)>("arb_interp.refcon", offsetof(PF_ArbParamsExtra, u.interp_func_params.refconPV), first);
  field<decltype(PF_ArbParamsExtra::u.interp_func_params.left_arbH)>("arb_interp.left", offsetof(PF_ArbParamsExtra, u.interp_func_params.left_arbH), first);
  field<decltype(PF_ArbParamsExtra::u.interp_func_params.right_arbH)>("arb_interp.right", offsetof(PF_ArbParamsExtra, u.interp_func_params.right_arbH), first);
  field<decltype(PF_ArbParamsExtra::u.interp_func_params.tF)>("arb_interp.amount", offsetof(PF_ArbParamsExtra, u.interp_func_params.tF), first);
  field<decltype(PF_ArbParamsExtra::u.interp_func_params.interpPH)>("arb_interp.output", offsetof(PF_ArbParamsExtra, u.interp_func_params.interpPH), first);
  field<decltype(PF_CustomUIInfo::reserved)>("custom_ui.reserved", offsetof(PF_CustomUIInfo, reserved), first);
  field<decltype(PF_CustomUIInfo::events)>("custom_ui.events", offsetof(PF_CustomUIInfo, events), first);
  field<decltype(PF_CustomUIInfo::comp_ui_width)>("custom_ui.comp_width", offsetof(PF_CustomUIInfo, comp_ui_width), first);
  field<decltype(PF_CustomUIInfo::comp_ui_height)>("custom_ui.comp_height", offsetof(PF_CustomUIInfo, comp_ui_height), first);
  field<decltype(PF_CustomUIInfo::comp_ui_alignment)>("custom_ui.comp_alignment", offsetof(PF_CustomUIInfo, comp_ui_alignment), first);
  field<decltype(PF_CustomUIInfo::layer_ui_width)>("custom_ui.layer_width", offsetof(PF_CustomUIInfo, layer_ui_width), first);
  field<decltype(PF_CustomUIInfo::layer_ui_height)>("custom_ui.layer_height", offsetof(PF_CustomUIInfo, layer_ui_height), first);
  field<decltype(PF_CustomUIInfo::layer_ui_alignment)>("custom_ui.layer_alignment", offsetof(PF_CustomUIInfo, layer_ui_alignment), first);
  field<decltype(PF_CustomUIInfo::preview_ui_width)>("custom_ui.preview_width", offsetof(PF_CustomUIInfo, preview_ui_width), first);
  field<decltype(PF_CustomUIInfo::preview_ui_height)>("custom_ui.preview_height", offsetof(PF_CustomUIInfo, preview_ui_height), first);
  field<decltype(PF_CustomUIInfo::preview_ui_alignment)>("custom_ui.preview_alignment", offsetof(PF_CustomUIInfo, preview_ui_alignment), first);
  field<decltype(PF_EventExtra::contextH)>("event.context", offsetof(PF_EventExtra, contextH), first);
  field<decltype(PF_EventExtra::e_type)>("event.type", offsetof(PF_EventExtra, e_type), first);
  field<decltype(PF_EventExtra::u)>("event.union", offsetof(PF_EventExtra, u), first);
  field<decltype(PF_EventExtra::effect_win)>("event.effect_window", offsetof(PF_EventExtra, effect_win), first);
  field<decltype(PF_EventExtra::cbs)>("event.callbacks", offsetof(PF_EventExtra, cbs), first);
  field<decltype(PF_EventExtra::evt_in_flags)>("event.in_flags", offsetof(PF_EventExtra, evt_in_flags), first);
  field<decltype(PF_EventExtra::evt_out_flags)>("event.out_flags", offsetof(PF_EventExtra, evt_out_flags), first);
  field<decltype(PF_Context::magic)>("context.magic", offsetof(PF_Context, magic), first);
  field<decltype(PF_Context::w_type)>("context.window_type", offsetof(PF_Context, w_type), first);
  field<decltype(PF_Context::plugin_state)>("context.plugin_state", offsetof(PF_Context, plugin_state), first);
  field<decltype(PF_Context::reserved_drawref)>("context.draw_ref", offsetof(PF_Context, reserved_drawref), first);
  field<decltype(PF_AdjustCursorEventInfo::screen_point)>("adjust_cursor.screen_point", offsetof(PF_AdjustCursorEventInfo, screen_point), first);
  field<decltype(PF_AdjustCursorEventInfo::modifiers)>("adjust_cursor.modifiers", offsetof(PF_AdjustCursorEventInfo, modifiers), first);
  field<decltype(PF_AdjustCursorEventInfo::set_cursor)>("adjust_cursor.set_cursor", offsetof(PF_AdjustCursorEventInfo, set_cursor), first);
  field<decltype(PF_DoClickEventInfo::when)>("do_click.when", offsetof(PF_DoClickEventInfo, when), first);
  field<decltype(PF_DoClickEventInfo::screen_point)>("do_click.screen_point", offsetof(PF_DoClickEventInfo, screen_point), first);
  field<decltype(PF_DoClickEventInfo::num_clicks)>("do_click.num_clicks", offsetof(PF_DoClickEventInfo, num_clicks), first);
  field<decltype(PF_DoClickEventInfo::modifiers)>("do_click.modifiers", offsetof(PF_DoClickEventInfo, modifiers), first);
  field<decltype(PF_DoClickEventInfo::continue_refcon)>("do_click.continue_refcon", offsetof(PF_DoClickEventInfo, continue_refcon), first);
  field<decltype(PF_DoClickEventInfo::send_drag)>("do_click.send_drag", offsetof(PF_DoClickEventInfo, send_drag), first);
  field<decltype(PF_DoClickEventInfo::last_time)>("do_click.last_time", offsetof(PF_DoClickEventInfo, last_time), first);
  field<decltype(PF_KeyDownEvent::when)>("key_down.when", offsetof(PF_KeyDownEvent, when), first);
  field<decltype(PF_KeyDownEvent::screen_point)>("key_down.screen_point", offsetof(PF_KeyDownEvent, screen_point), first);
  field<decltype(PF_KeyDownEvent::keycode)>("key_down.keycode", offsetof(PF_KeyDownEvent, keycode), first);
  field<decltype(PF_KeyDownEvent::modifiers)>("key_down.modifiers", offsetof(PF_KeyDownEvent, modifiers), first);
  field<decltype(PF_EffectWindowInfo::index)>("effect_window.index", offsetof(PF_EffectWindowInfo, index), first);
  field<decltype(PF_EffectWindowInfo::area)>("effect_window.area", offsetof(PF_EffectWindowInfo, area), first);
  field<decltype(PF_EffectWindowInfo::current_frame)>("effect_window.current_frame", offsetof(PF_EffectWindowInfo, current_frame), first);
  field<decltype(PF_AdvAppSuite2::PF_InfoDrawText)>("adv_app.info_text", offsetof(PF_AdvAppSuite2, PF_InfoDrawText), first);
  field<decltype(PF_AdvAppSuite2::PF_InfoDrawText3)>("adv_app.info_text3", offsetof(PF_AdvAppSuite2, PF_InfoDrawText3), first);
  field<decltype(DRAWBOT_DrawbotSuite1::GetSupplier)>("drawbot_draw.get_supplier", offsetof(DRAWBOT_DrawbotSuite1, GetSupplier), first);
  field<decltype(DRAWBOT_DrawbotSuite1::GetSurface)>("drawbot_draw.get_surface", offsetof(DRAWBOT_DrawbotSuite1, GetSurface), first);
  field<decltype(DRAWBOT_SupplierSuite1::NewPen)>("drawbot_supplier.new_pen", offsetof(DRAWBOT_SupplierSuite1, NewPen), first);
  field<decltype(DRAWBOT_SupplierSuite1::NewBrush)>("drawbot_supplier.new_brush", offsetof(DRAWBOT_SupplierSuite1, NewBrush), first);
  field<decltype(DRAWBOT_SupplierSuite1::NewPath)>("drawbot_supplier.new_path", offsetof(DRAWBOT_SupplierSuite1, NewPath), first);
  field<decltype(DRAWBOT_SupplierSuite1::ReleaseObject)>("drawbot_supplier.release", offsetof(DRAWBOT_SupplierSuite1, ReleaseObject), first);
  field<decltype(DRAWBOT_SurfaceSuite2::PaintRect)>("drawbot_surface.paint_rect", offsetof(DRAWBOT_SurfaceSuite2, PaintRect), first);
  field<decltype(DRAWBOT_SurfaceSuite2::FillPath)>("drawbot_surface.fill_path", offsetof(DRAWBOT_SurfaceSuite2, FillPath), first);
  field<decltype(DRAWBOT_SurfaceSuite2::StrokePath)>("drawbot_surface.stroke_path", offsetof(DRAWBOT_SurfaceSuite2, StrokePath), first);
  field<decltype(DRAWBOT_PathSuite1::AddRect)>("drawbot_path.add_rect", offsetof(DRAWBOT_PathSuite1, AddRect), first);
  field<decltype(PF_EffectCustomUISuite1::PF_GetDrawingReference)>("effect_custom_ui.get_drawing_ref", offsetof(PF_EffectCustomUISuite1, PF_GetDrawingReference), first);
  field<decltype(PF_EffectCustomUIOverlayThemeSuite1::PF_GetPreferredForegroundColor)>("overlay_theme.foreground", offsetof(PF_EffectCustomUIOverlayThemeSuite1, PF_GetPreferredForegroundColor), first);
  field<decltype(PF_EffectCustomUIOverlayThemeSuite1::PF_StrokePath)>("overlay_theme.stroke_path", offsetof(PF_EffectCustomUIOverlayThemeSuite1, PF_StrokePath), first);
  field<decltype(PFAppSuite4::PF_AppGetBgColor)>("app4.get_background_color", offsetof(PFAppSuite4, PF_AppGetBgColor), first);
  field<decltype(PFAppSuite4::PF_InvalidateRect)>("app4.invalidate_rect", offsetof(PFAppSuite4, PF_InvalidateRect), first);
  field<decltype(PFAppSuite4::PF_AppColorPickerDialog)>("app4.color_picker", offsetof(PFAppSuite4, PF_AppColorPickerDialog), first);
  field<decltype(PF_BatchSamplingSuite1::begin_sampling)>("batch_sampling.begin", offsetof(PF_BatchSamplingSuite1, begin_sampling), first);
  field<decltype(PF_BatchSamplingSuite1::end_sampling)>("batch_sampling.end", offsetof(PF_BatchSamplingSuite1, end_sampling), first);
  field<decltype(PF_BatchSamplingSuite1::get_batch_func)>("batch_sampling.get_func", offsetof(PF_BatchSamplingSuite1, get_batch_func), first);
  field<decltype(PF_BatchSamplingSuite1::get_batch_func16)>("batch_sampling.get_func16", offsetof(PF_BatchSamplingSuite1, get_batch_func16), first);
  field<decltype(PF_SliderDef::valid_min)>("slider.valid_min", offsetof(PF_SliderDef, valid_min), first);
  field<decltype(PF_SliderDef::valid_max)>("slider.valid_max", offsetof(PF_SliderDef, valid_max), first);
  field<decltype(PF_SliderDef::slider_min)>("slider.slider_min", offsetof(PF_SliderDef, slider_min), first);
  field<decltype(PF_SliderDef::slider_max)>("slider.slider_max", offsetof(PF_SliderDef, slider_max), first);
  field<decltype(PF_SliderDef::dephault)>("slider.default", offsetof(PF_SliderDef, dephault), first);
  field<decltype(PF_PopupDef::num_choices)>("popup.num_choices", offsetof(PF_PopupDef, num_choices), first);
  field<decltype(PF_PopupDef::dephault)>("popup.default", offsetof(PF_PopupDef, dephault), first);
  field<decltype(PF_PopupDef::u)>("popup.names", offsetof(PF_PopupDef, u), first);
  field<decltype(PF_CheckBoxDef::dephault)>("checkbox.default", offsetof(PF_CheckBoxDef, dephault), first);
  field<decltype(PF_CheckBoxDef::u)>("checkbox.label", offsetof(PF_CheckBoxDef, u), first);
  field<decltype(PF_FloatSliderDef::valid_min)>("float_slider.valid_min", offsetof(PF_FloatSliderDef, valid_min), first);
  field<decltype(PF_FloatSliderDef::valid_max)>("float_slider.valid_max", offsetof(PF_FloatSliderDef, valid_max), first);
  field<decltype(PF_FloatSliderDef::slider_min)>("float_slider.slider_min", offsetof(PF_FloatSliderDef, slider_min), first);
  field<decltype(PF_FloatSliderDef::slider_max)>("float_slider.slider_max", offsetof(PF_FloatSliderDef, slider_max), first);
  field<decltype(PF_FloatSliderDef::dephault)>("float_slider.default", offsetof(PF_FloatSliderDef, dephault), first);
  field<decltype(PF_FloatSliderDef::precision)>("float_slider.precision", offsetof(PF_FloatSliderDef, precision), first);
  field<decltype(PF_LayerDef::width)>("layer.width", offsetof(PF_LayerDef, width), first);
  field<decltype(PF_LayerDef::height)>("layer.height", offsetof(PF_LayerDef, height), first);
  field<decltype(PF_LayerDef::rowbytes)>("layer.rowbytes", offsetof(PF_LayerDef, rowbytes), first);
  field<decltype(PF_LayerDef::data)>("layer.data", offsetof(PF_LayerDef, data), first);
  field<decltype(PF_LayerDef::world_flags)>("layer.world_flags", offsetof(PF_LayerDef, world_flags), first);
  field<decltype(PF_LayerDef::extent_hint)>("layer.extent_hint", offsetof(PF_LayerDef, extent_hint), first);
  field<decltype(PF_LayerDef::pix_aspect_ratio)>(
      "layer.pix_aspect_ratio", offsetof(PF_LayerDef, pix_aspect_ratio), first);
  field<decltype(PF_Pixel::alpha)>("pixel.alpha", offsetof(PF_Pixel, alpha), first);
  field<decltype(PF_Pixel::red)>("pixel.red", offsetof(PF_Pixel, red), first);
  field<decltype(PF_Pixel::green)>("pixel.green", offsetof(PF_Pixel, green), first);
  field<decltype(PF_Pixel::blue)>("pixel.blue", offsetof(PF_Pixel, blue), first);
  field<decltype(PF_Pixel16::alpha)>("pixel16.alpha", offsetof(PF_Pixel16, alpha), first);
  field<decltype(PF_Pixel16::red)>("pixel16.red", offsetof(PF_Pixel16, red), first);
  field<decltype(PF_Pixel16::green)>("pixel16.green", offsetof(PF_Pixel16, green), first);
  field<decltype(PF_Pixel16::blue)>("pixel16.blue", offsetof(PF_Pixel16, blue), first);
  field<decltype(PF_PixelFloat::alpha)>("pixel_float.alpha", offsetof(PF_PixelFloat, alpha), first);
  field<decltype(PF_PixelFloat::red)>("pixel_float.red", offsetof(PF_PixelFloat, red), first);
  field<decltype(PF_PixelFloat::green)>("pixel_float.green", offsetof(PF_PixelFloat, green), first);
  field<decltype(PF_PixelFloat::blue)>("pixel_float.blue", offsetof(PF_PixelFloat, blue), first);
  field<decltype(PF_PreRenderExtra::input)>("pre_extra.input", offsetof(PF_PreRenderExtra, input), first);
  field<decltype(PF_PreRenderExtra::output)>("pre_extra.output", offsetof(PF_PreRenderExtra, output), first);
  field<decltype(PF_PreRenderExtra::cb)>("pre_extra.callbacks", offsetof(PF_PreRenderExtra, cb), first);
  field<decltype(PF_PreRenderInput::output_request)>("pre_input.output_request", offsetof(PF_PreRenderInput, output_request), first);
  field<decltype(PF_PreRenderInput::bitdepth)>("pre_input.bitdepth", offsetof(PF_PreRenderInput, bitdepth), first);
  field<decltype(PF_PreRenderInput::gpu_data)>("pre_input.gpu_data", offsetof(PF_PreRenderInput, gpu_data), first);
  field<decltype(PF_PreRenderInput::what_gpu)>("pre_input.what_gpu", offsetof(PF_PreRenderInput, what_gpu), first);
  field<decltype(PF_PreRenderInput::device_index)>("pre_input.device_index", offsetof(PF_PreRenderInput, device_index), first);
  field<decltype(PF_PreRenderOutput::result_rect)>("pre_output.result_rect", offsetof(PF_PreRenderOutput, result_rect), first);
  field<decltype(PF_PreRenderOutput::max_result_rect)>("pre_output.max_result_rect", offsetof(PF_PreRenderOutput, max_result_rect), first);
  field<decltype(PF_PreRenderOutput::flags)>("pre_output.flags", offsetof(PF_PreRenderOutput, flags), first);
  field<decltype(PF_PreRenderOutput::pre_render_data)>("pre_output.pre_render_data", offsetof(PF_PreRenderOutput, pre_render_data), first);
  field<decltype(PF_PreRenderCallbacks::checkout_layer)>("pre_callbacks.checkout_layer", offsetof(PF_PreRenderCallbacks, checkout_layer), first);
  field<decltype(PF_SmartRenderExtra::input)>("smart_extra.input", offsetof(PF_SmartRenderExtra, input), first);
  field<decltype(PF_SmartRenderExtra::cb)>("smart_extra.callbacks", offsetof(PF_SmartRenderExtra, cb), first);
  field<decltype(PF_SmartRenderCallbacks::checkout_layer_pixels)>("smart_callbacks.checkout_layer_pixels", offsetof(PF_SmartRenderCallbacks, checkout_layer_pixels), first);
  field<decltype(PF_SmartRenderCallbacks::checkin_layer_pixels)>("smart_callbacks.checkin_layer_pixels", offsetof(PF_SmartRenderCallbacks, checkin_layer_pixels), first);
  field<decltype(PF_SmartRenderCallbacks::checkout_output)>("smart_callbacks.checkout_output", offsetof(PF_SmartRenderCallbacks, checkout_output), first);
  field<decltype(PF_SmartRenderInput::bitdepth)>("smart_input.bitdepth", offsetof(PF_SmartRenderInput, bitdepth), first);
  field<decltype(PF_SmartRenderInput::pre_render_data)>("smart_input.pre_render_data", offsetof(PF_SmartRenderInput, pre_render_data), first);
  field<decltype(PF_SmartRenderInput::gpu_data)>("smart_input.gpu_data", offsetof(PF_SmartRenderInput, gpu_data), first);
  field<decltype(PF_SmartRenderInput::what_gpu)>("smart_input.what_gpu", offsetof(PF_SmartRenderInput, what_gpu), first);
  field<decltype(PF_SmartRenderInput::device_index)>("smart_input.device_index", offsetof(PF_SmartRenderInput, device_index), first);
  field<decltype(PF_GPUDeviceSetupExtra::input)>("gpu_setup_extra.input", offsetof(PF_GPUDeviceSetupExtra, input), first);
  field<decltype(PF_GPUDeviceSetupExtra::output)>("gpu_setup_extra.output", offsetof(PF_GPUDeviceSetupExtra, output), first);
  field<decltype(PF_GPUDeviceSetupInput::what_gpu)>("gpu_setup_input.what_gpu", offsetof(PF_GPUDeviceSetupInput, what_gpu), first);
  field<decltype(PF_GPUDeviceSetupInput::device_index)>("gpu_setup_input.device_index", offsetof(PF_GPUDeviceSetupInput, device_index), first);
  field<decltype(PF_GPUDeviceSetupOutput::gpu_data)>("gpu_setup_output.gpu_data", offsetof(PF_GPUDeviceSetupOutput, gpu_data), first);
  field<decltype(PF_GPUDeviceSetdownExtra::input)>("gpu_setdown_extra.input", offsetof(PF_GPUDeviceSetdownExtra, input), first);
  field<decltype(PF_GPUDeviceSetdownInput::gpu_data)>("gpu_setdown_input.gpu_data", offsetof(PF_GPUDeviceSetdownInput, gpu_data), first);
  field<decltype(PF_GPUDeviceSetdownInput::what_gpu)>("gpu_setdown_input.what_gpu", offsetof(PF_GPUDeviceSetdownInput, what_gpu), first);
  field<decltype(PF_GPUDeviceSetdownInput::device_index)>("gpu_setdown_input.device_index", offsetof(PF_GPUDeviceSetdownInput, device_index), first);
  field<decltype(PF_UserChangedParamExtra::param_index)>("user_changed.param_index", offsetof(PF_UserChangedParamExtra, param_index), first);
  field<decltype(AEGP_CommandSuite1::AEGP_GetUniqueCommand)>("aegp_command.get_unique_command", offsetof(AEGP_CommandSuite1, AEGP_GetUniqueCommand), first);
  field<decltype(AEGP_CommandSuite1::AEGP_InsertMenuCommand)>("aegp_command.insert_menu_command", offsetof(AEGP_CommandSuite1, AEGP_InsertMenuCommand), first);
  field<decltype(AEGP_CommandSuite1::AEGP_DoCommand)>("aegp_command.do_command", offsetof(AEGP_CommandSuite1, AEGP_DoCommand), first);
  field<decltype(AEGP_RegisterSuite5::AEGP_RegisterCommandHook)>("aegp_register.command_hook", offsetof(AEGP_RegisterSuite5, AEGP_RegisterCommandHook), first);
  field<decltype(AEGP_RegisterSuite5::AEGP_RegisterUpdateMenuHook)>("aegp_register.update_menu_hook", offsetof(AEGP_RegisterSuite5, AEGP_RegisterUpdateMenuHook), first);
  field<decltype(AEGP_RegisterSuite5::AEGP_RegisterDeathHook)>("aegp_register.death_hook", offsetof(AEGP_RegisterSuite5, AEGP_RegisterDeathHook), first);
  field<decltype(AEGP_RegisterSuite5::AEGP_RegisterIdleHook)>("aegp_register.idle_hook", offsetof(AEGP_RegisterSuite5, AEGP_RegisterIdleHook), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetActiveItem)>("aegp_item.get_active_item", offsetof(AEGP_ItemSuite9, AEGP_GetActiveItem), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetItemType)>("aegp_item.get_type", offsetof(AEGP_ItemSuite9, AEGP_GetItemType), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetItemName)>("aegp_item.get_name", offsetof(AEGP_ItemSuite9, AEGP_GetItemName), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetItemDuration)>("aegp_item.get_duration", offsetof(AEGP_ItemSuite9, AEGP_GetItemDuration), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetItemCurrentTime)>("aegp_item.get_current_time", offsetof(AEGP_ItemSuite9, AEGP_GetItemCurrentTime), first);
  field<decltype(AEGP_ItemSuite9::AEGP_SetItemCurrentTime)>("aegp_item.set_current_time", offsetof(AEGP_ItemSuite9, AEGP_SetItemCurrentTime), first);
  field<decltype(AEGP_ItemSuite9::AEGP_GetItemID)>("aegp_item.get_id", offsetof(AEGP_ItemSuite9, AEGP_GetItemID), first);
  static_assert(sizeof(AEGP_ItemSuite9) == 26 * sizeof(void*));
  static_assert(offsetof(AEGP_ItemSuite9, AEGP_GetItemType) == 5 * sizeof(void*),
                "AEGP Item Suite 9 GetItemType must remain at slot 5 (offset 40)");
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerInPoint)>("aegp_layer.get_in_point", offsetof(AEGP_LayerSuite9, AEGP_GetLayerInPoint), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerDuration)>("aegp_layer.get_duration", offsetof(AEGP_LayerSuite9, AEGP_GetLayerDuration), first);
  field<decltype(AEGP_LayerSuite9::AEGP_SetLayerInPointAndDuration)>("aegp_layer.set_in_point_and_duration", offsetof(AEGP_LayerSuite9, AEGP_SetLayerInPointAndDuration), first);
  field<decltype(AEGP_LayerSuite8::AEGP_GetLayerFlags)>("aegp_layer.get_flags", offsetof(AEGP_LayerSuite8, AEGP_GetLayerFlags), first);
  field<decltype(AEGP_LayerSuite8::AEGP_SetLayerFlag)>("aegp_layer.set_flag", offsetof(AEGP_LayerSuite8, AEGP_SetLayerFlag), first);
  field<decltype(AEGP_CompSuite11::AEGP_GetCompFromItem)>("aegp_comp.get_from_item", offsetof(AEGP_CompSuite11, AEGP_GetCompFromItem), first);
  field<decltype(AEGP_CompSuite11::AEGP_GetCompFrameDuration)>("aegp_comp.get_frame_duration", offsetof(AEGP_CompSuite11, AEGP_GetCompFrameDuration), first);
  field<decltype(AEGP_CompSuite11::AEGP_GetCompFramerate)>("aegp_comp.get_framerate", offsetof(AEGP_CompSuite11, AEGP_GetCompFramerate), first);
  field<decltype(AEGP_CompSuite11::AEGP_GetNewCollectionFromCompSelection)>("aegp_comp.get_selection", offsetof(AEGP_CompSuite11, AEGP_GetNewCollectionFromCompSelection), first);
  field<decltype(AEGP_LayerSuite5::AEGP_GetCompNumLayers)>("aegp_layer5.get_comp_num_layers", offsetof(AEGP_LayerSuite5, AEGP_GetCompNumLayers), first);
  field<decltype(AEGP_LayerSuite5::AEGP_GetCompLayerByIndex)>("aegp_layer5.get_by_index", offsetof(AEGP_LayerSuite5, AEGP_GetCompLayerByIndex), first);
  field<decltype(AEGP_LayerSuite5::AEGP_GetLayerSourceItem)>("aegp_layer5.get_source_item", offsetof(AEGP_LayerSuite5, AEGP_GetLayerSourceItem), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetActiveLayer)>("aegp_layer9.get_active", offsetof(AEGP_LayerSuite9, AEGP_GetActiveLayer), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerIndex)>("aegp_layer9.get_index", offsetof(AEGP_LayerSuite9, AEGP_GetLayerIndex), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerParentComp)>("aegp_layer9.get_parent_comp", offsetof(AEGP_LayerSuite9, AEGP_GetLayerParentComp), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerName)>("aegp_layer9.get_name", offsetof(AEGP_LayerSuite9, AEGP_GetLayerName), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerID)>("aegp_layer9.get_id", offsetof(AEGP_LayerSuite9, AEGP_GetLayerID), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerParent)>("aegp_layer9.get_parent", offsetof(AEGP_LayerSuite9, AEGP_GetLayerParent), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerFromLayerID)>("aegp_layer9.get_from_id", offsetof(AEGP_LayerSuite9, AEGP_GetLayerFromLayerID), first);
  field<decltype(AEGP_LayerSuite8::AEGP_GetLayerFlags)>("aegp_layer8.get_flags", offsetof(AEGP_LayerSuite8, AEGP_GetLayerFlags), first);
  field<decltype(AEGP_LayerSuite8::AEGP_GetLayerTransferMode)>("aegp_layer8.get_transfer_mode", offsetof(AEGP_LayerSuite8, AEGP_GetLayerTransferMode), first);
  field<decltype(AEGP_LayerSuite9::AEGP_GetLayerObjectType)>("aegp_layer9.get_object_type", offsetof(AEGP_LayerSuite9, AEGP_GetLayerObjectType), first);
  field<decltype(AEGP_LayerSuite5::AEGP_GetLayerInPoint)>("aegp_layer5.get_in_point", offsetof(AEGP_LayerSuite5, AEGP_GetLayerInPoint), first);
  field<decltype(AEGP_LayerSuite5::AEGP_GetLayerDuration)>("aegp_layer5.get_duration", offsetof(AEGP_LayerSuite5, AEGP_GetLayerDuration), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetLayerNumEffects)>("aegp_effect.get_layer_count", offsetof(AEGP_EffectSuite4, AEGP_GetLayerNumEffects), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetLayerEffectByIndex)>("aegp_effect.get_by_index", offsetof(AEGP_EffectSuite4, AEGP_GetLayerEffectByIndex), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetInstalledKeyFromLayerEffect)>("aegp_effect.get_installed_key", offsetof(AEGP_EffectSuite4, AEGP_GetInstalledKeyFromLayerEffect), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetEffectParamUnionByIndex)>("aegp_effect.get_param_union_by_index", offsetof(AEGP_EffectSuite4, AEGP_GetEffectParamUnionByIndex), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetEffectFlags)>("aegp_effect.get_flags", offsetof(AEGP_EffectSuite4, AEGP_GetEffectFlags), first);
  field<decltype(AEGP_EffectSuite4::AEGP_DisposeEffect)>("aegp_effect.dispose", offsetof(AEGP_EffectSuite4, AEGP_DisposeEffect), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetNumInstalledEffects)>("aegp_effect.get_installed_count", offsetof(AEGP_EffectSuite4, AEGP_GetNumInstalledEffects), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetNextInstalledEffect)>("aegp_effect.get_next_installed", offsetof(AEGP_EffectSuite4, AEGP_GetNextInstalledEffect), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetEffectName)>("aegp_effect.get_name", offsetof(AEGP_EffectSuite4, AEGP_GetEffectName), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetEffectMatchName)>("aegp_effect.get_match_name", offsetof(AEGP_EffectSuite4, AEGP_GetEffectMatchName), first);
  field<decltype(AEGP_EffectSuite4::AEGP_GetEffectCategory)>("aegp_effect.get_category", offsetof(AEGP_EffectSuite4, AEGP_GetEffectCategory), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetEffectNumParamStreams)>("aegp_stream.get_effect_param_count", offsetof(AEGP_StreamSuite6, AEGP_GetEffectNumParamStreams), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetNewEffectStreamByIndex)>("aegp_stream.get_new_effect", offsetof(AEGP_StreamSuite6, AEGP_GetNewEffectStreamByIndex), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetStreamName)>("aegp_stream.get_name", offsetof(AEGP_StreamSuite6, AEGP_GetStreamName), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetNewLayerStream)>("aegp_stream.get_new_layer", offsetof(AEGP_StreamSuite6, AEGP_GetNewLayerStream), first);
  field<decltype(AEGP_StreamSuite6::AEGP_DisposeStream)>("aegp_stream.dispose", offsetof(AEGP_StreamSuite6, AEGP_DisposeStream), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetStreamType)>("aegp_stream.get_type", offsetof(AEGP_StreamSuite6, AEGP_GetStreamType), first);
  field<decltype(AEGP_StreamSuite6::AEGP_GetNewStreamValue)>("aegp_stream.get_value", offsetof(AEGP_StreamSuite6, AEGP_GetNewStreamValue), first);
  field<decltype(AEGP_StreamSuite6::AEGP_DisposeStreamValue)>("aegp_stream.dispose_value", offsetof(AEGP_StreamSuite6, AEGP_DisposeStreamValue), first);
  field<decltype(AEGP_StreamSuite6::AEGP_SetStreamValue)>("aegp_stream.set_value", offsetof(AEGP_StreamSuite6, AEGP_SetStreamValue), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetStreamNumKFs)>("aegp_keyframe.get_stream_count", offsetof(AEGP_KeyframeSuite5, AEGP_GetStreamNumKFs), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetKeyframeTime)>("aegp_keyframe.get_time", offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeTime), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetNewKeyframeValue)>("aegp_keyframe.get_value", offsetof(AEGP_KeyframeSuite5, AEGP_GetNewKeyframeValue), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetKeyframeInterpolation)>("aegp_keyframe.get_interpolation", offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeInterpolation), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_InsertKeyframe)>("aegp_keyframe.insert", offsetof(AEGP_KeyframeSuite5, AEGP_InsertKeyframe), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_DeleteKeyframe)>("aegp_keyframe.delete", offsetof(AEGP_KeyframeSuite5, AEGP_DeleteKeyframe), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeValue)>("aegp_keyframe.set_value", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeValue), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetStreamValueDimensionality)>("aegp_keyframe.value_dimensionality", offsetof(AEGP_KeyframeSuite5, AEGP_GetStreamValueDimensionality), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetStreamTemporalDimensionality)>("aegp_keyframe.temporal_dimensionality", offsetof(AEGP_KeyframeSuite5, AEGP_GetStreamTemporalDimensionality), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetNewKeyframeSpatialTangents)>("aegp_keyframe.get_spatial_tangents", offsetof(AEGP_KeyframeSuite5, AEGP_GetNewKeyframeSpatialTangents), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeSpatialTangents)>("aegp_keyframe.set_spatial_tangents", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeSpatialTangents), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetKeyframeTemporalEase)>("aegp_keyframe.get_temporal_ease", offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeTemporalEase), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeTemporalEase)>("aegp_keyframe.set_temporal_ease", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeTemporalEase), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetKeyframeFlags)>("aegp_keyframe.get_flags", offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeFlags), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeFlag)>("aegp_keyframe.set_flag", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeFlag), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeInterpolation)>("aegp_keyframe.set_interpolation", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeInterpolation), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_StartAddKeyframes)>("aegp_keyframe.start_add", offsetof(AEGP_KeyframeSuite5, AEGP_StartAddKeyframes), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_AddKeyframes)>("aegp_keyframe.add", offsetof(AEGP_KeyframeSuite5, AEGP_AddKeyframes), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetAddKeyframe)>("aegp_keyframe.set_add", offsetof(AEGP_KeyframeSuite5, AEGP_SetAddKeyframe), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_EndAddKeyframes)>("aegp_keyframe.end_add", offsetof(AEGP_KeyframeSuite5, AEGP_EndAddKeyframes), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_GetKeyframeLabelColorIndex)>("aegp_keyframe.get_label", offsetof(AEGP_KeyframeSuite5, AEGP_GetKeyframeLabelColorIndex), first);
  field<decltype(AEGP_KeyframeSuite5::AEGP_SetKeyframeLabelColorIndex)>("aegp_keyframe.set_label", offsetof(AEGP_KeyframeSuite5, AEGP_SetKeyframeLabelColorIndex), first);
  field<decltype(AEGP_CollectionSuite2::AEGP_DisposeCollection)>("aegp_collection.dispose", offsetof(AEGP_CollectionSuite2, AEGP_DisposeCollection), first);
  field<decltype(AEGP_CollectionSuite2::AEGP_GetCollectionNumItems)>("aegp_collection.get_count", offsetof(AEGP_CollectionSuite2, AEGP_GetCollectionNumItems), first);
  field<decltype(AEGP_CollectionSuite2::AEGP_GetCollectionItemByIndex)>("aegp_collection.get_by_index", offsetof(AEGP_CollectionSuite2, AEGP_GetCollectionItemByIndex), first);
  field<decltype(AEGP_CollectionItemV2::type)>("aegp_collection_item.type", offsetof(AEGP_CollectionItemV2, type), first);
  field<decltype(AEGP_CollectionItemV2::u)>("aegp_collection_item.union", offsetof(AEGP_CollectionItemV2, u), first);
  field<decltype(AEGP_CollectionItemV2::stream_refH)>("aegp_collection_item.stream_ref", offsetof(AEGP_CollectionItemV2, stream_refH), first);
  field<decltype(AEGP_StreamValue2::streamH)>("aegp_stream_value.stream", offsetof(AEGP_StreamValue2, streamH), first);
  field<decltype(AEGP_StreamValue2::val)>("aegp_stream_value.val", offsetof(AEGP_StreamValue2, val), first);
  std::cout << "\n  },\n  \"selectors\":{"
            << "\"about\":" << static_cast<int>(PF_Cmd_ABOUT) << ','
            << "\"global_setup\":" << static_cast<int>(PF_Cmd_GLOBAL_SETUP) << ','
            << "\"global_setdown\":" << static_cast<int>(PF_Cmd_GLOBAL_SETDOWN) << ','
            << "\"params_setup\":" << static_cast<int>(PF_Cmd_PARAMS_SETUP)
            << ",\"sequence_setup\":" << static_cast<int>(PF_Cmd_SEQUENCE_SETUP)
            << ",\"sequence_resetup\":" << static_cast<int>(PF_Cmd_SEQUENCE_RESETUP)
            << ",\"sequence_flatten\":" << static_cast<int>(PF_Cmd_SEQUENCE_FLATTEN)
            << ",\"sequence_setdown\":" << static_cast<int>(PF_Cmd_SEQUENCE_SETDOWN)
            << ",\"do_dialog\":" << static_cast<int>(PF_Cmd_DO_DIALOG)
            << ",\"frame_setup\":" << static_cast<int>(PF_Cmd_FRAME_SETUP)
            << ",\"frame_setdown\":" << static_cast<int>(PF_Cmd_FRAME_SETDOWN)
            << ",\"render\":" << static_cast<int>(PF_Cmd_RENDER)
            << ",\"user_changed_param\":" << static_cast<int>(PF_Cmd_USER_CHANGED_PARAM)
            << ",\"update_params_ui\":" << static_cast<int>(PF_Cmd_UPDATE_PARAMS_UI)
            << ",\"query_dynamic_flags\":" << static_cast<int>(PF_Cmd_QUERY_DYNAMIC_FLAGS)
            << ",\"event\":" << static_cast<int>(PF_Cmd_EVENT)
            << ",\"get_external_dependencies\":"
            << static_cast<int>(PF_Cmd_GET_EXTERNAL_DEPENDENCIES)
            << ",\"arbitrary_callback\":" << static_cast<int>(PF_Cmd_ARBITRARY_CALLBACK)
            << ",\"get_flattened_sequence_data\":"
            << static_cast<int>(PF_Cmd_GET_FLATTENED_SEQUENCE_DATA)
            << ",\"smart_pre_render\":" << static_cast<int>(PF_Cmd_SMART_PRE_RENDER)
            << ",\"smart_render\":" << static_cast<int>(PF_Cmd_SMART_RENDER)
            << ",\"smart_render_gpu\":" << static_cast<int>(PF_Cmd_SMART_RENDER_GPU)
            << ",\"gpu_device_setup\":" << static_cast<int>(PF_Cmd_GPU_DEVICE_SETUP)
            << ",\"gpu_device_setdown\":" << static_cast<int>(PF_Cmd_GPU_DEVICE_SETDOWN)
            << "},\n  \"gpu_frameworks\":{\"opencl\":"
            << static_cast<int>(PF_GPU_Framework_OPENCL)
            << "},\n  \"render_output_flags\":{\"gpu_render_possible\":"
            << static_cast<uint32_t>(PF_RenderOutputFlag_GPU_RENDER_POSSIBLE)
            << "},\n  \"out_flags\":{\"i_do_dialog\":"
            << static_cast<uint32_t>(PF_OutFlag_I_DO_DIALOG)
            << ",\"wide_time_input\":"
            << static_cast<uint32_t>(PF_OutFlag_WIDE_TIME_INPUT)
            << ",\"send_do_dialog\":"
            << static_cast<uint32_t>(PF_OutFlag_SEND_DO_DIALOG)
            << ",\"i_expand_buffer\":"
            << static_cast<uint32_t>(PF_OutFlag_I_EXPAND_BUFFER)
            << ",\"i_shrink_buffer\":"
            << static_cast<uint32_t>(PF_OutFlag_I_SHRINK_BUFFER)
            << ",\"i_use_shutter_angle\":"
            << static_cast<uint32_t>(PF_OutFlag_I_USE_SHUTTER_ANGLE)
            << ",\"i_use_audio\":"
            << static_cast<uint32_t>(PF_OutFlag_I_USE_AUDIO)
            << ",\"nop_render\":"
            << static_cast<uint32_t>(PF_OutFlag_NOP_RENDER)
            << ",\"i_write_input_buffer\":"
            << static_cast<uint32_t>(PF_OutFlag_I_WRITE_INPUT_BUFFER)
            << ",\"display_error_message\":"
            << static_cast<uint32_t>(PF_OutFlag_DISPLAY_ERROR_MESSAGE)
            << "},\n  \"out_flags2\":{\"automatic_wide_time_input\":"
            << static_cast<uint32_t>(PF_OutFlag2_AUTOMATIC_WIDE_TIME_INPUT)
            << "},\n  \"native_aex_loaded\":false,\n"
               "  \"selector_dispatched\":false\n}\n";
}
