#include <windows.h>

#include <cassert>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <iterator>
#include <string>

#include "runtime_module_audit.hpp"
#include "worker_runtime_admission.hpp"

namespace wr = aexcompat::worker_runtime;

namespace {

HANDLE open_observation_handle(const std::filesystem::path& path) {
  return CreateFileW(path.c_str(), GENERIC_READ,
                     FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                     nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
}

std::filesystem::path module_path(HMODULE module) {
  wchar_t path[32768]{};
  const DWORD length =
      GetModuleFileNameW(module, path, static_cast<DWORD>(std::size(path)));
  assert(length != 0 && length < std::size(path));
  return std::filesystem::path(path);
}

wr::PluginFileObservation observe(const std::filesystem::path& path) {
  HANDLE file = open_observation_handle(path);
  assert(file != INVALID_HANDLE_VALUE);
  wr::PluginFileObservation observation;
  assert(wr::capture_plugin_file_observation(file, observation));
  assert(CloseHandle(file));
  assert(observation.sha256.size() == 64);
  assert(observation.size_bytes > 0);
  return observation;
}

const wr::PluginExecutionImage& only_image() {
  const auto& images = wr::module_audit_report().execution_images;
  assert(images.size() == 1);
  return images.front();
}

void reset_record_only_report() {
  wr::ModuleAuditReport& report = wr::module_audit_report();
  report.required = false;
  report.recorded = false;
  report.execution_images.clear();
  assert(wr::module_audit_passed());
}

void assert_verdict_unchanged() {
  // The observer has no return value that a render path can turn into an exit
  // code, and it must not mutate the historical module-audit verdict either.
  assert(!wr::module_audit_report().required);
  assert(wr::module_audit_passed());
}

}  // namespace

int wmain(int argc, wchar_t** argv) {
  assert(argc == 1 || argc == 2);
  const std::filesystem::path fixture =
      argc == 2 ? std::filesystem::path(argv[1])
                : std::filesystem::path(argv[0]).parent_path() /
                      L"runtime_module_path_cache_fixture.dll";
  assert(std::filesystem::is_regular_file(fixture));

  HANDLE selected_file = open_observation_handle(fixture);
  assert(selected_file != INVALID_HANDLE_VALUE);
  wr::PluginFileObservation selected;
  assert(wr::capture_plugin_file_observation(selected_file, selected));

  HMODULE fixture_module = LoadLibraryW(fixture.c_str());
  assert(fixture_module);

  reset_record_only_report();
  wr::record_loaded_plugin_execution_image(
      0, fixture, fixture_module, &selected, selected.sha256,
      selected.size_bytes);
  {
    const wr::PluginExecutionImage& image = only_image();
    assert(image.plugin_index == 0);
    assert(_stricmp(image.basename.c_str(), fixture.filename().string().c_str()) ==
           0);
    assert(image.sha256 == selected.sha256);
    assert(image.size_bytes == selected.size_bytes);
    assert(image.binding_status ==
           "same_file_identity_matches_loaded_module");
    assert(wr::module_audit_json().find(fixture.string()) == std::string::npos);
    assert_verdict_unchanged();
  }

  // Mutation: retain the selected fixture observation, but supply the
  // self-test executable's HMODULE. The diagnostic must report the executable
  // bytes and mismatch state rather than falsely attaching the fixture digest.
  HMODULE wrong_module = GetModuleHandleW(nullptr);
  assert(wrong_module);
  const std::filesystem::path wrong_path = module_path(wrong_module);
  const wr::PluginFileObservation wrong = observe(wrong_path);
  reset_record_only_report();
  wr::record_loaded_plugin_execution_image(
      0, fixture, wrong_module, &selected, selected.sha256,
      selected.size_bytes);
  {
    const wr::PluginExecutionImage& image = only_image();
    assert(_stricmp(image.basename.c_str(),
                    wrong_path.filename().string().c_str()) == 0);
    assert(image.sha256 == wrong.sha256);
    assert(image.sha256 != selected.sha256);
    assert(image.size_bytes == wrong.size_bytes);
    assert(image.binding_status == "loaded_module_identity_mismatch");
    assert_verdict_unchanged();
  }

  // Missing pre-load observation is a diagnostic state, not a load/render
  // failure. The already loaded image can still be hashed through its own
  // resolved handle without claiming a pre/post identity match.
  wr::PluginFileObservation unavailable;
  assert(!wr::capture_plugin_file_observation(INVALID_HANDLE_VALUE,
                                              unavailable));
  reset_record_only_report();
  wr::record_loaded_plugin_execution_image(
      0, fixture, fixture_module, nullptr, {}, 0);
  {
    const wr::PluginExecutionImage& image = only_image();
    assert(image.sha256 == selected.sha256);
    assert(image.size_bytes == selected.size_bytes);
    assert(image.binding_status == "loaded_module_identity_unavailable");
    assert_verdict_unchanged();
  }

  assert(FreeLibrary(fixture_module));
  assert(CloseHandle(selected_file));
  std::cout << "{\"worker_execution_identity_selftest\":\"passed\"}\n";
  return 0;
}
