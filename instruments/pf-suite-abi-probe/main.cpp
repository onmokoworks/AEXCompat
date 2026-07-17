#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectSuites.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <iostream>
#include <type_traits>

namespace {

template <typename Suite>
void suite_begin(const char* name, std::size_t slots) {
  std::cout << "    \"" << name << "\":{\"size\":" << sizeof(Suite)
            << ",\"alignment\":" << alignof(Suite)
            << ",\"named_slot_count\":" << slots << ",\"members\":{";
}

void member(const char* name, std::size_t offset, std::size_t size, bool& first) {
  if (!first) {
    std::cout << ',';
  }
  first = false;
  std::cout << "\"" << name << "\":{\"offset\":" << offset
            << ",\"size\":" << size << '}';
}

#define ABI_MEMBER(type, field, first) \
  member(#field, offsetof(type, field), sizeof(decltype(type::field)), first)

#define ASSERT_SLOT(type, field, index) \
  static_assert(offsetof(type, field) == (index) * sizeof(void*))

static_assert(std::is_standard_layout_v<PF_WorldTransformSuite1>);
static_assert(std::is_standard_layout_v<PF_PathDataSuite1>);
static_assert(std::is_standard_layout_v<AEGP_RenderOptionsSuite1>);
static_assert(std::is_standard_layout_v<AEGP_WorldSuite3>);
static_assert(std::is_standard_layout_v<AEGP_RenderSuite4>);
static_assert(std::is_standard_layout_v<AEGP_RenderAsyncManagerSuite1>);

ASSERT_SLOT(PF_WorldTransformSuite1, composite_rect, 0);
ASSERT_SLOT(PF_WorldTransformSuite1, transform_world, 6);
ASSERT_SLOT(PF_PathDataSuite1, PF_PathIsOpen, 0);
ASSERT_SLOT(PF_PathDataSuite1, PF_PathGetName, 10);
ASSERT_SLOT(AEGP_RenderOptionsSuite1, AEGP_NewFromItem, 0);
ASSERT_SLOT(AEGP_RenderOptionsSuite1, AEGP_GetMatteMode, 16);
ASSERT_SLOT(AEGP_WorldSuite3, AEGP_New, 0);
ASSERT_SLOT(AEGP_WorldSuite3, AEGP_NewReferenceFromPlatformWorld, 12);
ASSERT_SLOT(AEGP_RenderSuite4, AEGP_RenderAndCheckoutFrame, 0);
ASSERT_SLOT(AEGP_RenderSuite4, AEGP_GetReceiptGuid, 11);
ASSERT_SLOT(AEGP_RenderAsyncManagerSuite1,
            AEGP_CheckoutOrRender_ItemFrame_AsyncManager, 0);
ASSERT_SLOT(AEGP_RenderAsyncManagerSuite1,
            AEGP_CheckoutOrRender_LayerFrame_AsyncManager, 1);

}  // namespace

int main() {
  std::cout << "{\n  \"schema_version\":1,\n"
               "  \"source_kind\":\"compiled_sdk_header_observation\",\n"
               "  \"architecture\":\"x86_64-windows\",\n"
               "  \"scalars\":{"
            << "\"pointer\":" << sizeof(void*)
            << ",\"A_char\":" << sizeof(A_char)
            << ",\"A_short\":" << sizeof(A_short)
            << ",\"A_long\":" << sizeof(A_long)
            << ",\"A_u_long\":" << sizeof(A_u_long)
            << ",\"A_FpLong\":" << sizeof(A_FpLong)
            << ",\"PF_Err\":" << sizeof(PF_Err)
            << ",\"PF_Boolean\":" << sizeof(PF_Boolean)
            << ",\"PF_Field\":" << sizeof(PF_Field)
            << ",\"PF_XferMode\":" << sizeof(PF_XferMode)
            << ",\"PF_MaskMode\":" << sizeof(PF_MaskMode)
            << ",\"AEGP_PluginID\":" << sizeof(AEGP_PluginID)
            << ",\"AEGP_WorldType\":" << sizeof(AEGP_WorldType)
            << ",\"AEGP_MatteMode\":" << sizeof(AEGP_MatteMode)
            << ",\"AEGP_FrameReceiptH\":" << sizeof(AEGP_FrameReceiptH)
            << ",\"AEGP_WorldH\":" << sizeof(AEGP_WorldH)
            << ",\"A_Time\":" << sizeof(A_Time)
            << ",\"A_LRect\":" << sizeof(A_LRect)
            << ",\"PF_PathVertex\":" << sizeof(PF_PathVertex)
            << "},\n  \"suite_versions\":{"
            << "\"kAEGPRenderSuiteVersion4\":"
            << kAEGPRenderSuiteVersion4
            << ",\"kAEGPRenderAsyncManagerSuiteVersion1\":"
            << kAEGPRenderAsyncManagerSuiteVersion1
            << "},\n  \"suites\":{\n";

  bool first = true;
  suite_begin<PF_WorldTransformSuite1>("PF_WorldTransformSuite1", 7);
  ABI_MEMBER(PF_WorldTransformSuite1, composite_rect, first);
  ABI_MEMBER(PF_WorldTransformSuite1, blend, first);
  ABI_MEMBER(PF_WorldTransformSuite1, convolve, first);
  ABI_MEMBER(PF_WorldTransformSuite1, copy, first);
  ABI_MEMBER(PF_WorldTransformSuite1, copy_hq, first);
  ABI_MEMBER(PF_WorldTransformSuite1, transfer_rect, first);
  ABI_MEMBER(PF_WorldTransformSuite1, transform_world, first);
  std::cout << "}},\n";

  first = true;
  suite_begin<PF_PathDataSuite1>("PF_PathDataSuite1", 11);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathIsOpen, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathNumSegments, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathVertexInfo, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathPrepareSegLength, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathGetSegLength, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathEvalSegLength, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathEvalSegLengthDeriv1, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathCleanupSegLength, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathIsInverted, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathGetMaskMode, first);
  ABI_MEMBER(PF_PathDataSuite1, PF_PathGetName, first);
  std::cout << "}},\n";

