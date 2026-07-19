#pragma once

#include <windows.h>

#include <filesystem>
#include <string>

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
  std::filesystem::path authorization_manifest;
};

struct RuntimeContext {
  std::filesystem::path plugin_path;
  HMODULE module{};
  bool stdout_redirected{};
  RuntimeStdoutRestore restore_native_stdout{};
};

// Returns the historical worker exit code on rejection. On success module
// ownership transfers to RuntimeContext.
int admit_runtime(const RuntimeHostHooks& hooks,
                  const RuntimeAdmissionRequest& request,
                  RuntimeContext& context);

// Builds the bounded request consumed by admission without loading the
// plug-in. Returns the historical malformed-argument code for non-ASCII
// identity text so callers cannot accidentally widen the security boundary.
int prepare_runtime_request(const wchar_t* plugin_argument,
                            const wchar_t* expected_sha256,
                            bool authorize_runtime_modules,
                            const wchar_t* authorization_manifest,
                            RuntimeAdmissionRequest& request);

}  // namespace aexcompat::worker_runtime
