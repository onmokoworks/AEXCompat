#include "worker_runtime_admission.hpp"

#include "runtime_module_audit.hpp"

#include <bcrypt.h>

#include <array>
#include <cstring>
#include <iostream>
#include <system_error>
#include <utility>
#include <vector>

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

void close_file_handle(HANDLE& handle) noexcept {
  if (handle != INVALID_HANDLE_VALUE) CloseHandle(handle);
  handle = INVALID_HANDLE_VALUE;
}

bool same_file_identity(const BY_HANDLE_FILE_INFORMATION& left,
                        const BY_HANDLE_FILE_INFORMATION& right) noexcept {
  return left.dwVolumeSerialNumber == right.dwVolumeSerialNumber &&
      left.nFileIndexHigh == right.nFileIndexHigh &&
      left.nFileIndexLow == right.nFileIndexLow;
}

bool hash_file_handle_sha256(HANDLE file, std::string& digest) noexcept {
  digest.clear();
  if (!file || file == INVALID_HANDLE_VALUE) return false;
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  LARGE_INTEGER original{};
  LARGE_INTEGER zero{};
  bool restore_position = false;
  auto cleanup = [&]() noexcept {
    if (hash) BCryptDestroyHash(hash);
    if (algorithm) BCryptCloseAlgorithmProvider(algorithm, 0);
    if (restore_position)
      SetFilePointerEx(file, original, nullptr, FILE_BEGIN);
  };
  try {
    DWORD object_size{}, hash_size{}, returned{};
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM,
                                    nullptr, 0) < 0 ||
        BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                          reinterpret_cast<PUCHAR>(&object_size),
                          sizeof(object_size), &returned, 0) < 0 ||
        BCryptGetProperty(algorithm, BCRYPT_HASH_LENGTH,
                          reinterpret_cast<PUCHAR>(&hash_size),
                          sizeof(hash_size), &returned, 0) < 0 ||
        object_size == 0 || hash_size != 32 ||
        !SetFilePointerEx(file, zero, &original, FILE_CURRENT)) {
      cleanup();
      return false;
    }
    restore_position = true;
    if (!SetFilePointerEx(file, zero, nullptr, FILE_BEGIN)) {
      cleanup();
      return false;
    }
    std::vector<unsigned char> object(object_size);
    std::array<unsigned char, 64 * 1024> buffer{};
    std::array<unsigned char, 32> bytes{};
    if (BCryptCreateHash(algorithm, &hash, object.data(),
                         static_cast<ULONG>(object.size()), nullptr, 0, 0) < 0) {
      cleanup();
      return false;
    }
    for (;;) {
      DWORD read{};
      if (!ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()),
                    &read, nullptr)) {
        cleanup();
        return false;
      }
      if (read == 0) break;
      if (BCryptHashData(hash, buffer.data(), read, 0) < 0) {
        cleanup();
        return false;
      }
    }
    if (BCryptFinishHash(hash, bytes.data(),
                         static_cast<ULONG>(bytes.size()), 0) < 0) {
      cleanup();
      return false;
    }
    constexpr char hex[] = "0123456789abcdef";
    digest.reserve(bytes.size() * 2);
    for (const unsigned char byte : bytes) {
      digest.push_back(hex[byte >> 4]);
      digest.push_back(hex[byte & 0x0f]);
    }
    cleanup();
    return true;
  } catch (...) {
    digest.clear();
    cleanup();
    return false;
  }
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

bool capture_plugin_file_observation(
    HANDLE file, PluginFileObservation& observation) noexcept {
  observation = {};
  if (!file || file == INVALID_HANDLE_VALUE) return false;
  LARGE_INTEGER size{};
  if (!GetFileInformationByHandle(file, &observation.file_information) ||
      !GetFileSizeEx(file, &size) || size.QuadPart <= 0 ||
      !hash_file_handle_sha256(file, observation.sha256)) {
    observation = {};
    return false;
  }
  observation.size_bytes = static_cast<uint64_t>(size.QuadPart);
  return true;
}

