#pragma once

// In-place cluster session manifest (`cluster-manifest-v2`, #751/#816).

#include <windows.h>

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime {
class WorkerSession;
}

namespace aexcompat::worker_runtime::cluster {

using FileSha256 = bool (*)(const std::filesystem::path&, std::string&);

// Session-wide caps (design §2.1): exceeding any of them fails the load
// closed and the broker falls back to the per-plugin path.
inline constexpr std::size_t kMaxPlugins = 256;
inline constexpr uint32_t kMaxModuleBound = 4096;
inline constexpr uint64_t kMaxManifestBytes = 4 * 1024 * 1024;
// A plug-in payload rides the same encoding and bound as the launch argv
// payload (design §2.1): ASCII printables only.
inline constexpr std::size_t kMaxPayloadBytes = 16384;
// In-place manifests (issue #751, `cluster-manifest-v2`): the bounded count
// of dependency search directories, matching the broker's and the
// `--dependency-dirs-v1` re-validation.
inline constexpr std::size_t kMaxSearchDirs = 16;

struct PluginEntry {
  std::string basename;
  std::string sha256;  // 64 hex, as written by the broker
  std::string payload;
  bool has_payload{};
  // The plug-in's real absolute path.
  std::filesystem::path path;
};

struct Manifest {
  std::vector<PluginEntry> plugins;
  uint32_t module_bound{};
  std::filesystem::path manifest_path;
  // In-place mode (issue #751, `cluster-manifest-v2`): plug-ins load from
  // their real `path` entries, the dependency closure resolves through
  // `search_dirs` (admitted into USER_DIRS at session start), there are no
  // pinned dependencies, and the module audit is recorded, never enforced.
  std::vector<std::filesystem::path> search_dirs;
};

// Loads and strictly validates a `cluster-manifest-v2` document.
bool load_manifest(const std::filesystem::path& path, Manifest& result);

// Launch cross-check for render sessions: full path and SHA-256 must name
// plugins[0] exactly.
bool matches_launch_plugin(const Manifest& manifest,
                           const std::filesystem::path& plugin_path,
                           const std::string& plugin_sha256);

// Resolves plugins[index] to its declared real path.
std::filesystem::path plugin_path(const Manifest& manifest, std::size_t index);

// Case-insensitive compare of two 64-hex digests.
bool hash_equals(const std::string& actual, const std::string& declared);

// In-place manifests (issue #751): admits `search_dirs` plus every plug-in's
// parent directory (deduplicated, bounded at 64 like the broker validation)
// into the process-wide USER_DIRS set. Cookies stay for the process lifetime
// (deferred release, issue #474). Returns 0 on success, 3 on a shape
// violation (config rejection), 11 when the loader refuses a directory.
int admit_in_place_manifest_dirs(const Manifest& manifest);

// The same union, as the recorded module audit's classification roots.
std::vector<std::filesystem::path> in_place_audit_roots(const Manifest& manifest);

// Strips the Win32 device prefix (`\\?\`, or `\\?\UNC\` -> `\\`) from a path.
// Broker canonical paths may arrive in verbatim form, which MSVC's
// std::filesystem::canonical rejects outright.
std::filesystem::path normalize_verbatim(const std::filesystem::path& path);

}  // namespace aexcompat::worker_runtime::cluster
