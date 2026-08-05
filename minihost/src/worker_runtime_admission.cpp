#include "worker_runtime_admission.hpp"

#include "runtime_module_audit.hpp"

#include <cstring>
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

void remove_directory_cookies(
    std::vector<DLL_DIRECTORY_COOKIE>& cookies) noexcept {
  for (DLL_DIRECTORY_COOKIE& cookie : cookies) remove_directory_cookie(cookie);
  cookies.clear();
}

bool pipl_resource_is_aegp(const unsigned char* bytes,
                           std::size_t size) noexcept {
  constexpr std::size_t kMaxPiplBytes = 1024 * 1024;
  if (!bytes || size < 20 || size > kMaxPiplBytes) return false;
  // The PiPL kind property is a MIB8/dnik property whose four-byte value is
  // xgEA. This is deliberately only a discriminator: the full PiPL parser
  // remains the authoritative effect-entrypoint validator after admission.
  for (std::size_t offset = 0; offset + 20 <= size; ++offset) {
    if (std::memcmp(bytes + offset, "MIB8", 4) != 0 ||
        std::memcmp(bytes + offset + 4, "dnik", 4) != 0 ||
        bytes[offset + 12] != 4 || bytes[offset + 13] != 0 ||
        bytes[offset + 14] != 0 || bytes[offset + 15] != 0 ||
        std::memcmp(bytes + offset + 16, "xgEA", 4) != 0)
      continue;
    return true;
  }
  return false;
}

struct AegpPreflightScan {
  HMODULE module{};
  bool found{};
  std::size_t names_seen{};
};

BOOL CALLBACK scan_pipl_resource_name(HMODULE module, LPCWSTR, LPWSTR name,
                                      LONG_PTR context) {
  auto* scan = reinterpret_cast<AegpPreflightScan*>(context);
  if (!scan || scan->names_seen++ >= 64) return FALSE;
  const HRSRC resource = FindResourceW(module, name, L"PiPL");
  if (!resource) return TRUE;
  const DWORD size = SizeofResource(module, resource);
  const HGLOBAL loaded = LoadResource(module, resource);
  const auto* bytes = loaded
      ? static_cast<const unsigned char*>(LockResource(loaded)) : nullptr;
  if (pipl_resource_is_aegp(bytes, size)) {
    scan->found = true;
    return FALSE;
  }
  return TRUE;
}

bool is_aegp_candidate_without_execution(
    const std::filesystem::path& plugin_path) noexcept {
  // Map the image without resolving imports or calling DllMain. The PiPL
  // discriminator lets the PF-only worker reject an AEGP before third-party
  // dependency initialization can reach process teardown. The normal
  // LoadLibraryExW below remains the only executable admission path.
  const HMODULE preflight_module = LoadLibraryExW(
      plugin_path.c_str(), nullptr, DONT_RESOLVE_DLL_REFERENCES);
  if (!preflight_module) return false;
  AegpPreflightScan scan{preflight_module};
  EnumResourceNamesW(preflight_module, L"PiPL", &scan_pipl_resource_name,
                     reinterpret_cast<LONG_PTR>(&scan));
  const bool aegp_candidate = scan.found;
  FreeLibrary(preflight_module);
  return aegp_candidate;
}

