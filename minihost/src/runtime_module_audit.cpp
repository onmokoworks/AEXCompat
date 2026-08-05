#include "runtime_module_audit.hpp"

#include <windows.h>
#include <psapi.h>
#include <bcrypt.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <fstream>
#include <set>
#include <sstream>
#include <type_traits>
#include <utility>

namespace aexcompat::worker_runtime {
namespace {

#pragma comment(lib, "psapi.lib")
#pragma comment(lib, "bcrypt.lib")

constexpr std::size_t kMaxAuditedModules = 512;
constexpr std::size_t kMaxAuditFailureRejections = 16;
// Cluster sessions (issue #405) replace the fixed bound with the manifest's
// launch-time authenticated module_bound (design §5) and narrow the `plugin`
// classification to the manifest's declared basenames. Zero/empty means the
// legacy fixed-bound behavior.
std::size_t g_cluster_module_bound = 0;
std::set<std::string> g_declared_plugin_basenames;
// In-place load mode (issue #751): dependency search directories admitted at
// runtime admission, already canonicalized by configure below. A module whose
// parent is one of these roots classifies as `plugin`.
std::vector<std::filesystem::path> g_audit_search_roots;

std::size_t audit_module_bound() {
  return g_cluster_module_bound != 0 ? g_cluster_module_bound : kMaxAuditedModules;
}

struct AuthorizedRuntimeModule {
  std::filesystem::path path;
  uint64_t size{};
  std::string sha256;
};

std::vector<AuthorizedRuntimeModule> g_authorized_runtime_modules;
// The 32-byte session identity and GPU-framework backend id the last accepted
// AEXRMA1 manifest carried. `parse_runtime_module_authorization` used to discard
// both after validation; the GPU module-audit preflight (#290) echoes them into
// the report the broker re-authenticates, so they are retained here.
std::array<unsigned char, 32> g_authorized_session_identity{};
uint32_t g_authorized_backend{};
ModuleAuditReport g_module_audit;
FileSha256 g_file_sha256{};

// SHA-256 domain separation prefix for module path tokens; must match the broker
// `runtime_module_policy::PATH_TOKEN_DOMAIN` byte-for-byte (two embedded NULs).
constexpr unsigned char kPathTokenDomain[] = {
    'A', 'E', 'X', 'C', 'o', 'm', 'p', 'a', 't', ' ', 'r', 'u', 'n',
    't', 'i', 'm', 'e', ' ', 'm', 'o', 'd', 'u', 'l', 'e', ' ', 'p',
    'a', 't', 'h', ' ', 't', 'o', 'k', 'e', 'n', 0, 'v', '1', 0};

// Lowercase-hex SHA-256 of an arbitrary byte range. Separate from the file
// hasher (`g_file_sha256`) so the path-token computation does not touch disk.
std::string hash_bytes_hex(const unsigned char* data, std::size_t size) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
    return {};
  std::string result;
  if (BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
          reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &returned, 0) >= 0) {
    std::vector<unsigned char> object(object_size);
    if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0) >= 0) {
      std::array<unsigned char, 32> digest{};
      if (BCryptHashData(hash, const_cast<PUCHAR>(data), static_cast<ULONG>(size), 0) >= 0 &&
          BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0) {
        constexpr char hex[] = "0123456789abcdef";
        result.reserve(64);
        for (unsigned char byte : digest) {
          result.push_back(hex[byte >> 4]);
          result.push_back(hex[byte & 0x0f]);
        }
      }
      BCryptDestroyHash(hash);
    }
  }
  BCryptCloseAlgorithmProvider(algorithm, 0);
  return result;
}

// Reproduces the broker `runtime_module_policy::path_token`: the SHA-256 of the
// domain prefix followed by the folded path bytes, where the folded path is the
// canonicalized module path lowercased with forward slashes turned to
// backslashes. The broker computes it over Rust's `fs::canonicalize` output,
// which is `\\?\`-prefixed on Windows, so the prefix is restored here before
// folding (the audit's `canonical_path` strips it). ASCII module paths fold
// identically to Rust's Unicode lowercase; a non-ASCII path folds differently
// and the broker re-authentication then rejects the report fail-closed.
std::string path_token(const std::filesystem::path& canonical_stripped) {
  std::wstring folded = L"\\\\?\\" + canonical_stripped.wstring();
  for (wchar_t& ch : folded) {
    if (ch == L'/') ch = L'\\';
    ch = towlower(ch);
  }
  const int needed = WideCharToMultiByte(CP_UTF8, 0, folded.c_str(),
      static_cast<int>(folded.size()), nullptr, 0, nullptr, nullptr);
  if (needed <= 0) return {};
  std::string utf8(static_cast<std::size_t>(needed), '\0');
  WideCharToMultiByte(CP_UTF8, 0, folded.c_str(), static_cast<int>(folded.size()),
      utf8.data(), needed, nullptr, nullptr);
  std::vector<unsigned char> buffer(kPathTokenDomain,
      kPathTokenDomain + sizeof(kPathTokenDomain));
  buffer.insert(buffer.end(), utf8.begin(), utf8.end());
  return hash_bytes_hex(buffer.data(), buffer.size());
}

