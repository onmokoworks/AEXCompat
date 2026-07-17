#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <iostream>
#include <type_traits>

namespace {
using Suite = AEGP_ColorSettingsSuite6;

#define SLOT(field, index, signature)                                      \
  static_assert(offsetof(Suite, field) == (index) * sizeof(void*));         \
  static_assert(std::is_same_v<decltype(Suite::field), signature>)

SLOT(AEGP_GetBlendingTables, 0,
     A_Err (*)(PR_RenderContextH, PF_EffectBlendingTables*));
SLOT(AEGP_DoesViewHaveColorSpaceXform, 1,
     A_Err (*)(AEGP_ItemViewP, A_Boolean*));
SLOT(AEGP_XformWorkingToViewColorSpace, 2,
     A_Err (*)(AEGP_ItemViewP, AEGP_WorldH, AEGP_WorldH));
SLOT(AEGP_GetNewWorkingSpaceColorProfile, 3,
     A_Err (*)(AEGP_PluginID, AEGP_CompH, AEGP_ColorProfileP*));
SLOT(AEGP_GetNewColorProfileFromICCProfile, 4,
     A_Err (*)(AEGP_PluginID, A_long, const void*, AEGP_ColorProfileP*));
SLOT(AEGP_GetNewICCProfileFromColorProfile, 5,
     A_Err (*)(AEGP_PluginID, AEGP_ConstColorProfileP, AEGP_MemHandle*));
SLOT(AEGP_GetNewColorProfileDescription, 6,
     A_Err (*)(AEGP_PluginID, AEGP_ConstColorProfileP, AEGP_MemHandle*));
SLOT(AEGP_DisposeColorProfile, 7, A_Err (*)(AEGP_ColorProfileP));
SLOT(AEGP_GetColorProfileApproximateGamma, 8,
     A_Err (*)(AEGP_ConstColorProfileP, A_FpShort*));
SLOT(AEGP_IsRGBColorProfile, 9,
     A_Err (*)(AEGP_ConstColorProfileP, A_Boolean*));
SLOT(AEGP_SetWorkingColorSpace, 10,
     A_Err (*)(AEGP_PluginID, AEGP_CompH, AEGP_ConstColorProfileP));
SLOT(AEGP_IsOCIOColorManagementUsed, 11,
     A_Err (*)(AEGP_PluginID, A_Boolean*));
SLOT(AEGP_GetOCIOConfigurationFile, 12,
     A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
SLOT(AEGP_GetOCIOConfigurationFilePath, 13,
     A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
SLOT(AEGPD_GetOCIOWorkingColorSpace, 14,
     A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
SLOT(AEGPD_GetOCIODisplayColorSpace, 15,
     A_Err (*)(AEGP_PluginID, AEGP_MemHandle*, AEGP_MemHandle*));
SLOT(AEGPD_IsColorSpaceAwareEffectsEnabled, 16,
     A_Err (*)(AEGP_PluginID, A_Boolean*));
SLOT(AEGPD_GetLUTInterpolationMethod, 17,
     A_Err (*)(AEGP_PluginID, A_u_short*));
SLOT(AEGPD_GetGraphicsWhiteLuminance, 18,
     A_Err (*)(AEGP_PluginID, A_u_short*));
SLOT(AEGPD_GetWorkingColorSpaceId, 19,
     A_Err (*)(AEGP_PluginID, AEGP_GuidP));

static_assert(std::is_standard_layout_v<Suite>);
static_assert(sizeof(Suite) == 20 * sizeof(void*));
static_assert(kAEGPColorSettingsSuiteVersion6 == 7);

void scalar(const char* name, std::size_t size, bool& first) {
  if (!first) std::cout << ',';
  first = false;
  std::cout << "\"" << name << "\":" << size;
}

void member(const char* name, std::size_t offset, std::size_t size,
            bool type_matches, bool& first) {
  if (!first) std::cout << ',';
  first = false;
  std::cout << "\n      \"" << name << "\":{\"offset\":" << offset
            << ",\"size\":" << size << ",\"type_matches\":"
            << (type_matches ? "true" : "false") << '}';
}

#define EMIT(field, signature)                                             \
  member(#field, offsetof(Suite, field), sizeof(decltype(Suite::field)),    \
         std::is_same_v<decltype(Suite::field), signature>, first)
}  // namespace

int main() {
  std::cout << "{\n  \"schema_version\":1,\n"
               "  \"source_kind\":\"compiled_sdk_header_observation\",\n"
               "  \"architecture\":\"x86_64-windows\",\n"
               "  \"acquisition\":{\"name\":\"" << kAEGPColorSettingsSuite
            << "\",\"version\":" << kAEGPColorSettingsSuiteVersion6
            << "},\n  \"types\":{";
  bool first = true;
  scalar("pointer", sizeof(void*), first);
  scalar("A_Err", sizeof(A_Err), first);
  scalar("A_long", sizeof(A_long), first);
  scalar("A_Boolean", sizeof(A_Boolean), first);
  scalar("A_FpShort", sizeof(A_FpShort), first);
  scalar("A_u_short", sizeof(A_u_short), first);
  scalar("AEGP_PluginID", sizeof(AEGP_PluginID), first);
  scalar("PR_RenderContextH", sizeof(PR_RenderContextH), first);
  scalar("PF_EffectBlendingTables", sizeof(PF_EffectBlendingTables), first);
  scalar("AEGP_ItemViewP", sizeof(AEGP_ItemViewP), first);
  scalar("AEGP_WorldH", sizeof(AEGP_WorldH), first);
  scalar("AEGP_CompH", sizeof(AEGP_CompH), first);
  scalar("AEGP_ColorProfileP", sizeof(AEGP_ColorProfileP), first);
  scalar("AEGP_ConstColorProfileP", sizeof(AEGP_ConstColorProfileP), first);
  scalar("AEGP_MemHandle", sizeof(AEGP_MemHandle), first);
  scalar("AEGP_GuidP", sizeof(AEGP_GuidP), first);
  std::cout << "},\n  \"suite\":{\"name\":\"AEGP_ColorSettingsSuite6\","
               "\"size\":" << sizeof(Suite) << ",\"alignment\":"
            << alignof(Suite) << ",\"slot_count\":20,\"members\":{";
  first = true;
  EMIT(AEGP_GetBlendingTables,
       A_Err (*)(PR_RenderContextH, PF_EffectBlendingTables*));
  EMIT(AEGP_DoesViewHaveColorSpaceXform,
       A_Err (*)(AEGP_ItemViewP, A_Boolean*));
  EMIT(AEGP_XformWorkingToViewColorSpace,
       A_Err (*)(AEGP_ItemViewP, AEGP_WorldH, AEGP_WorldH));
  EMIT(AEGP_GetNewWorkingSpaceColorProfile,
       A_Err (*)(AEGP_PluginID, AEGP_CompH, AEGP_ColorProfileP*));
  EMIT(AEGP_GetNewColorProfileFromICCProfile,
       A_Err (*)(AEGP_PluginID, A_long, const void*, AEGP_ColorProfileP*));
  EMIT(AEGP_GetNewICCProfileFromColorProfile,
       A_Err (*)(AEGP_PluginID, AEGP_ConstColorProfileP, AEGP_MemHandle*));
  EMIT(AEGP_GetNewColorProfileDescription,
       A_Err (*)(AEGP_PluginID, AEGP_ConstColorProfileP, AEGP_MemHandle*));
  EMIT(AEGP_DisposeColorProfile, A_Err (*)(AEGP_ColorProfileP));
  EMIT(AEGP_GetColorProfileApproximateGamma,
       A_Err (*)(AEGP_ConstColorProfileP, A_FpShort*));
  EMIT(AEGP_IsRGBColorProfile,
       A_Err (*)(AEGP_ConstColorProfileP, A_Boolean*));
  EMIT(AEGP_SetWorkingColorSpace,
       A_Err (*)(AEGP_PluginID, AEGP_CompH, AEGP_ConstColorProfileP));
  EMIT(AEGP_IsOCIOColorManagementUsed,
       A_Err (*)(AEGP_PluginID, A_Boolean*));
  EMIT(AEGP_GetOCIOConfigurationFile,
       A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
  EMIT(AEGP_GetOCIOConfigurationFilePath,
       A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
  EMIT(AEGPD_GetOCIOWorkingColorSpace,
       A_Err (*)(AEGP_PluginID, AEGP_MemHandle*));
  EMIT(AEGPD_GetOCIODisplayColorSpace,
       A_Err (*)(AEGP_PluginID, AEGP_MemHandle*, AEGP_MemHandle*));
  EMIT(AEGPD_IsColorSpaceAwareEffectsEnabled,
       A_Err (*)(AEGP_PluginID, A_Boolean*));
  EMIT(AEGPD_GetLUTInterpolationMethod,
       A_Err (*)(AEGP_PluginID, A_u_short*));
  EMIT(AEGPD_GetGraphicsWhiteLuminance,
       A_Err (*)(AEGP_PluginID, A_u_short*));
  EMIT(AEGPD_GetWorkingColorSpaceId,
       A_Err (*)(AEGP_PluginID, AEGP_GuidP));
  std::cout << "\n    }}\n}\n";
}
