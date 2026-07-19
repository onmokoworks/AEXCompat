#include "worker_entry_admission.hpp"

#include "trace_writer.hpp"

#include <filesystem>

namespace aexcompat::worker_runtime {

int admit_worker_entry(const RuntimeHostHooks& hooks,
                       const RuntimeAdmissionRequest& request,
                       const char* trace_worker_label,
                       std::unique_ptr<aexcompat::TraceWriter>& trace_writer,
                       RuntimeContext& context) {
  if (!trace_worker_label) return 16;
  trace_writer = std::make_unique<aexcompat::TraceWriter>(
      "minihost", trace_worker_label,
      std::filesystem::path(request.plugin_argument).filename().string());
  if (trace_writer->requested() && !trace_writer->enabled()) return 16;
  return admit_runtime(hooks, request, context);
}

}  // namespace aexcompat::worker_runtime