std::wstring lowercase(std::wstring value) {
  std::transform(value.begin(), value.end(), value.begin(), towlower);
  return value;
}

std::string lowercase(const std::string& value) {
  std::string result = value;
  for (char& ch : result)
    if (ch >= 'A' && ch <= 'Z') ch = static_cast<char>(ch - 'A' + 'a');
  return result;
}

bool canonical_path(const std::filesystem::path& path,
                    std::filesystem::path& result) {
  HANDLE file = CreateFileW(path.c_str(), FILE_READ_ATTRIBUTES,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr, OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS, nullptr);
  if (file == INVALID_HANDLE_VALUE) return false;
  std::vector<wchar_t> buffer(32768);
  const DWORD length = GetFinalPathNameByHandleW(
      file, buffer.data(), static_cast<DWORD>(buffer.size()),
      FILE_NAME_NORMALIZED | VOLUME_NAME_DOS);
  CloseHandle(file);
  if (length == 0 || length >= buffer.size()) return false;
  std::wstring normalized(buffer.data(), length);
  if (normalized.rfind(L"\\\\?\\", 0) == 0) normalized.erase(0, 4);
  result = std::filesystem::path(normalized).lexically_normal();
  return result.is_absolute();
}

bool same_path(const std::filesystem::path& left,
               const std::filesystem::path& right) {
  return lowercase(left.wstring()) == lowercase(right.wstring());
}

