#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime {

using FileSha256 = bool(*)(const std::filesystem::path&, std::string&);

struct ModuleAuditSnapshot {
  std::string status{"not_required"};
  std::vector<std::string> worker;
  std::vector<std::string> plugin;
  std::vector<std::string> system32;
  std::vector<std::string> winsxs;
  std::vector<std::string> driverstore;
  std::vector<std::string> policy;
  uint32_t unknown_count{};
  std::vector<std::wstring> unknown_keys;
};

// One cluster-session swap epoch (docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md
// §5): pre_unload is the loaded-module set immediately before plugins[N] is
// freed, post_load the set right after plugins[N+1] finishes loading. The
// final plug-in's pre_unload stays the report's terminal `pre_unload`.
struct ModuleAuditEpoch {
  uint32_t plugin_index{};
  ModuleAuditSnapshot pre_unload;
  ModuleAuditSnapshot post_load;
};

struct ModuleAuditReport {
  bool required{};
  ModuleAuditSnapshot post_load;
  ModuleAuditSnapshot pre_unload;
  ModuleAuditSnapshot observed_union;
  uint32_t phase_count{};
  std::filesystem::path plugin_path;
  std::vector<ModuleAuditEpoch> epochs;
};

enum class LoadedModuleProvenance {
  sealed,
  system,
  unrecognized,
};

void configure_runtime_module_hash(FileSha256 hash) noexcept;
bool parse_runtime_module_authorization(const std::filesystem::path& plugin_path,
                                        const std::filesystem::path& manifest_name);
bool has_prefixed_basename(const std::filesystem::path& path,
                           const wchar_t* prefix);
ModuleAuditReport& module_audit_report() noexcept;
ModuleAuditSnapshot capture_module_audit();
void capture_module_audit_phase();
bool module_audit_passed();
std::string module_audit_json();
// Bounded failure-only diagnostics. Canonical paths stay private; each
// rejection carries only a basename, a canonical path token, and a path class.
std::string module_audit_failure_json();
// Classifies an already loaded HMODULE using the same authenticated sealed-root
// and Windows-owned path policy as the module audit. No path or basename leaves
// this boundary, and an unavailable or ambiguous provenance fails closed.
LoadedModuleProvenance classify_loaded_module_provenance(
    void* module) noexcept;

// Cluster-session audit mode (design §5): replaces the fixed
// kMaxAuditedModules enumeration/accumulation bound with the launch-time
// authenticated `module_bound` (capped at 4096) and narrows the `plugin`
// classification to the manifest's declared basename set. A loaded module
// under the sealed root whose basename is not declared counts as unknown
// (fail-closed). `declared_plugin_basenames` must be lowercased.
void configure_module_audit_cluster(std::size_t module_bound,
                                    std::vector<std::string> declared_plugin_basenames);

// Appends one swap epoch to the report. The snapshots are the values the
// caller captured via capture_module_audit() (which already accumulated them
// into observed_union).
void record_module_audit_epoch(uint32_t plugin_index,
                               ModuleAuditSnapshot pre_unload,
                               ModuleAuditSnapshot post_load);

// The GPU-framework backend id (1=cuda, 2=opencl, 3=directx, 4=opengl) carried
// by the AEXRMA1 manifest that the last successful
// `parse_runtime_module_authorization` accepted; 0 when none is loaded. The GPU
// module-audit preflight maps this to the transport framework code before
// loading the runtime and reports it back (#290).
uint32_t authorized_runtime_backend() noexcept;

// Emits the classified GPU module report the broker authenticates before a
// secure GPU dispatch (#290): the `GpuWorkerModuleReportDto` JSON
// `{session_identity, backend, modules:[{classification:"policy", basename,
// path_token, sha256, size}]}`. Only the AEXRMA1-authorized runtime modules that
// are actually loaded into this process are reported (classification "policy");
// the session_identity and backend echo the manifest the preflight authorized.
// Requires a prior successful `parse_runtime_module_authorization` and that the
// GPU runtime has been loaded (via the transport `begin_backend_context`).
std::string gpu_module_report_json();

}  // namespace aexcompat::worker_runtime
