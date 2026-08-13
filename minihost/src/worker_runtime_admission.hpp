#pragma once

#include <windows.h>

#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime {

// The runtime owns admission state; the host supplies only the file identity
// primitive and its stdout isolation boundary. This does not expose plug-in
// bytes or audit paths to the caller.
using RuntimeFileHash = bool(*)(const std::filesystem::path&, std::string&);
using RuntimeStdoutRedirect = bool(*)();
using RuntimeStdoutRestore = void(*)();

struct RuntimeHostHooks {
  RuntimeFileHash hash_file{};
  RuntimeStdoutRedirect redirect_native_stdout{};
  RuntimeStdoutRestore restore_native_stdout{};
};

struct RuntimeAdmissionRequest {
  std::filesystem::path plugin_argument;
  std::string expected_sha256;
  bool authorize_runtime_modules{};
  // AEGP admission is opt-in and is set only by the worker's explicit AEGP
  // invocation modes. PF routes retain the pre-execution AEGP rejection gate.
  bool allow_aegp_plugin{};
  std::filesystem::path authorization_manifest;
  // In-place load mode (issue #751): broker-validated directories admitted
  // into the process-wide USER_DIRS search set so an unstaged plug-in's
  // dependency closure resolves from where it actually lives. Empty on the
  // sealed staging path.
  std::vector<std::filesystem::path> dependency_search_dirs;
};

struct RuntimeContext {
  std::filesystem::path plugin_path;
  HMODULE module{};
  DLL_DIRECTORY_COOKIE sealed_directory_cookie{};
  // Cookies for the in-place dependency search directories (issue #751);
  // removed with the sealed cookie in the same lifecycle order.
  std::vector<DLL_DIRECTORY_COOKIE> search_directory_cookies;
  bool stdout_redirected{};
  RuntimeStdoutRestore restore_native_stdout{};
};

// Releases an admitted context in lifecycle order. This is used by the one
// preflight path that intentionally does not construct WorkerSession.
void release_runtime_context(RuntimeContext& context) noexcept;

// Returns the historical worker exit code on rejection. On success module
// ownership transfers to RuntimeContext.
int admit_runtime(const RuntimeHostHooks& hooks,
                  const RuntimeAdmissionRequest& request,
                  RuntimeContext& context);

// Splits admission at the provider/consumer boundary. Preparation authenticates
// the target, establishes the process DLL-search/stdout context, and transfers
// those resources to RuntimeContext without executing the target module.
// load_runtime_plugin then performs the sole executable admission of that
// target. This lets a host initialize approved companion providers after the
// host context exists but before the consumer PF executes.
int prepare_runtime_environment(const RuntimeHostHooks& hooks,
                                const RuntimeAdmissionRequest& request,
                                RuntimeContext& context);
int load_runtime_plugin(const RuntimeAdmissionRequest& request,
                        RuntimeContext& context);

// Builds the bounded request consumed by admission without loading the
// plug-in. Returns the historical malformed-argument code for non-ASCII
// identity text so callers cannot accidentally widen the security boundary.
int prepare_runtime_request(const wchar_t* plugin_argument,
                            const wchar_t* expected_sha256,
                            bool authorize_runtime_modules,
                            const wchar_t* authorization_manifest,
                            RuntimeAdmissionRequest& request);

// Parses the `--dependency-dirs-v1` value (issue #751): absolute directories
// joined by ';', bounded in count and shape. Returns the historical
// malformed-argument code (2) on any violation so a malformed launch never
// widens the search set silently.
int apply_dependency_search_dirs(const wchar_t* joined,
                                 RuntimeAdmissionRequest& request);

}  // namespace aexcompat::worker_runtime