// A canonicalized path alone is not sufficient for the WinSxS allowlist: a
// junction/symlink could resolve into an apparently valid assembly directory.
// Inspect every existing component without following reparse points and fail
// closed when any handle or metadata query is unavailable.
bool contains_reparse_component(const std::filesystem::path& path) {
  std::filesystem::path current = path;
  while (!current.empty()) {
    HANDLE file = CreateFileW(current.c_str(), FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
        OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        nullptr);
    if (file == INVALID_HANDLE_VALUE) return true;
    BY_HANDLE_FILE_INFORMATION information{};
    const bool failed = GetFileInformationByHandle(file, &information) == 0;
    CloseHandle(file);
    if (failed || (information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0)
      return true;
    const std::filesystem::path parent = current.parent_path();
    if (parent.empty() || parent == current) break;
    current = parent;
  }
  return false;
}

bool is_winsxs_module(const std::filesystem::path& module_path,
                      const std::filesystem::path& winsxs_root) {
  const std::filesystem::path assembly = module_path.parent_path();
  return !module_path.filename().empty() && !assembly.filename().empty() &&
      same_path(assembly.parent_path(), winsxs_root) &&
      !contains_reparse_component(module_path);
}

// The OS DriverStore is the modern home of user-mode driver DLLs (the GPU
// vendors' OpenGL/compute ICDs): like WinSxS it is an OS-managed component
// store under the system directory that only the driver installer may
// write, so a module packaged directly inside a FileRepository package
// directory classifies like a WinSxS assembly instead of failing the audit
// closed (issue #362). An empty root (a machine without the store) admits
// nothing.
bool is_driverstore_module(const std::filesystem::path& module_path,
                           const std::filesystem::path& driverstore_root) {
  if (driverstore_root.empty()) return false;
  const std::filesystem::path package = module_path.parent_path();
  return !module_path.filename().empty() && !package.filename().empty() &&
      same_path(package.parent_path(), driverstore_root) &&
      !contains_reparse_component(module_path);
}

std::string audit_basename(const std::filesystem::path& path) {
  const std::wstring name = path.filename().wstring();
  std::string result;
  result.reserve(name.size());
  for (wchar_t ch : name)
    result.push_back(ch >= 0x20 && ch <= 0x7e ? static_cast<char>(ch) : '?');
  return result;
}

template <typename T>
bool read_manifest_le(const std::vector<unsigned char>& bytes,
                      std::size_t& offset, T& value) {
  static_assert(std::is_unsigned_v<T>);
  if (offset > bytes.size() || bytes.size() - offset < sizeof(T)) return false;
  value = 0;
  for (std::size_t index = 0; index < sizeof(T); ++index)
    value |= static_cast<T>(bytes[offset + index]) << (index * 8);
  offset += sizeof(T);
  return true;
}

bool authorized_runtime_module(const std::filesystem::path& module_path) {
  const auto found = std::find_if(g_authorized_runtime_modules.begin(),
      g_authorized_runtime_modules.end(), [&](const AuthorizedRuntimeModule& entry) {
        return same_path(module_path, entry.path);
      });
  if (found == g_authorized_runtime_modules.end() || !g_file_sha256) return false;
  std::error_code error;
  const uint64_t size = std::filesystem::file_size(module_path, error);
  std::string digest;
  return !error && size == found->size && g_file_sha256(module_path, digest) &&
      digest == found->sha256;
}

ModuleAuditSnapshot audit_loaded_modules(const std::filesystem::path& plugin_path) {
  ModuleAuditSnapshot snapshot;
  snapshot.status = "failed";
  const std::size_t module_bound = audit_module_bound();
  std::vector<HMODULE> modules(module_bound);
  DWORD needed = 0;
  if (!EnumProcessModulesEx(GetCurrentProcess(), modules.data(),
          static_cast<DWORD>(modules.size() * sizeof(HMODULE)), &needed, LIST_MODULES_ALL) ||
      needed == 0 || needed > modules.size() * sizeof(HMODULE) || needed % sizeof(HMODULE) != 0) {
    snapshot.unknown_count = 1;
    return snapshot;
  }

  std::array<wchar_t, 32768> executable_buffer{};
  const DWORD executable_length = GetModuleFileNameW(
      nullptr, executable_buffer.data(), static_cast<DWORD>(executable_buffer.size()));
  std::array<wchar_t, MAX_PATH> system_buffer{};
  const UINT system_length = GetSystemDirectoryW(
      system_buffer.data(), static_cast<UINT>(system_buffer.size()));
  std::array<wchar_t, 32768> windows_buffer{};
  const UINT windows_length = GetWindowsDirectoryW(
      windows_buffer.data(), static_cast<UINT>(windows_buffer.size()));
  std::filesystem::path executable, plugin_root, system32, windows_root, winsxs_root,
      driverstore_root;
  if (executable_length == 0 || executable_length >= executable_buffer.size() ||
      system_length == 0 || system_length >= system_buffer.size() ||
      windows_length == 0 || windows_length >= windows_buffer.size() ||
      !canonical_path(executable_buffer.data(), executable) ||
      !canonical_path(plugin_path.parent_path(), plugin_root) ||
      !canonical_path(system_buffer.data(), system32) ||
      !canonical_path(windows_buffer.data(), windows_root) ||
      !canonical_path(windows_root / L"WinSxS", winsxs_root) ||
      contains_reparse_component(windows_buffer.data()) ||
      contains_reparse_component(
          std::filesystem::path(windows_buffer.data()) / L"WinSxS") ||
      !has_prefixed_basename(executable.parent_path(), L"aexcompat-trusted-worker-")) {
    snapshot.unknown_count = 1;
    return snapshot;
  }
  // The DriverStore is optional (always present on a normal Windows install,
  // but its absence must not fail an audit that never loads a driver): an
  // empty root classifies nothing, matching the pre-#362 behavior there.
  const std::filesystem::path driverstore_candidate =
      system32 / L"DriverStore" / L"FileRepository";
  if (!canonical_path(driverstore_candidate, driverstore_root) ||
      contains_reparse_component(driverstore_root))
    driverstore_root.clear();

  const std::size_t count = needed / sizeof(HMODULE);
  for (std::size_t index = 0; index < count; ++index) {
    std::array<wchar_t, 32768> module_buffer{};
    const DWORD module_length = GetModuleFileNameExW(GetCurrentProcess(), modules[index],
        module_buffer.data(), static_cast<DWORD>(module_buffer.size()));
    std::filesystem::path module_path;
    if (module_length == 0 || module_length >= module_buffer.size() ||
        !canonical_path(module_buffer.data(), module_path)) {
      ++snapshot.unknown_count;
      continue;
    }
    const std::string basename = audit_basename(module_path);
    if (same_path(module_path, executable)) snapshot.worker.push_back(basename);
    else if (same_path(module_path.parent_path(), plugin_root)) {
      // Cluster sessions narrow the plugin class to the manifest's declared
      // basename set (design §5): anything else under the sealed root is an
      // unknown module and fails the audit closed.
      if (!g_declared_plugin_basenames.empty() &&
          g_declared_plugin_basenames.count(lowercase(basename)) == 0) {
        ++snapshot.unknown_count;
        snapshot.unknown_keys.push_back(lowercase(module_path.wstring()));
      } else {
        snapshot.plugin.push_back(basename);
      }
    }
    else if (std::any_of(g_audit_search_roots.begin(), g_audit_search_roots.end(),
                         [&](const std::filesystem::path& root) {
                           return same_path(module_path.parent_path(), root);
                         }))
      snapshot.plugin.push_back(basename);
    else if (same_path(module_path.parent_path(), system32)) snapshot.system32.push_back(basename);
    else if (is_winsxs_module(module_path, winsxs_root) &&
             !contains_reparse_component(module_buffer.data()))
      snapshot.winsxs.push_back(basename);
    else if (is_driverstore_module(module_path, driverstore_root))
      snapshot.driverstore.push_back(basename);
    else if (authorized_runtime_module(module_path)) snapshot.policy.push_back(basename);
    else {
      ++snapshot.unknown_count;
      snapshot.unknown_keys.push_back(lowercase(module_path.wstring()));
    }
  }
  snapshot.status = snapshot.unknown_count == 0 ? "passed" : "failed";
  return snapshot;
}

bool module_audit_observing() {
  return g_module_audit.required || g_module_audit.recorded;
}

void accumulate_module_audit(const ModuleAuditSnapshot& snapshot) {
  if (!module_audit_observing()) return;
  ++g_module_audit.phase_count;
  // Cluster sessions may only WIDEN the historical fixed cap to the manifest's
  // declared module_bound (design §5); it never shrinks, so the fixed-cap
  // contract below is unchanged on every non-cluster path.
  const std::size_t module_bound = audit_module_bound();
  auto append_unique = [&module_bound](std::vector<std::string>& target,
                          const std::vector<std::string>& source) {
    for (const auto& value : source) {
      if (std::find(target.begin(), target.end(), value) != target.end()) continue;
      if (target.size() >= kMaxAuditedModules && target.size() >= module_bound) {
        g_module_audit.observed_union.unknown_count =
            (std::max)(1u, g_module_audit.observed_union.unknown_count);
        continue;
      }
      target.push_back(value);
    }
  };
  append_unique(g_module_audit.observed_union.worker, snapshot.worker);
  append_unique(g_module_audit.observed_union.plugin, snapshot.plugin);
  append_unique(g_module_audit.observed_union.system32, snapshot.system32);
  append_unique(g_module_audit.observed_union.winsxs, snapshot.winsxs);
  append_unique(g_module_audit.observed_union.driverstore, snapshot.driverstore);
  append_unique(g_module_audit.observed_union.policy, snapshot.policy);
  for (const auto& key : snapshot.unknown_keys) {
    auto& keys = g_module_audit.observed_union.unknown_keys;
    if (std::find(keys.begin(), keys.end(), key) != keys.end()) continue;
    if (keys.size() >= kMaxAuditedModules && keys.size() >= module_bound) {
      g_module_audit.observed_union.unknown_count =
          (std::max)(1u, g_module_audit.observed_union.unknown_count);
      continue;
    }
    keys.push_back(key);
  }
  g_module_audit.observed_union.unknown_count = (std::max)(
      g_module_audit.observed_union.unknown_count,
      static_cast<uint32_t>(g_module_audit.observed_union.unknown_keys.size()));
  if (snapshot.unknown_count != 0 && snapshot.unknown_keys.empty())
    g_module_audit.observed_union.unknown_count =
        (std::max)(1u, g_module_audit.observed_union.unknown_count);
  g_module_audit.observed_union.status =
      g_module_audit.observed_union.unknown_count == 0 ? "passed" : "failed";
}

std::string module_audit_snapshot_json(const ModuleAuditSnapshot& snapshot) {
  auto names = [](const std::vector<std::string>& values) {
    std::ostringstream output;
    output << '[';
    for (std::size_t index = 0; index < values.size(); ++index) {
      if (index) output << ',';
      output << '\"';
      for (char ch : values[index]) {
        if (ch == '\"' || ch == '\\') output << '\\';
        output << ch;
      }
      output << '\"';
    }
    output << ']';
    return output.str();
  };
  std::vector<std::string> unknown;
  unknown.reserve(snapshot.unknown_keys.size());
  for (const auto& key : snapshot.unknown_keys) {
    const std::string basename = audit_basename(std::filesystem::path(key));
    if (basename.empty() ||
        std::find(unknown.begin(), unknown.end(), basename) != unknown.end())
      continue;
    unknown.push_back(basename);
  }
  std::ostringstream output;
  output << "{\"status\":\"" << snapshot.status << "\",\"unknown_count\":"
         << snapshot.unknown_count << ",\"worker\":" << names(snapshot.worker)
         << ",\"plugin\":" << names(snapshot.plugin)
         << ",\"system32\":" << names(snapshot.system32)
         << ",\"winsxs\":" << names(snapshot.winsxs)
         << ",\"driverstore\":" << names(snapshot.driverstore)
         << ",\"policy\":" << names(snapshot.policy)
         << ",\"unknown\":" << names(unknown) << '}';
  return output.str();
}

}  // namespace

