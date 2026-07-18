#pragma once

#include <cstdint>
#include <string>

namespace aexcompat::l2mode {

// ABI-facing data remains private to l2_main. This is the narrow callback seam
// used by the mode executor for selector ordering, cleanup, and reporting.
enum class EarlyMode { None, AutomaticDialog, DoDialog, ExternalDependencies, ParametersOnly };
struct HandleStatistics { uint64_t created{}; uint64_t disposed{}; };
struct Hooks {
  uint32_t (*out_flags)(void*){};
  void (*copy_sequence_data_to_input)(void*){};
  int32_t (*sequence_setup)(void*, uint32_t*){};
  int32_t (*sequence_setdown)(void*, uint32_t*){};
  int32_t (*do_dialog)(void*, uint32_t*){};
  int32_t (*global_setdown)(void*){};
  std::string (*return_message)(void*){};
  bool (*handle_lifetimes_balanced)(void*){};
  void (*restore_stdout)(void*){};
  void (*unload_module)(void*){};
  void* (*external_dependencies)(void*, int32_t, int32_t*, uint32_t*){};
  bool (*handle_is_live)(void*, void*){};
  uint64_t (*handle_size)(void*, void*){};
  void* (*lock_handle)(void*, void*){};
  void (*unlock_handle)(void*, void*){};
  void (*dispose_handle)(void*, void*){};
  HandleStatistics (*handle_statistics)(void*){};
  bool (*dispose_arbitrary_defaults)(void*){};
  bool (*module_audit_required)(void*){};
  bool (*capture_pre_unload_audit_passed)(void*){};
  std::string (*module_audit_json)(void*){};
  void (*report_parameters)(void*, const char*, int32_t, int32_t, int32_t){};
};
struct Request {
  EarlyMode mode{EarlyMode::None}; void* context{}; Hooks hooks{};
  int32_t global_error{}; int32_t params_error{}; bool parameter_count_contract_valid{};
  const wchar_t* external_dependency_check_type{};
};
// -1 means no early mode. Any other result has reported and unloaded once.
int run_early_mode(const Request& request);

}  // namespace aexcompat::l2mode
