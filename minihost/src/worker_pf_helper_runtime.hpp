#pragma once
#include <cstdint>

namespace aexcompat::pf_helper {
constexpr int32_t kBadCallbackParam = 516;
constexpr int32_t kToolNone = 0;
constexpr int32_t kExtendedToolMin = 0;
constexpr int32_t kExtendedToolMax = 44;

class UiContextScope {
 public:
  explicit UiContextScope(int32_t context);
  ~UiContextScope();
  UiContextScope(const UiContextScope&) = delete;
  UiContextScope& operator=(const UiContextScope&) = delete;
 private:
  int32_t previous_;
  bool previous_active_;
};

int32_t __cdecl parse_clipboard();
int32_t __cdecl set_current_extended_tool(int32_t tool);
int32_t __cdecl get_current_extended_tool(int32_t* tool);
int32_t __cdecl get_current_tool(int32_t* tool);
void reset();
void set_context_tool(int32_t context, int32_t tool);
void set_effect_tool_for_test(int32_t tool);
int32_t effect_tool_for_test();
void* suite1();
void* suite2();
bool selftest();
}
