#include "worker_render_report.hpp"

#include <ostream>

namespace aexcompat::worker_render_report {

ReportSnapshot::ReportSnapshot(const std::ios& formatting_source) {
  stream_.copyfmt(formatting_source);
}

void emit(const ReportSnapshot& snapshot, std::ostream& output) {
  output << snapshot.json();
}

}  // namespace aexcompat::worker_render_report