void configure_runtime_module_hash(FileSha256 hash) noexcept {
  g_file_sha256 = hash;
}

void configure_module_audit_cluster(std::size_t module_bound,
                                    std::vector<std::string> declared_plugin_basenames) {
  g_cluster_module_bound = module_bound;
  g_declared_plugin_basenames.clear();
  for (const std::string& basename : declared_plugin_basenames)
    g_declared_plugin_basenames.insert(lowercase(basename));
}

void configure_module_audit_search_roots(
    const std::vector<std::filesystem::path>& roots) {
  g_audit_search_roots.clear();
  for (const std::filesystem::path& root : roots) {
    std::filesystem::path canonical;
    if (canonical_path(root, canonical)) g_audit_search_roots.push_back(canonical);
  }
}

void record_module_audit_epoch(uint32_t plugin_index,
                               ModuleAuditSnapshot pre_unload,
                               ModuleAuditSnapshot post_load) {
  if (!module_audit_observing()) return;
  g_module_audit.epochs.push_back(
      {plugin_index, std::move(pre_unload), std::move(post_load)});
}

bool parse_runtime_module_authorization(const std::filesystem::path& plugin_path,
                                        const std::filesystem::path& manifest_name,
                                        bool in_place_transport) {
  g_authorized_runtime_modules.clear();
  g_authorized_session_identity.fill(0);
  g_authorized_backend = 0;
  if (!g_file_sha256 || manifest_name.empty() ||
      manifest_name.is_absolute() != in_place_transport) return false;
  std::filesystem::path plugin_root, manifest_path;
  if (in_place_transport) {
    // #815: the broker owns this absolute target/image-transport document and
    // keeps it alive for the resident session. It is parsed completely before
    // LoadLibraryExW, and never becomes a DLL search root or executable path.
    if (!canonical_path(manifest_name, manifest_path)) return false;
  } else {
    if (manifest_name.has_parent_path() || manifest_name.filename() != manifest_name ||
        !canonical_path(plugin_path.parent_path(), plugin_root) ||
        !canonical_path(plugin_root / manifest_name, manifest_path) ||
        !same_path(manifest_path.parent_path(), plugin_root)) return false;
  }
  std::error_code error;
  const uint64_t manifest_size = std::filesystem::file_size(manifest_path, error);
  if (error || manifest_size > 16 * 1024 * 1024) return false;
  std::ifstream input(manifest_path, std::ios::binary);
  std::vector<unsigned char> bytes(static_cast<std::size_t>(manifest_size));
  if ((!bytes.empty() && !input.read(reinterpret_cast<char*>(bytes.data()), bytes.size())) ||
      input.peek() != std::ifstream::traits_type::eof()) return false;

  constexpr std::array<unsigned char, 8> magic{'A','E','X','R','M','A','1',0};
  if (bytes.size() < magic.size() || !std::equal(magic.begin(), magic.end(), bytes.begin()))
    return false;
  std::size_t offset = magic.size();
  uint32_t purpose{}, backend{}, count{};
  uint64_t expiry{};
  if (!read_manifest_le(bytes, offset, purpose) || purpose != 1 ||
      !read_manifest_le(bytes, offset, backend) || backend < 1 || backend > 4 ||
      !read_manifest_le(bytes, offset, expiry)) return false;
  if (offset > bytes.size() || bytes.size() - offset < 32) return false;
  const bool nonzero_session = std::any_of(bytes.begin() + offset, bytes.begin() + offset + 32,
      [](unsigned char value) { return value != 0; });
  // Retain the manifest's backend id and session identity for the GPU
  // module-audit preflight report (#290). Stale on a later parse failure, but
  // only read after this function returns true.
  g_authorized_backend = backend;
  std::copy(bytes.begin() + offset, bytes.begin() + offset + 32,
            g_authorized_session_identity.begin());
  offset += 32;
  const uint64_t now = static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::milliseconds>(
      std::chrono::system_clock::now().time_since_epoch()).count());
  if (!nonzero_session || expiry <= now || !read_manifest_le(bytes, offset, count) ||
      count == 0 || count > 128)
    return false;

  std::set<std::wstring> paths;
  std::set<std::wstring> basenames;
  for (uint32_t index = 0; index < count; ++index) {
    uint32_t path_units{};
    if (!read_manifest_le(bytes, offset, path_units) || path_units == 0 || path_units > 32767 ||
        offset > bytes.size() || bytes.size() - offset < static_cast<std::size_t>(path_units) * 2)
      return false;
    std::wstring path_text;
    path_text.reserve(path_units);
    for (uint32_t unit = 0; unit < path_units; ++unit) {
      const uint16_t value = static_cast<uint16_t>(bytes[offset]) |
          (static_cast<uint16_t>(bytes[offset + 1]) << 8);
      if (value == 0) return false;
      path_text.push_back(static_cast<wchar_t>(value));
      offset += 2;
    }
    const std::filesystem::path requested(path_text);
    std::filesystem::path canonical;
    std::wstring requested_text = requested.wstring();
    if (requested_text.rfind(L"\\\\?\\", 0) == 0) requested_text.erase(0, 4);
    const std::filesystem::path normalized_requested(requested_text);
    if (!requested.is_absolute() || !normalized_requested.is_absolute() ||
        !canonical_path(requested, canonical) ||
        !same_path(normalized_requested.lexically_normal(), canonical)) return false;
    const std::wstring key = lowercase(canonical.wstring());
    const std::wstring basename = lowercase(canonical.filename().wstring());
    uint64_t declared_size{};
    if (!paths.insert(key).second || basename.empty() || !basenames.insert(basename).second ||
        !read_manifest_le(bytes, offset, declared_size) ||
        declared_size == 0 || offset > bytes.size() || bytes.size() - offset < 32) return false;
    std::string declared_hash;
    declared_hash.reserve(64);
    constexpr char hex[] = "0123456789abcdef";
    for (std::size_t byte = 0; byte < 32; ++byte) {
      declared_hash.push_back(hex[bytes[offset + byte] >> 4]);
      declared_hash.push_back(hex[bytes[offset + byte] & 0x0f]);
    }
    offset += 32;
    const uint64_t actual_size = std::filesystem::file_size(canonical, error);
    std::string actual_hash;
    if (error || actual_size != declared_size || !g_file_sha256(canonical, actual_hash) ||
        actual_hash != declared_hash) return false;
    g_authorized_runtime_modules.push_back({canonical, declared_size, declared_hash});
  }
  return offset == bytes.size();
}

