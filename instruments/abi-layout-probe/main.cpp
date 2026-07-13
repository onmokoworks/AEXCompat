#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"

#include <cstddef>
#include <iostream>

namespace {
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
               "  \"sdk_boundary\":\"instrument_observation\",\n"
               "  \"pointer_size\":" << sizeof(void*) << ",\n"
               "  \"pf_in_data_size\":" << sizeof(PF_InData) << ",\n"
               "  \"pf_out_data_size\":" << sizeof(PF_OutData) << ",\n"
               "  \"pf_param_def_size\":" << sizeof(PF_ParamDef) << ",\n"
               "  \"pf_layer_def_size\":" << sizeof(PF_LayerDef) << ",\n"
               "  \"pf_interact_callbacks_size\":" << sizeof(PF_InteractCallbacks) << ",\n"
               "  \"pf_param_union_size\":" << sizeof(PF_ParamDefUnion) << ",\n"
               "  \"pf_util_callbacks_size\":" << sizeof(PF_UtilCallbacks) << ",\n"
               "  \"fields\":{";
  field<decltype(PF_InData::version)>("in.version", offsetof(PF_InData, version), first);
  field<decltype(PF_InData::serial_num)>("in.serial_num", offsetof(PF_InData, serial_num), first);
  field<decltype(PF_InData::appl_id)>("in.appl_id", offsetof(PF_InData, appl_id), first);
  field<decltype(PF_InData::num_params)>("in.num_params", offsetof(PF_InData, num_params), first);
  field<decltype(PF_InData::pica_basicP)>("in.pica_basicP", offsetof(PF_InData, pica_basicP), first);
  field<decltype(PF_InData::inter)>("in.inter", offsetof(PF_InData, inter), first);
  field<decltype(PF_InData::utils)>("in.utils", offsetof(PF_InData, utils), first);
  field<decltype(PF_InData::effect_ref)>("in.effect_ref", offsetof(PF_InData, effect_ref), first);
  field<decltype(PF_InData::global_data)>("in.global_data", offsetof(PF_InData, global_data), first);
  field<decltype(PF_InteractCallbacks::checkout_param)>("inter.checkout_param", offsetof(PF_InteractCallbacks, checkout_param), first);
  field<decltype(PF_InteractCallbacks::checkin_param)>("inter.checkin_param", offsetof(PF_InteractCallbacks, checkin_param), first);
  field<decltype(PF_InteractCallbacks::add_param)>("inter.add_param", offsetof(PF_InteractCallbacks, add_param), first);
  field<decltype(PF_InteractCallbacks::abort)>("inter.abort", offsetof(PF_InteractCallbacks, abort), first);
  field<decltype(PF_InteractCallbacks::progress)>("inter.progress", offsetof(PF_InteractCallbacks, progress), first);
  field<decltype(PF_InteractCallbacks::register_ui)>("inter.register_ui", offsetof(PF_InteractCallbacks, register_ui), first);
  field<decltype(PF_UtilCallbacks::host_new_handle)>("utils.host_new_handle", offsetof(PF_UtilCallbacks, host_new_handle), first);
  field<decltype(PF_UtilCallbacks::host_lock_handle)>("utils.host_lock_handle", offsetof(PF_UtilCallbacks, host_lock_handle), first);
  field<decltype(PF_UtilCallbacks::host_unlock_handle)>("utils.host_unlock_handle", offsetof(PF_UtilCallbacks, host_unlock_handle), first);
  field<decltype(PF_UtilCallbacks::host_dispose_handle)>("utils.host_dispose_handle", offsetof(PF_UtilCallbacks, host_dispose_handle), first);
  field<decltype(PF_OutData::my_version)>("out.my_version", offsetof(PF_OutData, my_version), first);
  field<decltype(PF_OutData::global_data)>("out.global_data", offsetof(PF_OutData, global_data), first);
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
  std::cout << "\n  },\n  \"selectors\":{"
            << "\"about\":" << static_cast<int>(PF_Cmd_ABOUT) << ','
            << "\"global_setup\":" << static_cast<int>(PF_Cmd_GLOBAL_SETUP) << ','
            << "\"global_setdown\":" << static_cast<int>(PF_Cmd_GLOBAL_SETDOWN) << ','
            << "\"params_setup\":" << static_cast<int>(PF_Cmd_PARAMS_SETUP)
            << "}\n}\n";
}
