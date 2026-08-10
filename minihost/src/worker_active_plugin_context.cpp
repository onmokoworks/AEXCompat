#include "worker_active_plugin_context.hpp"

namespace aexcompat::worker_runtime::active_plugin {

thread_local const aex_strings::StringTable* string_table = nullptr;
thread_local HMODULE effect_module = nullptr;

}  // namespace aexcompat::worker_runtime::active_plugin
