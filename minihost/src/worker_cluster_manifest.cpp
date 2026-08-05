#include "worker_cluster_manifest.hpp"

#include "runtime_module_audit.hpp"
#include "strict_json.hpp"

#include <algorithm>
#include <array>
#include <cstring>
#include <fstream>
#include <set>
#include <system_error>

namespace aexcompat::worker_runtime::cluster {
namespace {

using aexcompat::strict_json::JsonValue;
using aexcompat::strict_json::StrictJsonParser;
using aexcompat::strict_json::json_exact_keys;
using aexcompat::strict_json::json_member;
using aexcompat::strict_json::json_string;
using aexcompat::strict_json::json_u64;

std::wstring lowercase(std::wstring value) {
  std::transform(value.begin(), value.end(), value.begin(), towlower);
  return value;
}

std::string lowercase_ascii(std::string value) {
  for (char& ch : value)
    if (ch >= 'A' && ch <= 'Z') ch = static_cast<char>(ch - 'A' + 'a');
  return value;
}

// Windows-safe basename rules, mirroring the broker
// `session_dependency_manifest::validate_windows_basename` exactly: a single
// normal path component with no separators, drive letters, control
// characters, trailing dots/spaces, or reserved device names.
bool windows_safe_basename(const std::string& name) {
  if (name.empty() || name == "." || name == "..") return false;
  for (const unsigned char ch : name)
    if (ch < 0x20 || ch == 0x7f || ch == '/' || ch == '\\' || ch == ':')
      return false;
  if (name.back() == '.' || name.back() == ' ') return false;
  std::string stem = name.substr(0, name.find('.'));
  while (!stem.empty() && (stem.back() == '.' || stem.back() == ' '))
    stem.pop_back();
  stem = lowercase_ascii(stem);
  if (stem == "con" || stem == "prn" || stem == "aux" || stem == "nul")
    return false;
  if (stem.size() == 4 &&
      (stem.rfind("com", 0) == 0 || stem.rfind("lpt", 0) == 0) &&
      stem[3] >= '1' && stem[3] <= '9')
    return false;
  return true;
}

bool valid_sha256(const std::string& value) {
  if (value.size() != 64) return false;
  for (const unsigned char ch : value) {
    const bool digit = ch >= '0' && ch <= '9';
    const bool lower = ch >= 'a' && ch <= 'f';
    const bool upper = ch >= 'A' && ch <= 'F';
    if (!digit && !lower && !upper) return false;
  }
  return true;
}

bool canonical_of(const std::filesystem::path& path,
                  std::filesystem::path& result) {
  std::error_code error;
  const std::filesystem::path canonical = std::filesystem::canonical(path, error);
  if (error || !canonical.is_absolute()) return false;
  result = canonical;
  return true;
}

bool same_path(const std::filesystem::path& left,
               const std::filesystem::path& right) {
  return lowercase(left.wstring()) == lowercase(right.wstring());
}

// Verifies that `path` resolves to a regular file directly inside
// `sealed_root` (never through a parent escape) and authenticates its size
// and SHA-256 before the bytes are allowed to execute.
bool authenticate_file(const std::filesystem::path& path,
                       const std::filesystem::path& sealed_root,
                       const std::string& declared_sha256, uint64_t declared_size,
                       FileSha256 hash_file) {
  std::filesystem::path canonical;
  if (!canonical_of(path, canonical) || !same_path(canonical.parent_path(), sealed_root))
    return false;
  std::error_code error;
  if (!std::filesystem::is_regular_file(canonical, error) || error) return false;
  const uint64_t size = std::filesystem::file_size(canonical, error);
  if (error || size != declared_size) return false;
  std::string digest;
  return hash_file && hash_file(canonical, digest) &&
      hash_equals(digest, declared_sha256);
}

}  // namespace

bool hash_equals(const std::string& actual, const std::string& declared) {
  return valid_sha256(actual) && valid_sha256(declared) &&
      lowercase_ascii(actual) == lowercase_ascii(declared);
}

std::filesystem::path normalize_verbatim(const std::filesystem::path& path) {
  const std::wstring text = path.wstring();
  if (text.rfind(L"\\\\?\\UNC\\", 0) == 0)
    return std::filesystem::path(L"\\\\" + text.substr(8));
  if (text.rfind(L"\\\\?\\", 0) == 0)
    return std::filesystem::path(text.substr(4));
  return path;
}

namespace {

// The in-place manifest (`cluster-manifest-v2`, issue #751) names plug-ins by
// their real absolute path and carries the dependency search directories
// instead of a pinned closure. Paths arrive de-verbatimed from the broker.
bool parse_in_place_manifest(const JsonValue::Object& object, Manifest& parsed) {
  uint64_t module_bound = 0;
  if (!json_exact_keys(object, {"schema", "plugins", "search_dirs", "module_bound"}) ||
      !json_u64(object, "module_bound", module_bound) || module_bound == 0 ||
      module_bound > kMaxModuleBound)
    return false;
  parsed.module_bound = static_cast<uint32_t>(module_bound);
  parsed.in_place = true;
  const auto* plugins_value = json_member(object, "plugins");
  const auto* dirs_value = json_member(object, "search_dirs");
  if (!plugins_value || !std::holds_alternative<JsonValue::Array>(plugins_value->value) ||
      !dirs_value || !std::holds_alternative<JsonValue::Array>(dirs_value->value))
    return false;
  const auto& plugins = std::get<JsonValue::Array>(plugins_value->value);
  const auto& dirs = std::get<JsonValue::Array>(dirs_value->value);
  if (plugins.empty() || plugins.size() > kMaxPlugins || dirs.empty() ||
      dirs.size() > kMaxSearchDirs)
    return false;
  std::set<std::wstring> plugin_paths;
  for (const auto& plugin_value : plugins) {
    if (!std::holds_alternative<JsonValue::Object>(plugin_value.value)) return false;
    const auto& plugin_object = std::get<JsonValue::Object>(plugin_value.value);
    PluginEntry entry;
    const bool with_payload =
        json_exact_keys(plugin_object, {"path", "sha256", "payload"});
    if (!with_payload && !json_exact_keys(plugin_object, {"path", "sha256"}))
      return false;
    std::string path_text;
    if (!json_string(plugin_object, "path", path_text) || path_text.empty() ||
        !json_string(plugin_object, "sha256", entry.sha256) ||
        !valid_sha256(entry.sha256))
      return false;
    entry.path = std::filesystem::u8path(path_text);
    // Real paths, not sealed-root-relative names: absolute, with a
    // Windows-safe basename, unique across the cluster case-insensitively.
    // The basename is cut from the UTF-8 text itself: `path::string()` would
    // narrow through the ACP and can throw on a name the ACP cannot
    // represent, which the broker's UTF-8 validation legitimately admits.
    const std::size_t separator = path_text.find_last_of("/\\");
    entry.basename = separator == std::string::npos
                         ? path_text
                         : path_text.substr(separator + 1);
    if (!entry.path.is_absolute() || !windows_safe_basename(entry.basename) ||
        !plugin_paths.insert(lowercase(entry.path.wstring())).second)
      return false;
    if (with_payload) {
      if (!json_string(plugin_object, "payload", entry.payload) ||
          entry.payload.size() > kMaxPayloadBytes)
        return false;
      for (const unsigned char ch : entry.payload)
        if (ch < 0x20 || ch > 0x7e) return false;
      entry.has_payload = true;
    }
    parsed.plugins.push_back(std::move(entry));
  }
  std::set<std::wstring> seen_dirs;
  for (const auto& dir_value : dirs) {
    if (!std::holds_alternative<std::string>(dir_value.value)) return false;
    const std::filesystem::path dir =
        std::filesystem::u8path(std::get<std::string>(dir_value.value));
    if (dir.empty() || !dir.is_absolute() ||
        !seen_dirs.insert(lowercase(dir.wstring())).second)
      return false;
    parsed.search_dirs.push_back(dir);
  }
  return true;
}

}  // namespace

bool load_manifest(const std::filesystem::path& path, Manifest& result) {
  if (path.empty() || !path.is_absolute()) return false;
  // The broker hands over the verbatim (\\?\-prefixed) canonical form of the
  // staged path; normalize before canonicalizing (see normalize_verbatim).
  const std::filesystem::path argument = normalize_verbatim(path);
  std::filesystem::path canonical;
  if (!canonical_of(argument, canonical)) return false;
  // Same absolute+canonical identity rule as load_aux_manifest: the broker
  // hands over the exact path it wrote, with no symlink indirection.
  std::error_code error;
  const std::filesystem::path absolute = std::filesystem::absolute(argument, error);
  if (error || absolute.lexically_normal() != canonical) return false;
  const uint64_t size = std::filesystem::file_size(canonical, error);
  if (error || size == 0 || size > kMaxManifestBytes) return false;
  std::ifstream input(canonical, std::ios::binary);
  if (!input) return false;
  std::string text((std::istreambuf_iterator<char>(input)), {});
  JsonValue root;
  if (input.bad() || !StrictJsonParser(std::move(text)).parse(root) ||
      !std::holds_alternative<JsonValue::Object>(root.value))
    return false;
  const auto& object = std::get<JsonValue::Object>(root.value);
  std::string schema;
  {
    // Schema dispatch: v2 (in-place, issue #751) parses its own shape and
    // needs no sealed root; v1 (sealed) continues below.
    std::string probe;
    if (json_string(object, "schema", probe) && probe == "cluster-manifest-v2") {
      Manifest parsed;
      if (!parse_in_place_manifest(object, parsed)) return false;
      parsed.manifest_path = canonical;
      result = std::move(parsed);
      return true;
    }
  }
  uint64_t module_bound = 0;
  if (!json_exact_keys(object, {"schema", "plugins", "dependencies", "module_bound"}) ||
      !json_string(object, "schema", schema) || schema != "cluster-manifest-v1" ||
      !json_u64(object, "module_bound", module_bound) || module_bound == 0 ||
      module_bound > kMaxModuleBound)
    return false;
  const auto* plugins_value = json_member(object, "plugins");
  const auto* dependencies_value = json_member(object, "dependencies");
  if (!plugins_value || !std::holds_alternative<JsonValue::Array>(plugins_value->value) ||
      !dependencies_value ||
      !std::holds_alternative<JsonValue::Array>(dependencies_value->value))
    return false;
  const auto& plugins = std::get<JsonValue::Array>(plugins_value->value);
  const auto& dependencies = std::get<JsonValue::Array>(dependencies_value->value);
  if (plugins.empty() || plugins.size() > kMaxPlugins ||
      dependencies.size() > kMaxModuleBound)
    return false;

  Manifest parsed;
  parsed.module_bound = static_cast<uint32_t>(module_bound);
  std::set<std::string> basenames;
  for (const auto& plugin_value : plugins) {
    if (!std::holds_alternative<JsonValue::Object>(plugin_value.value)) return false;
    const auto& plugin_object = std::get<JsonValue::Object>(plugin_value.value);
    PluginEntry entry;
    const bool with_payload = json_exact_keys(plugin_object, {"basename", "sha256", "payload"});
    if (!with_payload &&
        !json_exact_keys(plugin_object, {"basename", "sha256"}))
      return false;
    if (!json_string(plugin_object, "basename", entry.basename) ||
        !windows_safe_basename(entry.basename) ||
        !json_string(plugin_object, "sha256", entry.sha256) ||
        !valid_sha256(entry.sha256) ||
        !basenames.insert(lowercase_ascii(entry.basename)).second)
      return false;
    if (with_payload) {
      if (!json_string(plugin_object, "payload", entry.payload) ||
          entry.payload.size() > kMaxPayloadBytes)
        return false;
      for (const unsigned char ch : entry.payload)
        if (ch < 0x20 || ch > 0x7e) return false;
      entry.has_payload = true;
    }
    parsed.plugins.push_back(std::move(entry));
  }
  for (const auto& dependency_value : dependencies) {
    if (!std::holds_alternative<JsonValue::Object>(dependency_value.value))
      return false;
    const auto& dependency_object = std::get<JsonValue::Object>(dependency_value.value);
    DependencyEntry entry;
    if (!json_exact_keys(dependency_object, {"basename", "sha256", "size"}) ||
        !json_string(dependency_object, "basename", entry.basename) ||
        !windows_safe_basename(entry.basename) ||
        !json_string(dependency_object, "sha256", entry.sha256) ||
        !valid_sha256(entry.sha256) ||
        !json_u64(dependency_object, "size", entry.size) || entry.size == 0 ||
        !basenames.insert(lowercase_ascii(entry.basename)).second)
      return false;
    parsed.dependencies.push_back(std::move(entry));
  }

  // The manifest lives directly inside the sealed root so every entry path is
  // `sealed_root / basename`; the root must carry the sealed-staging prefix
  // the module audit and admission already require.
  parsed.manifest_path = canonical;
  parsed.sealed_root = canonical.parent_path();
  if (parsed.sealed_root.empty() ||
      !has_prefixed_basename(parsed.sealed_root, L"aexcompat-sealed-"))
    return false;
  result = std::move(parsed);
  return true;
}

bool matches_launch_plugin(const Manifest& manifest,
                           const std::filesystem::path& plugin_path,
                           const std::string& plugin_sha256) {
  if (manifest.plugins.empty()) return false;
  const PluginEntry& first = manifest.plugins.front();
  if (manifest.in_place) {
    // In-place manifests (issue #751) name plugins[0] by its real path; the
    // launch positional must canonicalize to the same file.
    std::filesystem::path launch_canonical;
    std::filesystem::path declared_canonical;
    return canonical_of(normalize_verbatim(plugin_path), launch_canonical) &&
        canonical_of(first.path, declared_canonical) &&
        same_path(launch_canonical, declared_canonical) &&
        hash_equals(plugin_sha256, first.sha256);
  }
  const std::wstring basename = lowercase(plugin_path.filename().wstring());
  std::wstring declared;
  for (const unsigned char ch : first.basename) declared.push_back(ch);
  return basename == lowercase(declared) &&
      hash_equals(plugin_sha256, first.sha256);
}

bool admit_in_place_manifest_dirs(const Manifest& manifest) {
  // Admit the validated search directories plus every plug-in's own
  // directory into the process-wide USER_DIRS set (issue #751). Cookies stay
  // for the process lifetime (deferred release, issue #474); the broker
  // validated the deduplicated union against the same bound.
  std::set<std::wstring> admitted;
  std::vector<std::filesystem::path> roots = manifest.search_dirs;
  for (const auto& plugin : manifest.plugins)
    roots.push_back(plugin.path.parent_path());
  constexpr std::size_t kMaxAdmittedDirs = 64;
  for (const auto& root : roots) {
    if (root.empty()) return false;
    std::wstring key = lowercase(root.wstring());
    if (!admitted.insert(std::move(key)).second) continue;
    if (admitted.size() > kMaxAdmittedDirs) return false;
    if (!AddDllDirectory(root.c_str())) return false;
  }
  return true;
}

std::vector<std::filesystem::path> in_place_audit_roots(const Manifest& manifest) {
  std::vector<std::filesystem::path> roots = manifest.search_dirs;
  for (const auto& plugin : manifest.plugins)
    roots.push_back(plugin.path.parent_path());
  return roots;
}

std::filesystem::path plugin_path(const Manifest& manifest, std::size_t index) {
  if (index >= manifest.plugins.size()) return {};
  // In-place manifests (issue #751) name the real path; sealed manifests
  // resolve `sealed_root / basename`.
  if (manifest.in_place) return manifest.plugins[index].path;
  return manifest.sealed_root / std::filesystem::u8path(manifest.plugins[index].basename);
}

std::vector<std::string> declared_basenames(const Manifest& manifest) {
  std::vector<std::string> declared;
  declared.reserve(manifest.plugins.size() + manifest.dependencies.size());
  for (const auto& plugin : manifest.plugins)
    declared.push_back(lowercase_ascii(plugin.basename));
  for (const auto& dependency : manifest.dependencies)
    declared.push_back(lowercase_ascii(dependency.basename));
  return declared;
}

ClosurePins::~ClosurePins() {
  if (release_on_destroy_) release();
}

bool ClosurePins::pin(const Manifest& manifest, FileSha256 hash_file) {
  if (!pins_.empty()) return false;
  for (const auto& dependency : manifest.dependencies) {
    const std::filesystem::path path =
        manifest.sealed_root / std::filesystem::u8path(dependency.basename);
    std::filesystem::path canonical;
    // Authenticate the file before its bytes may execute (fail-closed double
    // of the broker staging check), then load with the admission flags.
    if (!canonical_of(path, canonical) ||
        !authenticate_file(canonical, manifest.sealed_root, dependency.sha256,
                           dependency.size, hash_file)) {
      release();
      return false;
    }
    HMODULE module = LoadLibraryExW(canonical.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) {
      release();
      return false;
    }
    // The loaded module must be the authenticated file directly under the
    // sealed root (design §3): a loader redirect to any other directory fails
    // the pin.
    std::array<wchar_t, 32768> loaded_buffer{};
    const DWORD loaded_length = GetModuleFileNameW(
        module, loaded_buffer.data(), static_cast<DWORD>(loaded_buffer.size()));
    std::filesystem::path loaded_path;
    if (loaded_length == 0 || loaded_length >= loaded_buffer.size() ||
        !canonical_of(loaded_buffer.data(), loaded_path) ||
        !same_path(loaded_path, canonical)) {
      FreeLibrary(module);
      release();
      return false;
    }
    pins_.push_back(module);
  }
  return true;
}

void ClosurePins::release() noexcept {
  for (auto it = pins_.rbegin(); it != pins_.rend(); ++it) FreeLibrary(*it);
  pins_.clear();
}

}  // namespace aexcompat::worker_runtime::cluster
