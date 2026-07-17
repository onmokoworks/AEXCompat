#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"

#include <cstdio>
#include <cstring>

namespace {
constexpr uintptr_t kRefcon = 0xA53C;

PF_Err make_value(PF_InData* in_data, A_long value, PF_ArbitraryH* result) {
  if (!result) return PF_Err_BAD_CALLBACK_PARAM;
  PF_Handle handle = PF_NEW_HANDLE(sizeof(A_long));
  if (!handle) return PF_Err_OUT_OF_MEMORY;
  auto* data = static_cast<A_long*>(PF_LOCK_HANDLE(handle));
  if (!data) { PF_DISPOSE_HANDLE(handle); return PF_Err_OUT_OF_MEMORY; }
  *data = value;
  PF_UNLOCK_HANDLE(handle);
  *result = handle;
  return PF_Err_NONE;
}

PF_Err read_value(PF_InData* in_data, PF_ArbitraryH handle, A_long* value) {
  if (!handle || !value) return PF_Err_BAD_CALLBACK_PARAM;
  auto* data = static_cast<A_long*>(PF_LOCK_HANDLE(handle));
  if (!data) return PF_Err_BAD_CALLBACK_PARAM;
  *value = *data;
  PF_UNLOCK_HANDLE(handle);
  return PF_Err_NONE;
}
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void* extra_void) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) {
    out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
    out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
  } else if (cmd == PF_Cmd_PARAMS_SETUP) {
    PF_ParamDef def{};
    def.param_type = PF_Param_ARBITRARY_DATA;
    def.uu.id = 1;
    std::strcpy(def.name, "Scannable Value");
    def.u.arb_d.id = 1;
    def.u.arb_d.refconPV = reinterpret_cast<void*>(kRefcon);
    PF_Err err = make_value(in_data, 7, &def.u.arb_d.dephault);
    if (!err) err = PF_ADD_PARAM(in_data, -1, &def);
    out_data->num_params = 2;
    return err;
  } else if (cmd == PF_Cmd_ARBITRARY_CALLBACK) {
    auto* extra = static_cast<PF_ArbParamsExtra*>(extra_void);
    if (!extra) return PF_Err_BAD_CALLBACK_PARAM;
    void* refcon = nullptr;
    switch (extra->which_function) {
      case PF_Arbitrary_NEW_FUNC: refcon = extra->u.new_func_params.refconPV; break;
      case PF_Arbitrary_DISPOSE_FUNC: refcon = extra->u.dispose_func_params.refconPV; break;
      case PF_Arbitrary_COPY_FUNC: refcon = extra->u.copy_func_params.refconPV; break;
      case PF_Arbitrary_SCAN_FUNC: refcon = extra->u.scan_func_params.refconPV; break;
      default: refcon = extra->u.print_func_params.refconPV; break;
    }
    if (refcon != reinterpret_cast<void*>(kRefcon)) return PF_Err_BAD_CALLBACK_PARAM;
    A_long value = 0;
    switch (extra->which_function) {
      case PF_Arbitrary_NEW_FUNC:
        return make_value(in_data, 7, extra->u.new_func_params.arbPH);
      case PF_Arbitrary_DISPOSE_FUNC:
        PF_DISPOSE_HANDLE(extra->u.dispose_func_params.arbH); return PF_Err_NONE;
      case PF_Arbitrary_COPY_FUNC:
        if (read_value(in_data, extra->u.copy_func_params.src_arbH, &value)) return PF_Err_BAD_CALLBACK_PARAM;
        return make_value(in_data, value, extra->u.copy_func_params.dst_arbPH);
      case PF_Arbitrary_FLAT_SIZE_FUNC:
        *extra->u.flat_size_func_params.flat_data_sizePLu = sizeof(A_long); return PF_Err_NONE;
      case PF_Arbitrary_FLATTEN_FUNC:
        if (extra->u.flatten_func_params.buf_sizeLu != sizeof(A_long) ||
            read_value(in_data, extra->u.flatten_func_params.arbH, &value)) return PF_Err_BAD_CALLBACK_PARAM;
        std::memcpy(extra->u.flatten_func_params.flat_dataPV, &value, sizeof(value)); return PF_Err_NONE;
      case PF_Arbitrary_UNFLATTEN_FUNC:
        if (extra->u.unflatten_func_params.buf_sizeLu != sizeof(A_long)) return PF_Err_BAD_CALLBACK_PARAM;
        std::memcpy(&value, extra->u.unflatten_func_params.flat_dataPV, sizeof(value));
        return make_value(in_data, value, extra->u.unflatten_func_params.arbPH);
      case PF_Arbitrary_INTERP_FUNC:
        if (read_value(in_data, extra->u.interp_func_params.left_arbH, &value)) return PF_Err_BAD_CALLBACK_PARAM;
        return make_value(in_data, value, extra->u.interp_func_params.interpPH);
      case PF_Arbitrary_COMPARE_FUNC: {
        A_long other = 0;
        if (read_value(in_data, extra->u.compare_func_params.a_arbH, &value) ||
            read_value(in_data, extra->u.compare_func_params.b_arbH, &other)) return PF_Err_BAD_CALLBACK_PARAM;
        *extra->u.compare_func_params.compareP = value == other ? PF_ArbCompare_EQUAL : PF_ArbCompare_NOT_EQUAL;
        return PF_Err_NONE;
      }
      case PF_Arbitrary_PRINT_SIZE_FUNC:
        *extra->u.print_size_func_params.print_sizePLu = 32; return PF_Err_NONE;
      case PF_Arbitrary_PRINT_FUNC:
        if (read_value(in_data, extra->u.print_func_params.arbH, &value)) return PF_Err_BAD_CALLBACK_PARAM;
        std::snprintf(extra->u.print_func_params.print_bufferPC,
                      extra->u.print_func_params.print_sizeLu, "value=%d", value); return PF_Err_NONE;
      case PF_Arbitrary_SCAN_FUNC: {
        if (!extra->u.scan_func_params.bufPC || extra->u.scan_func_params.bytes_to_scanLu > 31)
          return PF_Err_CANNOT_PARSE_KEYFRAME_TEXT;
        char buffer[32]{};
        std::memcpy(buffer, extra->u.scan_func_params.bufPC, extra->u.scan_func_params.bytes_to_scanLu);
        int parsed = 0; char trailing = 0;
        if (std::sscanf(buffer, "value=%d%c", &parsed, &trailing) != 1 || parsed < -1000 || parsed > 1000)
          return PF_Err_CANNOT_PARSE_KEYFRAME_TEXT;
        return make_value(in_data, parsed, extra->u.scan_func_params.arbPH);
      }
    }
  } else if (cmd == PF_Cmd_RENDER) {
    A_long value = 0;
    if (!params || !params[1] || read_value(in_data, params[1]->u.arb_d.value, &value) || value != 7)
      return PF_Err_BAD_CALLBACK_PARAM;
    if (!output || !output->data || !params[0] || !params[0]->u.ld.data) return PF_Err_BAD_CALLBACK_PARAM;
    for (A_long y = 0; y < output->height; ++y)
      std::memcpy(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                  reinterpret_cast<A_u_char*>(params[0]->u.ld.data) + y * params[0]->u.ld.rowbytes,
                  static_cast<size_t>(output->width) * 4);
  }
  return PF_Err_NONE;
}
