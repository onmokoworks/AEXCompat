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
  std::vector<std::string> policy;
  uint32_t unknown_count{};
  std::vector<std::wstring> unknown_keys;
};

struct ModuleAuditReport {
  bool required{};
  ModuleAuditSnapshot post_load;
  ModuleAuditSnapshot pre_unload;
  ModuleAuditSnapshot observed_union;
  uint32_t phase_count{};
  std::filesystem::path plugin_path;
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

}  // namespace aexcompat::worker_runtime
