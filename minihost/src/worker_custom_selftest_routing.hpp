#pragma once

#include <array>
#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::custom_selftests {

struct Request {
  int argc{};
  wchar_t** argv{};
};

struct Hooks {
  bool (*run_pf_path_data_hardening)(){};
  bool (*verify_world_double_dispose_rejected)(){};
  bool (*verify_world_allocation_limit_rejected)(){};
  bool (*verify_owned_world_snapshot_is_atomic)(){};
  bool (*verify_owned_world_snapshot_concurrent_dispose)(){};
  bool (*verify_pf_effect_sequence_data_suite1)(){};
  bool (*verify_aegp_async_receipts)(){};
  std::string (*sha256_bytes)(const unsigned char*, std::size_t){};
  std::string (*hex_bytes)(const unsigned char*, std::size_t){};
  const std::array<uint8_t, 4>* render_options_baseline8{};
  const std::array<uint8_t, 4>* render_options_time8{};
  const std::array<uint8_t, 4>* render_options_downsample8{};
  const std::array<uint8_t, 4>* render_options_roi_inside8{};
  const std::array<uint8_t, 4>* render_options_matte8{};
  const std::array<uint16_t, 4>* render_options_argb16{};
  const std::array<float, 4>* render_options_argb32f{};
};

struct Result {
  bool handled{};
  int exit_code{};
  std::string output;
};

// Owns exact arity, protocol output, and exit codes for the remaining one-off
// component self-tests. Host-private verification bodies and hash helpers stay
// behind explicit hooks; shared component statistics are read from their owner
// runtimes.
Result dispatch(const Request& request, const Hooks& hooks);

}  // namespace aexcompat::worker_runtime::custom_selftests
