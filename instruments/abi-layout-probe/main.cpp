#include "AEConfig.h"
#include "AE_Effect.h"

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
               "  \"fields\":{";
  field<decltype(PF_InData::version)>("in.version", offsetof(PF_InData, version), first);
  field<decltype(PF_InData::serial_num)>("in.serial_num", offsetof(PF_InData, serial_num), first);
  field<decltype(PF_InData::appl_id)>("in.appl_id", offsetof(PF_InData, appl_id), first);
  field<decltype(PF_InData::num_params)>("in.num_params", offsetof(PF_InData, num_params), first);
  field<decltype(PF_InData::pica_basicP)>("in.pica_basicP", offsetof(PF_InData, pica_basicP), first);
  field<decltype(PF_OutData::my_version)>("out.my_version", offsetof(PF_OutData, my_version), first);
  field<decltype(PF_OutData::out_flags)>("out.out_flags", offsetof(PF_OutData, out_flags), first);
  field<decltype(PF_OutData::num_params)>("out.num_params", offsetof(PF_OutData, num_params), first);
  field<decltype(PF_OutData::return_msg)>("out.return_msg", offsetof(PF_OutData, return_msg), first);
  field<decltype(PF_OutData::out_flags2)>("out.out_flags2", offsetof(PF_OutData, out_flags2), first);
  std::cout << "\n  },\n  \"selectors\":{"
            << "\"about\":" << static_cast<int>(PF_Cmd_ABOUT) << ','
            << "\"global_setup\":" << static_cast<int>(PF_Cmd_GLOBAL_SETUP) << ','
            << "\"global_setdown\":" << static_cast<int>(PF_Cmd_GLOBAL_SETDOWN) << ','
            << "\"params_setup\":" << static_cast<int>(PF_Cmd_PARAMS_SETUP)
            << "}\n}\n";
}