void record_loaded_plugin_execution_image(
    uint32_t plugin_index, const std::filesystem::path& requested_path,
    HMODULE loaded_module, const PluginFileObservation* selected,
    const std::string& fallback_sha256,
    uint64_t fallback_size_bytes) noexcept {
  HANDLE loaded_file = INVALID_HANDLE_VALUE;
  try {
    std::filesystem::path observed_path = requested_path;
    std::string observed_sha256 = fallback_sha256;
    uint64_t observed_size = fallback_size_bytes;
    const char* status = "loaded_module_path_unavailable";
    std::array<wchar_t, 32768> path{};
    const DWORD length = loaded_module
        ? GetModuleFileNameW(loaded_module, path.data(),
                             static_cast<DWORD>(path.size()))
        : 0;
    if (length == 0 || length >= path.size()) {
      record_plugin_execution_image(plugin_index, observed_path,
                                    observed_sha256, observed_size, status);
      return;
    }
    observed_path = std::filesystem::path(path.data());
    loaded_file = CreateFileW(
        observed_path.c_str(), GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
        OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (loaded_file == INVALID_HANDLE_VALUE) {
      record_plugin_execution_image(
          plugin_index, observed_path, observed_sha256, observed_size,
          "loaded_module_identity_unavailable");
      return;
    }
    BY_HANDLE_FILE_INFORMATION loaded_information{};
    const bool loaded_identity_available =
        GetFileInformationByHandle(loaded_file, &loaded_information) != 0;
    if (selected && loaded_identity_available &&
        same_file_identity(selected->file_information, loaded_information)) {
      observed_sha256 = selected->sha256;
      observed_size = selected->size_bytes;
      status = "same_file_identity_matches_loaded_module";
    } else {
      // On a mismatch (or when the pre-load observation was unavailable), hash
      // the loaded module's resolved file through this same handle. This avoids
      // falsely attaching the selected file's digest to a different HMODULE.
      PluginFileObservation loaded_observation;
      if (capture_plugin_file_observation(loaded_file, loaded_observation)) {
        observed_sha256 = std::move(loaded_observation.sha256);
        observed_size = loaded_observation.size_bytes;
      }
      status = selected && loaded_identity_available
          ? "loaded_module_identity_mismatch"
          : "loaded_module_identity_unavailable";
    }
    CloseHandle(loaded_file);
    loaded_file = INVALID_HANDLE_VALUE;
    record_plugin_execution_image(plugin_index, observed_path,
                                  observed_sha256, observed_size, status);
  } catch (...) {
    // Provenance collection is record-only. Allocation/path conversion failure
    // must not change the already loaded plug-in's execution verdict.
    close_file_handle(loaded_file);
    record_plugin_execution_image(
        plugin_index, requested_path, fallback_sha256, fallback_size_bytes,
        "loaded_module_identity_unavailable");
  }
}

int admit_runtime(const RuntimeHostHooks& hooks,
                  const RuntimeAdmissionRequest& request,
                  RuntimeContext& context) {
  const int prepare_error =
      prepare_runtime_environment(hooks, request, context);
  if (prepare_error != 0) return prepare_error;
  const int load_error = load_runtime_plugin(request, context);
  if (load_error != 0) release_runtime_context(context);
  return load_error;
}

int prepare_runtime_environment(const RuntimeHostHooks& hooks,
                                const RuntimeAdmissionRequest& request,
                                RuntimeContext& context) {
  reset_context(context);
  if (!hooks.hash_file || !hooks.redirect_native_stdout ||
      !hooks.restore_native_stdout || request.expected_sha256.empty()) return 10;

  const std::filesystem::path plugin_path =
      std::filesystem::absolute(request.plugin_argument);
  if (!plugin_path.is_absolute()) return 11;

  // Execution identity is an observation, not an admission tier. Open with
  // ordinary sharing so provenance cannot pin the plug-in or reject a loader
  // behavior that worked before this diagnostic existed. If the handle cannot
  // be observed, retain the historical path-hash admission below and report
  // the missing binding only after a successful load.
  HANDLE plugin_file = CreateFileW(
      plugin_path.c_str(), GENERIC_READ,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
      OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  PluginFileObservation plugin_observation;
  bool plugin_observation_available =
      capture_plugin_file_observation(plugin_file, plugin_observation);
  if (!plugin_observation_available) {
    close_file_handle(plugin_file);
  }
  std::string actual_sha256;
  uint64_t plugin_size_bytes{};
  if (plugin_observation_available) {
    actual_sha256 = plugin_observation.sha256;
    plugin_size_bytes = plugin_observation.size_bytes;
  } else {
    // This is the pre-existing admission behavior and remains authoritative
    // when best-effort provenance cannot acquire a handle.
    if (!hooks.hash_file(plugin_path, actual_sha256)) return 10;
    std::error_code size_error;
    plugin_size_bytes = std::filesystem::file_size(plugin_path, size_error);
    if (size_error) plugin_size_bytes = 0;
  }
  if (actual_sha256 != request.expected_sha256) {
    close_file_handle(plugin_file);
    return 10;
  }

  // Authorization is parsed before LoadLibraryExW. The parser verifies every
  // exact dependency identity and fails closed before the target can execute.
  if (request.authorize_runtime_modules &&
      !parse_runtime_module_authorization(plugin_path,
                                          request.authorization_manifest,
                                          !request.dependency_search_dirs.empty())) {
    close_file_handle(plugin_file);
    return 15;
  }

  // This worker only supports PF effects. Reject an AEGP candidate from an
  // image-only preflight so its DllMain and delay-loaded dependencies never
  // execute in a PF inspection process. In particular, this keeps an AEGP's
  // third-party teardown outside the PF worker's shutdown contract (#377).
  if (!request.allow_aegp_plugin &&
      is_aegp_candidate_without_execution(plugin_path)) {
    std::cerr << "plugin_kind:aegp_candidate\n" << std::flush;
    close_file_handle(plugin_file);
    return 12;
  }

  if (!SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 |
                                LOAD_LIBRARY_SEARCH_USER_DIRS)) {
    const DWORD error = GetLastError();
    close_file_handle(plugin_file);
    return report_load_failure("set_default_dll_directories", error);
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
      close_file_handle(plugin_file);
      return report_load_failure("add_dll_directory", error);
    }
    search_directory_cookies.push_back(cookie);
  }
  if (!hooks.redirect_native_stdout()) {
    remove_cookies();
    close_file_handle(plugin_file);
    return 13;
  }
  context.plugin_path = plugin_path;
  context.plugin_observation_handle = plugin_file;
  context.plugin_file_observation = std::move(plugin_observation);
  context.plugin_file_observation_available = plugin_observation_available;
  context.plugin_sha256 = actual_sha256;
  context.plugin_size_bytes = plugin_size_bytes;
  context.sealed_directory_cookie = sealed_directory_cookie;
  context.search_directory_cookies = std::move(search_directory_cookies);
  context.stdout_redirected = true;
  context.restore_native_stdout = hooks.restore_native_stdout;
  return 0;
}

