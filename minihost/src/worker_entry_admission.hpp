#pragma once

#include "worker_runtime_admission.hpp"

#include <memory>

namespace aexcompat {
class TraceWriter;
}

namespace aexcompat::worker_runtime {

// Owns the entry-side ordering from trace setup through authenticated module
// admission. WorkerSession remains stack-owned by the caller so its historical
// cleanup and early-return behavior are unchanged.
int admit_worker_entry(const RuntimeHostHooks& hooks,
                       const RuntimeAdmissionRequest& request,
                       const char* trace_worker_label,
                       std::unique_ptr<aexcompat::TraceWriter>& trace_writer,
                       RuntimeContext& context);

}  // namespace aexcompat::worker_runtime
