#pragma once

// Cluster session manifest (`cluster-manifest-v1`, issue #405,
// docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md §2). One cluster session loads
// several plug-ins that share a single dependency closure into one worker
// process; the manifest is the launch-time trust decision that names, in
// order, every plug-in the worker may ever load (basename + SHA-256, never a
// path the worker resolves) plus the whole shared closure, and declares the
// module bound the module audit is validated against (§5). The broker writes
// it into the sealed root and hands the absolute path over argv
// (`--cluster-manifest-v1 <path>`), the same authenticated-transport pattern
// as `--aux-manifest-v1` / `--parameter-animation-v1`. A swap or inspect
// message then carries only an index into this pre-authenticated list.

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
  // In-place manifests (issue #751): the plug-in's real absolute path; empty
  // on sealed (`cluster-manifest-v1`) manifests, where the path is
  // `sealed_root / basename`.
  std::filesystem::path path;
};

struct DependencyEntry {
  std::string basename;
  std::string sha256;
  uint64_t size{};
};

struct Manifest {
  std::vector<PluginEntry> plugins;
  std::vector<DependencyEntry> dependencies;
  uint32_t module_bound{};
  // Canonical manifest path and its parent. On a sealed manifest the parent
  // is the sealed root (the aux-manifest sibling convention), so every
  // plug-in and dependency path is `sealed_root / basename` and never
  // escapes it. On an in-place manifest (issue #751) the parent is the
  // broker-owned transport directory and `sealed_root` stays empty.
  std::filesystem::path manifest_path;
  std::filesystem::path sealed_root;
  // In-place mode (issue #751, `cluster-manifest-v2`): plug-ins load from
  // their real `path` entries, the dependency closure resolves through
  // `search_dirs` (admitted into USER_DIRS at session start), there are no
  // pinned dependencies, and the module audit is recorded, never enforced.
  bool in_place{};
  std::vector<std::filesystem::path> search_dirs;
};

// Loads and strictly validates a `cluster-manifest-v1` document: exact-key
// JSON, the §2.1 caps, the Windows-safe basename rules shared with the broker
// `session_dependency_manifest` validator, 64-hex SHA-256 fields, payload
// encoding, case-insensitive basename uniqueness across both entry sets, and
// the `aexcompat-sealed-` prefix on the manifest's parent directory.
bool load_manifest(const std::filesystem::path& path, Manifest& result);

// Launch cross-check for render sessions (design §2.2): the argv plug-in
// path's basename and SHA-256 must name plugins[0] exactly (basenames compare
// case-insensitively, digests are 64-hex text).
bool matches_launch_plugin(const Manifest& manifest,
                           const std::filesystem::path& plugin_path,
                           const std::string& plugin_sha256);

// resolves plugins[index] / dependencies[index] to sealed_root / basename.
std::filesystem::path plugin_path(const Manifest& manifest, std::size_t index);

// Lowercased basenames of every plug-in and dependency: the declared set the
// cluster module audit validates its `plugin` class against (design §5).
std::vector<std::string> declared_basenames(const Manifest& manifest);

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
// The broker canonicalizes the sealed root with GetFinalPathNameByHandleW, so
// every staged path it hands over argv arrives in verbatim form — which
// MSVC's std::filesystem::canonical rejects outright. Normalizing the prefix
// first keeps the broker's exact-path contract intact while letting the
// worker canonicalize the same bytes the existing admission path loads.
std::filesystem::path normalize_verbatim(const std::filesystem::path& path);

// The pinned closure (design §3): every manifest dependency loaded once with
// the admission LoadLibraryExW flags and held for the whole session so
// FreeLibrary of a swapped plug-in cannot unload shared dependencies. Pins
// release in reverse load order — except in the deferred-release model
// (issue #474), where nothing is released mid-process and every module
// unloads in one loader-ordered pass at process exit.
class ClosurePins {
 public:
  ClosurePins() = default;
  ClosurePins(const ClosurePins&) = delete;
  ClosurePins& operator=(const ClosurePins&) = delete;
  ~ClosurePins();

  // On failure every already-pinned module is released in reverse order.
  bool pin(const Manifest& manifest, FileSha256 hash_file);
  void release() noexcept;
  // Deferred release (issue #474): the destructor keeps the pins mapped for
  // the process lifetime instead of releasing them.
  void suppress_release_on_destroy() noexcept { release_on_destroy_ = false; }
  std::size_t count() const noexcept { return pins_.size(); }

 private:
  std::vector<HMODULE> pins_;
  bool release_on_destroy_{true};
};

}  // namespace aexcompat::worker_runtime::cluster