  first = true;
  suite_begin<AEGP_RenderOptionsSuite1>("AEGP_RenderOptionsSuite1", 17);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_NewFromItem, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_Duplicate, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_Dispose, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetTime, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetTime, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetTimeStep, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetTimeStep, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetFieldRender, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetFieldRender, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetWorldType, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetWorldType, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetDownsampleFactor, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetDownsampleFactor, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetRegionOfInterest, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetRegionOfInterest, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_SetMatteMode, first);
  ABI_MEMBER(AEGP_RenderOptionsSuite1, AEGP_GetMatteMode, first);
  std::cout << "}},\n";

  first = true;
  suite_begin<AEGP_WorldSuite3>("AEGP_WorldSuite3", 13);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_New, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_Dispose, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetType, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetSize, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetRowBytes, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetBaseAddr8, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetBaseAddr16, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_GetBaseAddr32, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_FillOutPFEffectWorld, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_FastBlur, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_NewPlatformWorld, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_DisposePlatformWorld, first);
  ABI_MEMBER(AEGP_WorldSuite3, AEGP_NewReferenceFromPlatformWorld, first);
  std::cout << "}},\n";

  first = true;
  suite_begin<AEGP_RenderSuite4>("AEGP_RenderSuite4", 12);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_RenderAndCheckoutFrame, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_RenderAndCheckoutLayerFrame, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_CheckinFrame, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_GetReceiptWorld, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_GetRenderedRegion, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_IsRenderedFrameSufficient, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_RenderNewItemSoundData, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_GetCurrentTimestamp, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_HasItemChangedSinceTimestamp, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_IsItemWorthwhileToRender, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_CheckinRenderedFrame, first);
  ABI_MEMBER(AEGP_RenderSuite4, AEGP_GetReceiptGuid, first);
  std::cout << "}},\n";

  first = true;
  suite_begin<AEGP_RenderAsyncManagerSuite1>(
      "AEGP_RenderAsyncManagerSuite1", 2);
  ABI_MEMBER(AEGP_RenderAsyncManagerSuite1,
             AEGP_CheckoutOrRender_ItemFrame_AsyncManager, first);
  ABI_MEMBER(AEGP_RenderAsyncManagerSuite1,
             AEGP_CheckoutOrRender_LayerFrame_AsyncManager, first);
  std::cout << "}}\n  }\n}\n";
}