bool has_prefixed_basename(const std::filesystem::path& path,
                           const wchar_t* prefix) {
  if (!prefix) return false;
  return lowercase(path.filename().wstring()).rfind(lowercase(prefix), 0) == 0;
}

ModuleAuditReport& module_audit_report() noexcept {
  return g_module_audit;
}

LoadedModuleProvenance classify_loaded_module_provenance(
    void* module_value) noexcept {
  HMODULE module = static_cast<HMODULE>(module_value);
  if (!module) return LoadedModuleProvenance::unrecognized;
  try {
    std::array<wchar_t, 32768> module_buffer{};
    const DWORD module_length = GetModuleFileNameExW(
        GetCurrentProcess(), module, module_buffer.data(),
        static_cast<DWORD>(module_buffer.size()));
    std::filesystem::path module_path;
    if (module_length == 0 || module_length >= module_buffer.size() ||
        !canonical_path(module_buffer.data(), module_path))
      return LoadedModuleProvenance::unrecognized;

    std::array<wchar_t, MAX_PATH> system_buffer{};
    std::array<wchar_t, 32768> windows_buffer{};
    const UINT system_length = GetSystemDirectoryW(
        system_buffer.data(), static_cast<UINT>(system_buffer.size()));
    const UINT windows_length = GetWindowsDirectoryW(
        windows_buffer.data(), static_cast<UINT>(windows_buffer.size()));
    std::filesystem::path system32, windows_root, winsxs_root,
        driverstore_root;
    if (system_length != 0 && system_length < system_buffer.size() &&
        windows_length != 0 && windows_length < windows_buffer.size() &&
        canonical_path(system_buffer.data(), system32) &&
        canonical_path(windows_buffer.data(), windows_root) &&
        canonical_path(windows_root / L"WinSxS", winsxs_root)) {
      const std::filesystem::path driverstore_candidate =
          system32 / L"DriverStore" / L"FileRepository";
      if (!canonical_path(driverstore_candidate, driverstore_root))
        driverstore_root.clear();
      if (same_path(module_path.parent_path(), system32) ||
          is_winsxs_module(module_path, winsxs_root) ||
          is_driverstore_module(module_path, driverstore_root))
        return LoadedModuleProvenance::system;
    }

    if (!module_audit_observing() || g_module_audit.plugin_path.empty())
      return LoadedModuleProvenance::unrecognized;
    std::filesystem::path plugin_root;
    const bool in_plugin_root =
        canonical_path(g_module_audit.plugin_path.parent_path(), plugin_root) &&
        same_path(module_path.parent_path(), plugin_root);
    const bool in_search_root = std::any_of(
        g_audit_search_roots.begin(), g_audit_search_roots.end(),
        [&](const std::filesystem::path& root) {
          return same_path(module_path.parent_path(), root);
        });
    if (!in_plugin_root && !in_search_root)
      return LoadedModuleProvenance::unrecognized;
    const std::string basename = audit_basename(module_path);
    const auto observed = [&](const std::vector<std::string>& modules) {
      return std::find(modules.begin(), modules.end(), basename) !=
             modules.end();
    };
    if (observed(g_module_audit.post_load.plugin) ||
        observed(g_module_audit.observed_union.plugin))
      return LoadedModuleProvenance::sealed;
  } catch (...) {
    return LoadedModuleProvenance::unrecognized;
  }
  return LoadedModuleProvenance::unrecognized;
}

