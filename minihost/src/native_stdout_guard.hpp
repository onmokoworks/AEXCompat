#pragma once

namespace aexcompat::worker_runtime {

bool redirect_native_stdout();
void restore_native_stdout();

}  // namespace aexcompat::worker_runtime
