#include "worker_runtime_admission.hpp"

#include "runtime_module_audit.hpp"

#include <iostream>

namespace aexcompat::worker_runtime {
namespace {

void reset_context(RuntimeContext& context) noexcept {
  context = {};
}

}  // namespace

int admit_runtime(const RuntimeHostHooks& hooks,
                  const RuntimeAdmissionRequest& request,
                  RuntimeContext& context) {
  reset_context(context);
  if (!hooks.hash_file || !hooks.redirect_native_stdout ||
      !hooks.restore_native_stdout || request.expected_sha256.empty()) return 10;

  std::string actual_sha256;
  if (!hooks.hash_file(request.plugin_argument, actual_sha256) ||
      actual_sha256 != request.expected_sha256) return 10;
  const std::filesystem::path plugin_path =
      std::filesystem::absolute(request.plugin_argument);
  if (!plugin_path.is_absolute()) return 11;

  // Authorization is parsed before LoadLibraryExW. The parser verifies every
  // exact dependency identity and fails closed before the target can execute.
  if (request.authorize_runtime_modules &&
      !parse_runtime_module_authorization(plugin_path,
                                          request.authorization_manifest)) return 15;

  SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                           LOAD_LIBRARY_SEARCH_USER_DIRS);
  HMODULE module = LoadLibraryExW(plugin_path.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) return 11;

  ModuleAuditReport& audit = module_audit_report();
  audit.required = has_prefixed_basename(plugin_path.parent_path(),
                                         L"aexcompat-sealed-");
  audit.plugin_path = plugin_path;
  if (audit.required) {
    audit.post_load = capture_module_audit();
    if (audit.post_load.status != "passed") {
      audit.pre_unload = capture_module_audit();
      std::cout << "{\"schema_version\":1,\"stage\":\"module_audit\","
                   "\"status\":\"module_audit_failed\",\"module_audit\":"
                << module_audit_json() << "}\n";
      FreeLibrary(module);
      return 14;
    }
  }
  if (!hooks.redirect_native_stdout()) {
    FreeLibrary(module);
    return 13;
  }
  context.plugin_path = plugin_path;
  context.module = module;
  context.stdout_redirected = true;
  return 0;
}

}  // namespace aexcompat::worker_runtime
