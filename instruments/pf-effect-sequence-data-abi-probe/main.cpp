#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cstddef>
#include <iostream>
#include <type_traits>

using Suite = PF_EffectSequenceDataSuite1;
using Getter = PF_Err (*)(PF_ProgPtr, PF_ConstHandle*);

static_assert(std::is_standard_layout_v<Suite>);
static_assert(sizeof(Suite) == sizeof(void*));
static_assert(offsetof(Suite, PF_GetConstSequenceData) == 0);
static_assert(std::is_same_v<decltype(Suite::PF_GetConstSequenceData), Getter>);
static_assert(std::is_same_v<PF_ConstHandle, const void* const*>);
static_assert(kPFEffectSequenceDataSuiteVersion1 == 1);

int main() {
  std::cout << "{\"schema_version\":1,\"source_kind\":\"compiled_sdk_header_observation\""
               ",\"architecture\":\"x86_64-windows\",\"acquisition\":{\"name\":\""
            << kPFEffectSequenceDataSuite << "\",\"version\":"
            << kPFEffectSequenceDataSuiteVersion1
            << "},\"suite\":{\"name\":\"PF_EffectSequenceDataSuite1\",\"size\":"
            << sizeof(Suite) << ",\"alignment\":" << alignof(Suite)
            << ",\"slot_count\":1,\"members\":{\"PF_GetConstSequenceData\":{\"offset\":"
            << offsetof(Suite, PF_GetConstSequenceData) << ",\"size\":"
            << sizeof(Suite::PF_GetConstSequenceData)
            << ",\"type_matches\":true}}},\"types\":{\"pointer\":" << sizeof(void*)
            << ",\"PF_Err\":" << sizeof(PF_Err)
            << ",\"PF_ProgPtr\":" << sizeof(PF_ProgPtr)
            << ",\"PF_ConstHandle\":" << sizeof(PF_ConstHandle)
            << "}}\n";
}
