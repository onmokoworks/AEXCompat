#pragma once

#include <cstdint>
#include <filesystem>
#include <fstream>
#include <string>

namespace aexcompat {

class TraceWriter {
 public:
  TraceWriter(std::string host_kind, std::string host_version_label,
              std::string plugin_label);

  bool enabled() const;
  const std::filesystem::path& path() const;
  void session_start();
  void selector_dispatch(const std::string& selector);
  void suite_acquire(const std::string& name, std::int64_t version, bool granted);
  void suite_release(const std::string& name, std::int64_t version, bool granted);
  void world_descriptor(std::int64_t width, std::int64_t height,
                        std::int64_t rowbytes, const std::string& pixel_format);
  void callback_invoke();
  void error(const std::string& code_label, const std::string& message,
             bool unimplemented = false);
  void session_end();

 private:
  void write_base(const std::string& event_kind, const std::string& payload = {});
  static std::string escape(const std::string& value);
  static bool safe_string(const std::string& value);

  std::string host_kind_;
  std::string host_version_label_;
  std::string plugin_label_;
  std::uint64_t event_index_ = 0;
  std::filesystem::path path_;
  std::ofstream stream_;
};

}  // namespace aexcompat