int report_load_failure(const char* stage, DWORD error) noexcept {
  std::cerr << "stage:load_failure stage=" << stage
            << " win32_error=" << static_cast<unsigned long>(error) << '\n'
            << std::flush;
  return 11;
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
                                          request.authorization_manifest,
                                          !request.dependency_search_dirs.empty())) return 15;

  // This worker only supports PF effects. Reject an AEGP candidate from an
  // image-only preflight so its DllMain and delay-loaded dependencies never
  // execute in a PF inspection process. In particular, this keeps an AEGP's
  // third-party teardown outside the PF worker's shutdown contract (#377).
  if (!request.allow_aegp_plugin &&
      is_aegp_candidate_without_execution(plugin_path)) {
    std::cerr << "plugin_kind:aegp_candidate\n" << std::flush;
    return 12;
  }

  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                                LOAD_LIBRARY_SEARCH_USER_DIRS)) {
    return report_load_failure("set_default_dll_directories", GetLastError());
  }
  // Static imports use DLL_LOAD_DIR below. Delay-load helpers call
  // LoadLibrary(name) later, so only broker-validated in-place search
  // directories are admitted
  // to the process-wide USER_DIRS search set. PATH/CWD and arbitrary absolute
  // paths remain excluded by SetDefaultDllDirectories.
  DLL_DIRECTORY_COOKIE sealed_directory_cookie{};
  std::vector<DLL_DIRECTORY_COOKIE> search_directory_cookies;
  const auto remove_cookies = [&]() noexcept {
    remove_directory_cookies(search_directory_cookies);
    remove_directory_cookie(sealed_directory_cookie);
  };
  for (const std::filesystem::path& directory :
       request.dependency_search_dirs) {
    const DLL_DIRECTORY_COOKIE cookie = AddDllDirectory(directory.c_str());
    if (!cookie) {
      const DWORD error = GetLastError();
      remove_cookies();
      return report_load_failure("add_dll_directory", error);
    }
    search_directory_cookies.push_back(cookie);
  }
  // In-place loads (issue #751) resolve the plug-in's static imports through
  // the USER_DIRS search set as well, because the dependency closure lives in
  // its real directories instead of beside a staged copy.
  const bool in_place = !request.dependency_search_dirs.empty();
  HMODULE module = LoadLibraryExW(plugin_path.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32 |
      (in_place ? LOAD_LIBRARY_SEARCH_USER_DIRS : 0));
  if (!module) {
    const DWORD error = GetLastError();
    remove_cookies();
    return report_load_failure("load_library", error);
  }
  ModuleAuditReport& audit = module_audit_report();
  // Record, never enforce (issue #678/#751): the in-place route captures the
  // loaded-module set as provenance, and no status here fails the launch.
  audit.required = false;
  audit.recorded = in_place;
  audit.plugin_path = plugin_path;
  if (in_place)
    configure_module_audit_search_roots(request.dependency_search_dirs);
  if (audit.required) {
    audit.post_load = capture_module_audit();
    if (audit.post_load.status != "passed") {
      audit.pre_unload = capture_module_audit();
      std::cout << "{\"schema_version\":1,\"stage\":\"module_audit\","
                   "\"status\":\"module_audit_failed\",\"module_audit\":"
                << module_audit_json() << "}\n";
      FreeLibrary(module);
      remove_cookies();
      return 14;
    }
  } else if (audit.recorded) {
    audit.post_load = capture_module_audit();
  }
  if (!hooks.redirect_native_stdout()) {
    FreeLibrary(module);
    remove_cookies();
    return 13;
  }
  context.plugin_path = plugin_path;
  context.module = module;
  context.sealed_directory_cookie = sealed_directory_cookie;
  context.search_directory_cookies = std::move(search_directory_cookies);
  context.stdout_redirected = true;
  context.restore_native_stdout = hooks.restore_native_stdout;
  return 0;
}

void release_runtime_context(RuntimeContext& context) noexcept {
  if (context.module) {
    FreeLibrary(context.module);
    context.module = nullptr;
  }
  remove_directory_cookies(context.search_directory_cookies);
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

int apply_dependency_search_dirs(const wchar_t* joined,
                                 RuntimeAdmissionRequest& request) {
  constexpr std::size_t kMaxSearchDirs = 16;
  constexpr std::size_t kMaxJoinedLength = 32768;
  if (!joined || !*joined) return 2;
  const std::wstring text(joined);
  if (text.size() > kMaxJoinedLength) return 2;
  request.dependency_search_dirs.clear();
  std::size_t start = 0;
  while (start <= text.size()) {
    const std::size_t end = text.find(L';', start);
    const std::wstring entry = text.substr(
        start, end == std::wstring::npos ? std::wstring::npos : end - start);
    const std::filesystem::path directory(entry);
    if (entry.empty() || !directory.is_absolute() ||
        request.dependency_search_dirs.size() >= kMaxSearchDirs) {
      request.dependency_search_dirs.clear();
      return 2;
    }
    request.dependency_search_dirs.push_back(directory);
    if (end == std::wstring::npos) break;
    start = end + 1;
  }
  return 0;
}

}  // namespace aexcompat::worker_runtime