int load_runtime_plugin(const RuntimeAdmissionRequest& request,
                        RuntimeContext& context) {
  if (context.plugin_path.empty() || context.module) return 10;
  // In-place loads (issue #751) resolve the plug-in's static imports through
  // the USER_DIRS search set as well, because the dependency closure lives in
  // its real directories instead of beside a staged copy.
  const bool in_place = !request.dependency_search_dirs.empty();
  const std::filesystem::path& plugin_path = context.plugin_path;
  HMODULE module = LoadLibraryExW(plugin_path.c_str(), nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32 |
      (in_place ? LOAD_LIBRARY_SEARCH_USER_DIRS : 0));
  if (!module) {
    const DWORD error = GetLastError();
    return report_load_failure("load_library", error);
  }
  const PluginFileObservation* selected =
      context.plugin_file_observation_available
      ? &context.plugin_file_observation
      : nullptr;
  record_loaded_plugin_execution_image(
      0, plugin_path, module, selected, context.plugin_sha256,
      context.plugin_size_bytes);
  // The observation handle is not an execution control and does not belong to
  // the resident WorkerSession. Close it immediately after binding the report
  // to the loaded HMODULE; all later frames reuse the recorded digest.
  close_file_handle(context.plugin_observation_handle);
  context.plugin_file_observation = {};
  context.plugin_file_observation_available = false;
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
      return 14;
    }
  } else if (audit.recorded) {
    audit.post_load = capture_module_audit();
  }
  context.module = module;
  return 0;
}

void release_runtime_context(RuntimeContext& context) noexcept {
  if (context.module) {
    FreeLibrary(context.module);
    context.module = nullptr;
  }
  close_file_handle(context.plugin_observation_handle);
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
