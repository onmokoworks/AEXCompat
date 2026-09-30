#include "worker_active_plugin_context.hpp"

#include <array>
#include <cstdint>
#include <cstdio>
#include <stdexcept>
#include <thread>

namespace active = aexcompat::worker_runtime::active_plugin;

namespace {

HMODULE fake_module(std::uintptr_t value) {
  return reinterpret_cast<HMODULE>(value);
}

}  // namespace

int main() {
  aexcompat::aex_strings::StringTable inherited_table;
  aexcompat::aex_strings::StringTable previous_tables[2];
  const HMODULE inherited_module = fake_module(0x1000);
  const std::array<HMODULE, 2> previous_modules{
      fake_module(0x2000), fake_module(0x3000)};
  active::string_table = &inherited_table;
  active::effect_module = inherited_module;

  std::array<bool, 2> observed_expected{};
  std::array<bool, 2> restored_previous{};
  bool exception_observed = false;
  std::array<std::thread, 2> threads;
  for (std::size_t index = 0; index < threads.size(); ++index) {
    threads[index] = std::thread([&, index]() {
      active::string_table = &previous_tables[index];
      active::effect_module = previous_modules[index];
      try {
        active::with_context(&inherited_table, inherited_module, [&]() {
          observed_expected[index] =
              active::string_table == &inherited_table &&
              active::effect_module == inherited_module;
          if (index == 1) throw std::runtime_error("restore on exception");
        });
      } catch (const std::runtime_error&) {
        exception_observed = index == 1;
      }
      restored_previous[index] =
          active::string_table == &previous_tables[index] &&
          active::effect_module == previous_modules[index];
    });
  }
  for (auto& thread : threads) thread.join();

  bool null_observed = false;
  bool null_restored = false;
  std::thread null_thread([&]() {
    active::string_table = &previous_tables[0];
    active::effect_module = previous_modules[0];
    active::with_context(nullptr, nullptr, [&]() {
      null_observed = active::string_table == nullptr &&
          active::effect_module == nullptr;
    });
    null_restored = active::string_table == &previous_tables[0] &&
        active::effect_module == previous_modules[0];
  });
  null_thread.join();

  const bool caller_unchanged = active::string_table == &inherited_table &&
      active::effect_module == inherited_module;
  const bool passed = observed_expected[0] && observed_expected[1] &&
      restored_previous[0] && restored_previous[1] && null_observed &&
      null_restored && exception_observed && caller_unchanged;
  if (passed) {
    std::puts("{\"active_plugin_context_selftest\":\"passed\"}");
  }
  return passed ? 0 : 1;
}
