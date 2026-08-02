#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <cstdio>
#include <cstring>

namespace {
constexpr char kResultPath[] =
    "target/ae-oracles/pf-batch-sampling.result.json";

struct Snapshot {
  PF_SampPB value{};
};

int ChangedBytes(const Snapshot& left, const Snapshot& right) {
  const auto* a = reinterpret_cast<const unsigned char*>(&left.value);
  const auto* b = reinterpret_cast<const unsigned char*>(&right.value);
  int changed = 0;
  for (size_t i = 0; i < sizeof(PF_SampPB); ++i) changed += a[i] != b[i];
  return changed;
}

void WriteSnapshot(FILE* file, const char* name, const Snapshot& snapshot,
                   int changed_bytes) {
  const PF_SampPB& p = snapshot.value;
  std::fprintf(file,
      "\"%s\":{\"changed_bytes\":%d,\"src_non_null\":%s,"
      "\"x_radius\":%ld,\"y_radius\":%ld,\"area\":%ld,"
      "\"samp_behave\":%d,\"allow_asynch\":%ld,\"motion_blur\":%ld,"
      "\"mask0_non_null\":%s,\"fcm_table_non_null\":%s,"
      "\"fcd_table_non_null\":%s,\"reserved\":[%ld,%ld,%ld,%ld,%ld,%ld,%ld,%ld]}",
      name, changed_bytes, p.src ? "true" : "false",
      static_cast<long>(p.x_radius), static_cast<long>(p.y_radius),
      static_cast<long>(p.area), static_cast<int>(p.samp_behave),
      static_cast<long>(p.allow_asynch), static_cast<long>(p.motion_blur),
      p.mask0 ? "true" : "false", p.fcm_table ? "true" : "false",
      p.fcd_table ? "true" : "false", static_cast<long>(p.reserved[0]),
      static_cast<long>(p.reserved[1]), static_cast<long>(p.reserved[2]),
      static_cast<long>(p.reserved[3]), static_cast<long>(p.reserved[4]),
      static_cast<long>(p.reserved[5]), static_cast<long>(p.reserved[6]),
      static_cast<long>(p.reserved[7]));
}

void CopyWorld(const PF_EffectWorld& input, PF_LayerDef* output) {
  const A_long rows = input.height < output->height ? input.height : output->height;
  const A_long bytes = input.rowbytes < output->rowbytes ? input.rowbytes : output->rowbytes;
  for (A_long y = 0; y < rows; ++y) {
    std::memcpy(reinterpret_cast<unsigned char*>(output->data) + y * output->rowbytes,
                reinterpret_cast<const unsigned char*>(input.data) + y * input.rowbytes,
                static_cast<size_t>(bytes));
  }
}

PF_Err Render(PF_InData* in, PF_ParamDef* params[], PF_LayerDef* output) {
  if (!in || !in->pica_basicP || !params || !params[0] || !output ||
      !params[0]->u.ld.data || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  CopyWorld(params[0]->u.ld, output);

  const PF_BatchSamplingSuite1* suite = nullptr;
  const PF_Err acquire_err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
      kPFBatchSamplingSuite, kPFBatchSamplingSuiteVersion1,
      reinterpret_cast<const void**>(&suite)));

  PF_Err begin_err = PF_Err_BAD_CALLBACK_PARAM;
  PF_Err end_err = PF_Err_BAD_CALLBACK_PARAM;
  PF_Err get8_err = PF_Err_BAD_CALLBACK_PARAM;
  PF_Err get16_err = PF_Err_BAD_CALLBACK_PARAM;
  PF_BatchSampleFunc batch8 = nullptr;
  PF_BatchSample16Func batch16 = nullptr;
  Snapshot before{};
  before.value.src = &params[0]->u.ld;
  before.value.samp_behave = PF_SampleEdgeBehav_ZERO;
  Snapshot after_begin = before;
  Snapshot after_end = before;
  const PF_ModeFlags mode_flags = PF_MF_Alpha_STRAIGHT;

  const bool slots[4] = {suite && suite->begin_sampling, suite && suite->end_sampling,
                         suite && suite->get_batch_func, suite && suite->get_batch_func16};
  if (!acquire_err && suite) {
    if (suite->begin_sampling)
      begin_err = suite->begin_sampling(in->effect_ref, in->quality, mode_flags, &after_begin.value);
    after_end = after_begin;
    if (begin_err == PF_Err_NONE) {
      if (suite->get_batch_func)
        get8_err = suite->get_batch_func(in->effect_ref, in->quality, mode_flags,
                                         &after_begin.value, &batch8);
      if (suite->get_batch_func16)
        get16_err = suite->get_batch_func16(in->effect_ref, in->quality, mode_flags,
                                             &after_begin.value, &batch16);
      if (suite->end_sampling)
        end_err = suite->end_sampling(in->effect_ref, in->quality, mode_flags, &after_end.value);
    }
  }

  const PF_Err release_err = (!acquire_err && suite)
      ? static_cast<PF_Err>(in->pica_basicP->ReleaseSuite(
            kPFBatchSamplingSuite, kPFBatchSamplingSuiteVersion1))
      : PF_Err_BAD_CALLBACK_PARAM;
  if (FILE* file = std::fopen(kResultPath, "wb")) {
    std::fprintf(file,
        "{\"schema_version\":1,\"suite_name\":\"%s\",\"suite_version\":%d,"
        "\"available\":%s,\"acquire_err\":%d,\"release_attempted\":%s,"
        "\"release_err\":%d,\"slots\":{\"begin_sampling\":%s,\"end_sampling\":%s,"
        "\"get_batch_func\":%s,\"get_batch_func16\":%s},"
        "\"begin_err\":%d,\"end_err\":%d,\"get_batch_func_err\":%d,"
        "\"get_batch_func_pointer_non_null\":%s,\"get_batch_func16_err\":%d,"
        "\"get_batch_func16_pointer_non_null\":%s,\"batch_pointer_invoked\":false,",
        kPFBatchSamplingSuite, kPFBatchSamplingSuiteVersion1,
        (!acquire_err && suite) ? "true" : "false", acquire_err,
        (!acquire_err && suite) ? "true" : "false", release_err,
        slots[0] ? "true" : "false", slots[1] ? "true" : "false",
        slots[2] ? "true" : "false", slots[3] ? "true" : "false",
        begin_err, end_err, get8_err, batch8 ? "true" : "false", get16_err,
        batch16 ? "true" : "false");
    WriteSnapshot(file, "samp_pb_before", before, 0); std::fputc(',', file);
    WriteSnapshot(file, "samp_pb_after_begin", after_begin, ChangedBytes(before, after_begin));
    std::fputc(',', file);
    WriteSnapshot(file, "samp_pb_after_end", after_end, ChangedBytes(after_begin, after_end));
    std::fputs("}\n", file);
    std::fclose(file);
  }
  return PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in,
    PF_OutData* out, PF_ParamDef* params[], PF_LayerDef* output, void*) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) {
    out->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
    out->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
  } else if (cmd == PF_Cmd_PARAMS_SETUP) {
    out->num_params = 1;
  } else if (cmd == PF_Cmd_RENDER) {
    return Render(in, params, output);
  }
  return PF_Err_NONE;
}
