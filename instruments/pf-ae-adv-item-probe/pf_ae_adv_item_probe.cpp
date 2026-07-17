#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_AdvEffectSuites.h"

#include <cstdio>
#include <new>
#include <sstream>
#include <windows.h>

static_assert(sizeof(PF_AdvItemSuite1) == 5 * sizeof(void*));
static_assert(offsetof(PF_AdvItemSuite1, PF_MoveTimeStep) == 0 * sizeof(void*));
static_assert(offsetof(PF_AdvItemSuite1, PF_MoveTimeStepActiveItem) == 1 * sizeof(void*));
static_assert(offsetof(PF_AdvItemSuite1, PF_TouchActiveItem) == 2 * sizeof(void*));
static_assert(offsetof(PF_AdvItemSuite1, PF_ForceRerender) == 3 * sizeof(void*));
static_assert(offsetof(PF_AdvItemSuite1, PF_EffectIsActiveOrEnabled) == 4 * sizeof(void*));

namespace {
template<class F> PF_Err guarded(F&& call, LONG& exception) {
  PF_Err err = PF_Err_INTERNAL_STRUCT_DAMAGED; exception = 0;
  __try { err = call(); }
  __except(EXCEPTION_EXECUTE_HANDLER) { exception = GetExceptionCode(); }
  return err;
}

void emit(PF_InData* in, PF_EffectWorld* world) {
  const void* raw = nullptr;
  const SPErr acquire = in->pica_basicP->AcquireSuite(
      kPFAdvItemSuite, kPFAdvItemSuiteVersion1, &raw);
  const auto* suite = static_cast<const PF_AdvItemSuite1*>(raw);
  std::ostringstream out;
  out << "AEXCOMPAT_PF_ADV_ITEM_V1 {\"acquire_err\":" << acquire;
  int acquired = acquire == 0 ? 1 : 0, released = 0;
  if (suite) {
    LONG ex[8]{}; PF_Boolean enabled = TRUE;
    const PF_Err move = guarded([&]{ return suite->PF_MoveTimeStep(in, world, PF_Step_FORWARD, 1); }, ex[0]);
    const PF_Err active = guarded([&]{ return suite->PF_MoveTimeStepActiveItem(PF_Step_BACKWARD, 1); }, ex[1]);
    const PF_Err touch = guarded([&]{ return suite->PF_TouchActiveItem(); }, ex[2]);
    const PF_Err rerender = guarded([&]{ return suite->PF_ForceRerender(in, world); }, ex[3]);
    const PF_Err enabled_err = guarded([&]{ return suite->PF_EffectIsActiveOrEnabled(nullptr, &enabled); }, ex[4]);
    const PF_Err invalid_direction = guarded([&]{ return suite->PF_MoveTimeStepActiveItem(static_cast<PF_Step>(99), 1); }, ex[5]);
    const PF_Err negative_steps = guarded([&]{ return suite->PF_MoveTimeStepActiveItem(PF_Step_FORWARD, -1); }, ex[6]);
    const PF_Err null_inputs = guarded([&]{ return suite->PF_MoveTimeStep(nullptr, nullptr, PF_Step_FORWARD, 1); }, ex[7]);
    out << ",\"move\":" << move << ",\"active\":" << active
        << ",\"touch\":" << touch << ",\"rerender\":" << rerender
        << ",\"enabled_err\":" << enabled_err << ",\"enabled\":" << static_cast<int>(enabled)
        << ",\"invalid_direction\":" << invalid_direction
        << ",\"negative_steps\":" << negative_steps << ",\"null_inputs\":" << null_inputs;
  }
  SPErr release = PF_Err_NONE;
  if (acquired) { release = in->pica_basicP->ReleaseSuite(kPFAdvItemSuite, kPFAdvItemSuiteVersion1); released = 1; }
  out << ",\"release_err\":" << release << ",\"lease_balanced\":"
      << (acquired == released ? "true" : "false") << '}';
  std::puts(out.str().c_str()); OutputDebugStringA((out.str() + "\n").c_str());
}
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in, PF_OutData* out,
                                         PF_ParamDef*[], PF_LayerDef* output, void*) {
  try {
    if (!in || !out) return PF_Err_BAD_CALLBACK_PARAM;
    if (cmd == PF_Cmd_GLOBAL_SETUP) {
      out->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out->out_flags = PF_OutFlag_PIX_INDEPENDENT; return PF_Err_NONE;
    }
    if (cmd == PF_Cmd_PARAMS_SETUP) { out->num_params = 1; return PF_Err_NONE; }
    if (cmd == PF_Cmd_RENDER) { emit(in, output); return PF_Err_NONE; }
    return PF_Err_NONE;
  } catch (const std::bad_alloc&) { return PF_Err_OUT_OF_MEMORY; }
  catch (...) { return PF_Err_INTERNAL_STRUCT_DAMAGED; }
}
