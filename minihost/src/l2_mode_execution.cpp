#include "l2_mode_execution.hpp"

#include <cstring>
#include <iostream>

namespace aexcompat::l2mode {
namespace {
constexpr uint32_t kOutFlagIDoDialog = 1u << 5;
constexpr uint32_t kOutFlagSendDoDialog = 1u << 7;
constexpr uint32_t kOutFlagDisplayErrorMessage = 1u << 8;
constexpr uint64_t kMaxDependencyBytes = 64 * 1024;

std::string escape_json(const std::string& input) {
  std::string output;
  for (unsigned char ch : input) {
    if (ch == '"' || ch == '\\') output.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) output.push_back(static_cast<char>(ch));
  }
  return output;
}
void stage(const char* name, const char* phase, int32_t error = 0, bool with_error = false) {
  std::cerr << "stage:" << name << '_' << phase;
  if (with_error) std::cerr << " error=" << error;
  std::cerr << '\n' << std::flush;
}
int global_setdown(const Request& r) {
  return r.global_error == 0 ? r.hooks.global_setdown(r.context) : -1;
}

int automatic_dialog(const Request& r) {
  const bool capable = (r.hooks.out_flags(r.context) & kOutFlagIDoDialog) != 0;
  uint32_t setup_exception = 0;
  stage("sequence_setup", "begin");
  const int32_t setup_error = r.params_error == 0
      ? r.hooks.sequence_setup(r.context, &setup_exception) : -1;
  stage("sequence_setup", "end", setup_error, true);
  r.hooks.copy_sequence_data_to_input(r.context);
  const bool requested = (r.hooks.out_flags(r.context) & kOutFlagSendDoDialog) != 0;
  const bool dispatched = capable && requested && setup_error == 0;
  uint32_t dialog_exception = 0;
  int32_t dialog_error = -1;
  if (dispatched) {
    stage("do_dialog", "begin");
    dialog_error = r.hooks.do_dialog(r.context, &dialog_exception);
    stage("do_dialog", "end", dialog_error, true);
  }
  const std::string message = r.hooks.return_message(r.context);
  uint32_t sequence_setdown_exception = 0;
  stage("sequence_setdown", "begin");
  const int32_t sequence_setdown_error = setup_error == 0
      ? r.hooks.sequence_setdown(r.context, &sequence_setdown_exception) : -1;
  stage("sequence_setdown", "end", sequence_setdown_error, true);
  stage("global_setdown", "begin");
  const int32_t setdown_error = global_setdown(r);
  stage("global_setdown", "end", setdown_error, true);
  const bool valid = dispatched && setup_exception == 0 && dialog_error == 0 &&
      dialog_exception == 0 && sequence_setdown_error == 0 &&
      sequence_setdown_exception == 0 && setdown_error == 0 &&
      r.hooks.handle_lifetimes_balanced(r.context);
  r.hooks.restore_stdout(r.context);
  std::cout << "{\"schema_version\":1,\"stage\":\"automatic_dialog\",\"status\":\""
            << (valid ? "automatic_dialog_completed" :
                (requested ? "automatic_dialog_error" : "automatic_dialog_not_requested"))
            << "\",\"dialog_capability_advertised\":" << (capable ? "true" : "false")
            << ",\"automatic_dialog_requested\":" << (requested ? "true" : "false")
            << ",\"selector_dispatched\":" << (dispatched ? "true" : "false")
            << ",\"sequence_setup_error\":" << setup_error
            << ",\"sequence_setup_exception_code\":" << setup_exception
            << ",\"dialog_error\":" << dialog_error
            << ",\"dialog_exception_code\":" << dialog_exception
            << ",\"return_message\":\"" << escape_json(message) << "\""
            << ",\"sequence_setdown_error\":" << sequence_setdown_error
            << ",\"sequence_setdown_exception_code\":" << sequence_setdown_exception
            << ",\"handle_lifetimes_balanced\":"
            << (r.hooks.handle_lifetimes_balanced(r.context) ? "true" : "false")
            << ",\"global_setdown_error\":" << setdown_error << "}\n";
  r.hooks.unload_module(r.context);
  return valid ? 0 : (requested ? 20 : 21);
}

int do_dialog(const Request& r) {
  const bool advertised = (r.hooks.out_flags(r.context) & kOutFlagIDoDialog) != 0;
  uint32_t exception = 0;
  int32_t error = -1;
  if (r.params_error == 0 && advertised) {
    stage("do_dialog", "begin"); error = r.hooks.do_dialog(r.context, &exception);
    stage("do_dialog", "end", error, true);
  }
  const uint32_t returned_flags = r.hooks.out_flags(r.context);
  const std::string message = r.hooks.return_message(r.context);
  stage("global_setdown", "begin");
  const int32_t setdown_error = global_setdown(r);
  stage("global_setdown", "end", setdown_error, true);
  const bool valid = r.params_error == 0 && advertised && error == 0 && exception == 0 &&
      setdown_error == 0 && r.hooks.handle_lifetimes_balanced(r.context);
  r.hooks.restore_stdout(r.context);
  std::cout << "{\"schema_version\":1,\"stage\":\"do_dialog\",\"status\":\""
            << (valid ? "dialog_completed" : (advertised ? "dialog_error" : "dialog_not_advertised"))
            << "\",\"dialog_advertised\":" << (advertised ? "true" : "false")
            << ",\"selector_dispatched\":" << (advertised ? "true" : "false")
            << ",\"selector_error\":" << error << ",\"exception_code\":" << exception
            << ",\"display_error_message\":"
            << ((returned_flags & kOutFlagDisplayErrorMessage) ? "true" : "false")
            << ",\"return_message\":\"" << escape_json(message) << "\""
            << ",\"handle_lifetimes_balanced\":"
            << (r.hooks.handle_lifetimes_balanced(r.context) ? "true" : "false")
            << ",\"global_setdown_error\":" << setdown_error << "}\n";
  r.hooks.unload_module(r.context);
  return valid ? 0 : (advertised ? 20 : 21);
}

int external_dependencies(const Request& r) {
  int32_t check_type = -1;
  try { check_type = std::stoi(r.external_dependency_check_type); } catch (...) {}
  if (check_type < 0 || check_type > 2) {
    if (r.global_error == 0) r.hooks.global_setdown(r.context);
    r.hooks.unload_module(r.context); return 3;
  }
  int32_t selector_error = -1; uint32_t exception = 0;
  stage("get_external_dependencies", "begin");
  void* handle = r.params_error == 0
      ? r.hooks.external_dependencies(r.context, check_type, &selector_error, &exception) : nullptr;
  stage("get_external_dependencies", "end", selector_error, true);
  uint64_t bytes = 0; bool nul_terminated = handle == nullptr; bool handle_valid = handle == nullptr;
  std::string text;
  if (handle) {
    handle_valid = r.hooks.handle_is_live(r.context, handle);
    bytes = handle_valid ? r.hooks.handle_size(r.context, handle) : 0;
    if (bytes > 0 && bytes <= kMaxDependencyBytes) {
      const char* data = static_cast<const char*>(r.hooks.lock_handle(r.context, handle));
      if (data) {
        const void* terminator = std::memchr(data, '\0', static_cast<std::size_t>(bytes));
        nul_terminated = terminator != nullptr;
        if (terminator) text.assign(data, static_cast<const char*>(terminator));
        r.hooks.unlock_handle(r.context, handle);
      }
    }
    r.hooks.dispose_handle(r.context, handle);
  }
  const bool disposed = !handle || !r.hooks.handle_is_live(r.context, handle);
  stage("global_setdown", "begin"); const int32_t setdown_error = global_setdown(r);
  stage("global_setdown", "end", setdown_error, true);
  const HandleStatistics stats = r.hooks.handle_statistics(r.context);
  const bool valid = selector_error == 0 && handle_valid && nul_terminated && disposed &&
      setdown_error == 0 && r.hooks.handle_lifetimes_balanced(r.context);
  r.hooks.restore_stdout(r.context);
  std::cout << "{\"schema_version\":1,\"stage\":\"external_dependencies\",\"status\":\""
            << (valid ? "dependencies_inspected" : "dependency_error")
            << "\",\"check_type\":" << check_type << ",\"selector_error\":" << selector_error
            << ",\"exception_code\":" << exception << ",\"dependency_text\":\""
            << escape_json(text) << "\",\"dependency_bytes\":" << bytes
            << ",\"handle_returned\":" << (handle ? "true" : "false")
            << ",\"handle_valid\":" << (handle_valid ? "true" : "false")
            << ",\"nul_terminated\":" << (nul_terminated ? "true" : "false")
            << ",\"handle_host_disposed\":" << (disposed ? "true" : "false")
            << ",\"handles_created\":" << stats.created << ",\"handles_disposed\":" << stats.disposed
            << ",\"handle_lifetimes_balanced\":"
            << (r.hooks.handle_lifetimes_balanced(r.context) ? "true" : "false")
            << ",\"global_setdown_error\":" << setdown_error << "}\n";
  r.hooks.unload_module(r.context); return valid ? 0 : 20;
}

int parameters_only(const Request& r) {
  const bool defaults_disposed = r.hooks.dispose_arbitrary_defaults(r.context);
  stage("global_setdown", "begin"); const int32_t setdown_error = global_setdown(r);
  stage("global_setdown", "end", setdown_error, true);
  if (r.hooks.module_audit_required(r.context) && !r.hooks.capture_pre_unload_audit_passed(r.context)) {
    r.hooks.restore_stdout(r.context);
    std::cout << "{\"schema_version\":1,\"stage\":\"module_audit\",\"status\":\"module_audit_failed\",\"module_audit\":"
              << r.hooks.module_audit_json(r.context) << "}\n";
    r.hooks.unload_module(r.context); return 14;
  }
  r.hooks.report_parameters(r.context,
      r.global_error == 0 && r.params_error == 0 && r.parameter_count_contract_valid
          ? "parameters_inspected" : "selector_error",
      r.global_error, r.params_error, setdown_error);
  r.hooks.unload_module(r.context);
  return r.global_error == 0 && r.params_error == 0 && r.parameter_count_contract_valid &&
      defaults_disposed && setdown_error == 0 ? 0 : 20;
}
}  // namespace

int run_early_mode(const Request& r) {
  switch (r.mode) {
    case EarlyMode::None: return -1;
    case EarlyMode::AutomaticDialog: return automatic_dialog(r);
    case EarlyMode::DoDialog: return do_dialog(r);
    case EarlyMode::ExternalDependencies: return external_dependencies(r);
    case EarlyMode::ParametersOnly: return parameters_only(r);
  }
  return -1;
}
}  // namespace aexcompat::l2mode
