#include "worker_pf_helper_runtime.hpp"
#include <array>
#include <atomic>
#include <cstddef>

namespace aexcompat::pf_helper {
namespace {
constexpr std::size_t kUiContextCount = 3;
std::atomic<int32_t> g_effect_tool{kExtendedToolMin};
std::array<std::atomic<int32_t>, kUiContextCount> g_ui_tools{};
thread_local int32_t g_ui_context = -1;
thread_local bool g_ui_context_active = false;
using Suite1 = std::array<void*, 1>;
using Suite2 = std::array<void*, 3>;
Suite1 g_suite1{reinterpret_cast<void*>(&get_current_tool)};
Suite2 g_suite2{reinterpret_cast<void*>(&parse_clipboard),
                reinterpret_cast<void*>(&set_current_extended_tool),
                reinterpret_cast<void*>(&get_current_extended_tool)};
static_assert(sizeof(Suite1) == sizeof(void*));
static_assert(sizeof(Suite2) == 3 * sizeof(void*));
std::atomic<int32_t>& current_tool() {
  return g_ui_context >= 0 && g_ui_context < static_cast<int32_t>(g_ui_tools.size())
      ? g_ui_tools[static_cast<std::size_t>(g_ui_context)] : g_effect_tool;
}
}

UiContextScope::UiContextScope(int32_t context)
    : previous_(g_ui_context), previous_active_(g_ui_context_active) {
  g_ui_context = context >= 0 && context < static_cast<int32_t>(kUiContextCount) ? context : -1;
  g_ui_context_active = g_ui_context >= 0;
}
UiContextScope::~UiContextScope() { g_ui_context_active = previous_active_; g_ui_context = previous_; }
int32_t __cdecl parse_clipboard() { return kBadCallbackParam; }
int32_t __cdecl set_current_extended_tool(int32_t tool) {
  if (!g_ui_context_active || g_ui_context < 0 ||
      g_ui_context >= static_cast<int32_t>(g_ui_tools.size()) ||
      tool < kExtendedToolMin || tool > kExtendedToolMax) return kBadCallbackParam;
  current_tool().store(tool, std::memory_order_release); return 0;
}
int32_t __cdecl get_current_extended_tool(int32_t* tool) {
  if (!tool) return kBadCallbackParam;
  *tool = current_tool().load(std::memory_order_acquire); return 0;
}
int32_t __cdecl get_current_tool(int32_t* tool) {
  if (!tool) return kBadCallbackParam; *tool = kToolNone; return 0;
}
void reset() {
  g_effect_tool.store(kExtendedToolMin, std::memory_order_release);
  for (auto& tool : g_ui_tools) tool.store(kExtendedToolMin, std::memory_order_release);
}
void set_context_tool(int32_t context, int32_t tool) {
  if (context >= 0 && context < static_cast<int32_t>(g_ui_tools.size()))
    g_ui_tools[static_cast<std::size_t>(context)].store(tool, std::memory_order_release);
}
void set_effect_tool_for_test(int32_t tool) { g_effect_tool.store(tool, std::memory_order_release); }
int32_t effect_tool_for_test() { return g_effect_tool.load(std::memory_order_acquire); }
void* suite1() { return g_suite1.data(); }
void* suite2() { return g_suite2.data(); }
bool selftest() {
  reset(); int32_t tool = -1;
  if (parse_clipboard() != kBadCallbackParam || get_current_tool(&tool) != 0 || tool != kToolNone ||
      get_current_tool(nullptr) != kBadCallbackParam || set_current_extended_tool(1) != kBadCallbackParam) return false;
  { UiContextScope scope(1); if (set_current_extended_tool(14) != 0 ||
      get_current_extended_tool(&tool) != 0 || tool != 14) return false; }
  return get_current_extended_tool(&tool) == 0 && tool == kExtendedToolMin;
}
}
