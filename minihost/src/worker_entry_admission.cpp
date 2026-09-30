#include "worker_entry_admission.hpp"

#include "trace_writer.hpp"

#include <filesystem>

namespace aexcompat::worker_runtime {

int admit_worker_entry(const RuntimeHostHooks& hooks,
                       const RuntimeAdmissionRequest& request,
                       const char* trace_worker_label,
                       std::unique_ptr<aexcompat::TraceWriter>& trace_writer,
                       RuntimeContext& context) {
  const int prepare_error = prepare_worker_entry(
      hooks, request, trace_worker_label, trace_writer, context);
  if (prepare_error != 0) return prepare_error;
  const int load_error = load_runtime_plugin(request, context);
  if (load_error != 0) release_runtime_context(context);
  return load_error;
}

int prepare_worker_entry(const RuntimeHostHooks& hooks,
                         const RuntimeAdmissionRequest& request,
                         const char* trace_worker_label,
                         std::unique_ptr<aexcompat::TraceWriter>& trace_writer,
                         RuntimeContext& context) {
  if (!trace_worker_label) return 16;
  trace_writer = std::make_unique<aexcompat::TraceWriter>(
      "minihost", trace_worker_label,
      std::filesystem::path(request.plugin_argument).filename().string());
  if (trace_writer->requested() && !trace_writer->enabled()) return 16;
  return prepare_runtime_environment(hooks, request, context);
}

}  // namespace aexcompat::worker_runtime
