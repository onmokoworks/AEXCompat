#include "worker_custom_ui_state.hpp"

namespace aexcompat::worker_runtime::custom_ui {
namespace {
State g_state;
}

State& state() { return g_state; }

}  // namespace aexcompat::worker_runtime::custom_ui