ModuleAuditSnapshot capture_module_audit() {
  if (!module_audit_observing()) return {};
  ModuleAuditSnapshot snapshot = audit_loaded_modules(g_module_audit.plugin_path);
  accumulate_module_audit(snapshot);
  return snapshot;
}

void capture_module_audit_phase() {
  if (!module_audit_observing()) return;
  accumulate_module_audit(audit_loaded_modules(g_module_audit.plugin_path));
}

bool module_audit_passed() {
  return !g_module_audit.required ||
      g_module_audit.observed_union.status == "passed";
}

std::string module_audit_json() {
  std::ostringstream output;
  // A recorded (in-place, #751) audit serializes the same passed/failed
  // judgement as a required one; only the enforcement differs, and that
  // lives in the lifecycle paths, not here.
  output << "{\"schema\":1,\"status\":\""
         << (module_audit_observing()
                 ? (g_module_audit.observed_union.status == "passed" ? "passed"
                                                                     : "failed")
                 : "not_required")
         << "\",\"post_load\":" << module_audit_snapshot_json(g_module_audit.post_load)
         << ",\"pre_unload\":" << module_audit_snapshot_json(g_module_audit.pre_unload)
         << ",\"observed_union\":" << module_audit_snapshot_json(g_module_audit.observed_union);
  // Cluster sessions append one epoch per swap (design §5). The key stays
  // absent on every non-cluster path so the one-shot validator's exact-key
  // contract is untouched.
  if (!g_module_audit.epochs.empty()) {
    output << ",\"epochs\":[";
    for (std::size_t index = 0; index < g_module_audit.epochs.size(); ++index) {
      if (index) output << ',';
      const ModuleAuditEpoch& epoch = g_module_audit.epochs[index];
      output << "{\"plugin_index\":" << epoch.plugin_index
             << ",\"pre_unload\":" << module_audit_snapshot_json(epoch.pre_unload)
             << ",\"post_load\":" << module_audit_snapshot_json(epoch.post_load)
             << '}';
    }
    output << ']';
  }
  output << ",\"phase_count\":" << g_module_audit.phase_count
         << ",\"unknown_count\":" << g_module_audit.observed_union.unknown_count
         << '}';
  return output.str();
}

