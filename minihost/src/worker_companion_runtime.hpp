#pragma once

#include "worker_companion_manifest.hpp"

#include <windows.h>

#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::companions {

// True while initialized companion providers remain resident. Their callbacks
// and death hooks have the same AEGP host-suite capability as initialization.
bool host_services_active() noexcept;

using FileSha256 = bool (*)(const std::filesystem::path&, std::string&);

class Runtime final {
 public:
  Runtime() = default;
  ~Runtime();
  Runtime(const Runtime&) = delete;
  Runtime& operator=(const Runtime&) = delete;

  bool initialize(const Manifest& manifest, void* basic_suite,
                  FileSha256 file_sha256) noexcept;
  bool shutdown() noexcept;
  bool active() const noexcept { return !modules_.empty(); }

 private:
  struct Loaded {
    HMODULE module{};
    int32_t plugin_id{};
    void* global_refcon{};
  };
  std::vector<Loaded> modules_;
  bool shutdown_attempted_{};
  bool host_services_enabled_{};
};

}  // namespace aexcompat::worker_runtime::companions
