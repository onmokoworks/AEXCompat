#pragma once

#include <iosfwd>
#include <sstream>

namespace aexcompat::worker_render_report {

// Owns one complete ordered report before it becomes externally observable.
// Callers append fields in schema order; emit publishes the JSON atomically.
class ReportSnapshot {
 public:
  explicit ReportSnapshot(const std::ios& formatting_source);

  std::ostream& stream() noexcept { return stream_; }
  const std::string json() const { return stream_.str(); }

 private:
  std::ostringstream stream_;
};

void emit(const ReportSnapshot& snapshot, std::ostream& output);

}  // namespace aexcompat::worker_render_report