std::string module_audit_failure_json() {
  if (!g_module_audit.required || module_audit_passed()) return {};
  const ModuleAuditSnapshot& snapshot = g_module_audit.observed_union;
  std::filesystem::path plugin_root;
  const bool plugin_root_available =
      canonical_path(g_module_audit.plugin_path.parent_path(), plugin_root);
  auto string_value = [](std::ostringstream& output, const std::string& value) {
    output << '"';
    for (char ch : value) {
      if (ch == '"' || ch == '\\') output << '\\';
      output << ch;
    }
    output << '"';
  };
  const std::size_t rejection_count =
      (std::min)(snapshot.unknown_keys.size(), kMaxAuditFailureRejections);
  const uint32_t attributed_count = static_cast<uint32_t>(
      (std::min)(snapshot.unknown_keys.size(),
                 static_cast<std::size_t>(snapshot.unknown_count)));
  std::ostringstream output;
  output << "{\"status\":\"failed\",\"reason\":\""
         << (snapshot.unknown_keys.empty()
                 ? "module_enumeration_or_path_resolution_failed"
                 : "loaded_module_policy_rejection")
         << "\",\"unknown_count\":" << snapshot.unknown_count
         << ",\"unattributed_count\":"
         << (snapshot.unknown_count - attributed_count)
         << ",\"rejections\":[";
  for (std::size_t index = 0; index < rejection_count; ++index) {
    if (index) output << ',';
    const std::filesystem::path path(snapshot.unknown_keys[index]);
    const std::string basename = audit_basename(path);
    const bool sealed_root =
        plugin_root_available && same_path(path.parent_path(), plugin_root);
    output << "{\"basename\":";
    string_value(output, basename);
    output << ",\"canonical_path_token\":";
    const std::string token = path_token(path);
    if (token.empty()) output << "null";
    else string_value(output, token);
    output << ",\"path_class\":\""
           << (sealed_root ? "sealed_root" : "external")
           << "\",\"reason\":\""
           << (sealed_root ? "undeclared_sealed_module"
                           : "outside_allowed_roots_or_unapproved_policy")
           << "\"}";
  }
  output << "],\"rejections_truncated\":"
         << (snapshot.unknown_keys.size() > rejection_count ? "true" : "false")
         << '}';
  return output.str();
}

