#pragma once

#include "aex_string_table.hpp"

#include <windows.h>

#include <functional>
#include <utility>

namespace aexcompat::worker_runtime::active_plugin {

extern thread_local const aex_strings::StringTable* string_table;
extern thread_local HMODULE effect_module;

class Scope {
 public:
  Scope(const aex_strings::StringTable* next_string_table,
        HMODULE next_effect_module) noexcept
      : previous_string_table_(string_table),
        previous_effect_module_(effect_module) {
    string_table = next_string_table;
    effect_module = next_effect_module;
  }

  Scope(const Scope&) = delete;
  Scope& operator=(const Scope&) = delete;

  ~Scope() {
    string_table = previous_string_table_;
    effect_module = previous_effect_module_;
  }

 private:
  const aex_strings::StringTable* previous_string_table_{};
  HMODULE previous_effect_module_{};
};

template <typename Callback>
decltype(auto) with_context(const aex_strings::StringTable* next_string_table,
                            HMODULE next_effect_module,
                            Callback&& callback) {
  Scope scope(next_string_table, next_effect_module);
  return std::invoke(std::forward<Callback>(callback));
}

}  // namespace aexcompat::worker_runtime::active_plugin
