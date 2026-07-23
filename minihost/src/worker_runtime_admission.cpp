#include "worker_runtime_admission.hpp"

#include "runtime_module_audit.hpp"

#include <iostream>

namespace aexcompat::worker_runtime {
namespace {

void reset_context(RuntimeContext& context) noexcept {
  context = {};
}

void remove_directory_cookie(DLL_DIRECTORY_COOKIE& cookie) noexcept {
  if (!cookie) return;
  RemoveDllDirectory(cookie);
  cookie = nullptr;
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
  const bool sealed = has_prefixed_basename(plugin_path.parent_path(),
                                            L"aexcompat-sealed-");

  // Authorization is parsed before LoadLibraryExW. The parser verifies every
  // exact dependency identity and fails closed before the target can execute.
  if (request.authorize_runtime_modules &&
      !parse_runtime_module_authorization(plugin_path,
                                          request.authorization_manifest)) return 15;

  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                                LOAD_LIBRARY_SEARCH_USER_DIRS)) return 11;
  // Static imports use DLL_LOAD_DIR below. Delay-load helpers call
  // LoadLibrary(name) later, so only an authenticated sealed root is admitted
  // to the process-wide USER_DIRS search set. PATH/CWD and arbitrary absolute
  // paths remain excluded by SetDefaultDllDirectories.
  DLL_DIRECTORY_COOKIE sealed_directory_cookie{};
  if (sealed) {
    sealed_directory_cookie = AddDllDirectory(plugin_path.parent_path().c_str());
    if (!sealed_directory_cookie) return 11;
  }
  HMODULE module = LoadLibraryExW(plugin_path.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) {
    remove_directory_cookie(sealed_directory_cookie);
    return 11;
  }

  ModuleAuditReport& audit = module_audit_report();
  audit.required = sealed;
  audit.plugin_path = plugin_path;
  if (audit.required) {
    audit.post_load = capture_module_audit();
    if (audit.post_load.status != "passed") {
      audit.pre_unload = capture_module_audit();
      std::cout << "{\"schema_version\":1,\"stage\":\"module_audit\","
                   "\"status\":\"module_audit_failed\",\"module_audit\":"
                << module_audit_json() << "}\n";
      FreeLibrary(module);
      remove_directory_cookie(sealed_directory_cookie);
      return 14;
    }
  }
  if (!hooks.redirect_native_stdout()) {
    FreeLibrary(module);
    remove_directory_cookie(sealed_directory_cookie);
    return 13;
  }
  context.plugin_path = plugin_path;
  context.module = module;
  context.sealed_directory_cookie = sealed_directory_cookie;
  context.stdout_redirected = true;
  context.restore_native_stdout = hooks.restore_native_stdout;
  return 0;
}

void release_runtime_context(RuntimeContext& context) noexcept {
  if (context.module) {
    FreeLibrary(context.module);
    context.module = nullptr;
  }
  remove_directory_cookie(context.sealed_directory_cookie);
  if (context.stdout_redirected && context.restore_native_stdout) {
    context.restore_native_stdout();
  }
  reset_context(context);
}

int prepare_runtime_request(const wchar_t* plugin_argument,
                            const wchar_t* expected_sha256,
                            bool authorize_runtime_modules,
                            const wchar_t* authorization_manifest,
                            RuntimeAdmissionRequest& request) {
  if (!plugin_argument || !expected_sha256) return 2;
  request = {};
  request.plugin_argument = plugin_argument;
  for (const wchar_t* character = expected_sha256; *character; ++character) {
    if (*character > 0x7f) return 2;
    request.expected_sha256.push_back(static_cast<char>(*character));
  }
  if (authorize_runtime_modules) {
    if (!authorization_manifest) return 2;
    request.authorize_runtime_modules = true;
    request.authorization_manifest = authorization_manifest;
  }
  return 0;
}

}  // namespace aexcompat::worker_runtime