uint32_t authorized_runtime_backend() noexcept { return g_authorized_backend; }

std::string gpu_module_report_json() {
  constexpr char hex[] = "0123456789abcdef";
  std::string session_hex;
  session_hex.reserve(64);
  for (unsigned char byte : g_authorized_session_identity) {
    session_hex.push_back(hex[byte >> 4]);
    session_hex.push_back(hex[byte & 0x0f]);
  }
  const char* backend = g_authorized_backend == 1   ? "cuda"
                        : g_authorized_backend == 2 ? "opencl"
                        : g_authorized_backend == 3 ? "directx"
                                                    : "";

  // Single pass over the loaded modules: an authorized runtime module is
  // reported only when it actually loaded into this process. Each module is
  // matched to its authorized entry and emitted from that entry's disk-verified
  // identity, so the report never serializes a raw path (only the hashed
  // path_token, the basename, and the size the broker re-authenticates).
  //
  // Fails closed (returns an empty string) when the module enumeration cannot be
  // trusted (query failure, or a > kMaxAuditedModules overflow that would drop
  // modules) or when none of the authorized modules actually loaded. The audit
  // path treats the same overflow as a failure, and an empty `modules` array
  // would authenticate vacuously downstream, so both must fail here instead of
  // emitting a report that proves nothing loaded.
  std::array<HMODULE, kMaxAuditedModules> modules{};
  DWORD needed = 0;
  if (!EnumProcessModulesEx(GetCurrentProcess(), modules.data(),
          static_cast<DWORD>(sizeof(modules)), &needed, LIST_MODULES_ALL) ||
      needed == 0 || needed > sizeof(modules) || needed % sizeof(HMODULE) != 0)
    return {};
  const std::size_t count = needed / sizeof(HMODULE);
  std::ostringstream entries;
  bool first = true;
  for (std::size_t index = 0; index < count; ++index) {
    std::array<wchar_t, 32768> buffer{};
    const DWORD length = GetModuleFileNameExW(GetCurrentProcess(), modules[index],
        buffer.data(), static_cast<DWORD>(buffer.size()));
    std::filesystem::path loaded_module;
    if (length == 0 || length >= buffer.size() ||
        !canonical_path(buffer.data(), loaded_module))
      continue;
    const auto found = std::find_if(g_authorized_runtime_modules.begin(),
        g_authorized_runtime_modules.end(),
        [&](const AuthorizedRuntimeModule& entry) {
          return same_path(loaded_module, entry.path);
        });
    if (found == g_authorized_runtime_modules.end()) continue;
    if (!first) entries << ',';
    first = false;
    entries << "{\"classification\":\"policy\",\"basename\":\""
            << audit_basename(found->path) << "\",\"path_token\":\""
            << path_token(found->path) << "\",\"sha256\":\"" << found->sha256
            << "\",\"size\":" << found->size << "}";
  }
  if (first) return {};  // no authorized module loaded -> nothing to authenticate
  std::ostringstream output;
  output << "{\"session_identity\":\"" << session_hex << "\",\"backend\":\""
         << backend << "\",\"modules\":[" << entries.str() << "]}";
  return output.str();
}

}  // namespace aexcompat::worker_runtime
