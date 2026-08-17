#include <windows.h>
#include <bcrypt.h>

#include "worker_pf_progress_info.hpp"
#include "parameter_animation_transport.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "worker_aegp_compute_cache.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_selector_dispatch.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <sstream>
#include <string>
#include <vector>

// Shared worker-entry helpers moved from worker_main (issue #165): rational
// time comparison, parameter-animation evaluation, the guarded global/sequence
// selector invokers, diagnostic escaping/hashing, and the world-dump telemetry
// wrappers. Effect identity stays in l2_main behind a cross-TU extern.
namespace aexcompat::l2_detail {

using namespace aexcompat::pf_state_runtime;
using aexcompat::parameter_animation::AnimationKey;
using aexcompat::parameter_animation::AnimationValueKind;
using aexcompat::parameter_animation::ParameterAnimationKey;
using aexcompat::parameter_animation::ParameterTimeline;
using aexcompat::parameter_animation::rational_less;
using aexcompat::render_pixel_transport::argb_to_rgba_native;
using aexcompat::worker_runtime::invoke_entry_seh;
using aexcompat::worker_runtime::parameters::ParamRecord;
using aexcompat::worker_runtime::parameters::animation_component_value;
using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);

extern aexcompat::worker_runtime::pf_progress_info::EffectRefObject g_effect;
std::string sha256_bytes(const unsigned char* data, std::size_t size);

namespace {
constexpr std::size_t kParamSize =
    aexcompat::worker_runtime::parameters::kDefinitionSize;
constexpr std::size_t kOutSequenceData = 56;
constexpr int32_t kGlobalSetdown = 3;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceResetup = 6;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kPfBadCallbackParam = 516;
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_parameter_runtime.records;
auto& g_parameter_timelines = g_parameter_runtime.timelines;

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}
}  // namespace

const ParameterTimeline *parameter_timeline(int32_t slot) {
  return aexcompat::worker_runtime::parameters::timeline(slot);
}

bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale) {
  return static_cast<int64_t>(left) * right_scale ==
      static_cast<int64_t>(right) * left_scale;
}

void write_rect(void* destination, int32_t width, int32_t height) {
  auto* bytes = static_cast<std::byte*>(destination);
  const int32_t values[4] = {0, 0, width, height};
  std::memcpy(bytes, values, sizeof(values));
}

ParameterAnimationKey evaluate_animation(const ParameterTimeline &timeline,
                                         int32_t time, uint32_t scale) {
  if (!rational_less(timeline.keys.front().time, timeline.keys.front().scale,
                     time, scale))
    return timeline.keys.front();
  for (std::size_t i = 1; i < timeline.keys.size(); ++i) {
    const auto &right = timeline.keys[i];
    if (rational_less(time, scale, right.time, right.scale)) {
      const auto &left = timeline.keys[i - 1];
      if (left.hold || left.kind != right.kind ||
          left.component_count != right.component_count)
        return left;
      const long double now = static_cast<long double>(time) / scale,
                        a = static_cast<long double>(left.time) / left.scale,
                        b = static_cast<long double>(right.time) / right.scale;
      const double f = static_cast<double>((now - a) / (b - a));
      AnimationKey value = left;
      if (value.kind == AnimationValueKind::Scalar)
        value.scalar += (right.scalar - value.scalar) * f;
      else if (value.kind == AnimationValueKind::Color)
        for (std::size_t c = 0; c < 4; ++c)
          value.color[c] = static_cast<unsigned char>(
              std::clamp(std::lround(value.color[c] +
                                     (right.color[c] - value.color[c]) * f),
                         0l, 255l));
      else
        for (int c = 0; c < value.component_count; ++c)
          value.components[c] +=
              (right.components[c] - value.components[c]) * f;
      return value;
    }
  }
  return timeline.keys.back();
}

bool write_animation_value(std::array<std::byte, kParamSize> &definition,
                           const ParamRecord &param,
                           const ParameterAnimationKey &key) {
  if (key.kind == AnimationValueKind::Scalar) {
    if (param.type == 1 || param.type == 4 || param.type == 7) {
      if (!std::isfinite(key.scalar) || std::floor(key.scalar) != key.scalar ||
          key.scalar < INT32_MIN || key.scalar > INT32_MAX)
        return false;
      write<int32_t>(definition, 56, static_cast<int32_t>(key.scalar));
    } else if (param.type == 2) {
      const double encoded = key.scalar * 65536.0;
      if (encoded < INT32_MIN || encoded > INT32_MAX)
        return false;
      write<int32_t>(definition, 56, static_cast<int32_t>(std::round(encoded)));
    } else if (param.type == 10) {
      write<double>(definition, 56, key.scalar);
    } else {
      return false;
    }
  } else if (key.kind == AnimationValueKind::Color) {
    if (param.type != 5)
      return false;
    std::memcpy(definition.data() + 56, key.color.data(), key.color.size());
  } else {
    const int component_count =
        param.type == 3 ? 1
                        : (param.type == 6 ? 2 : (param.type == 18 ? 3 : 0));
    if (component_count == 0 || key.component_count != component_count)
      return false;
    for (int component = 0; component < component_count; ++component) {
      if (param.type == 18) {
        write<double>(definition, 56 + component * 8,
                      animation_component_value(param, component,
                                                key.components[component]));
      } else {
        const double encoded =
            animation_component_value(param, component,
                                      key.components[component]) *
            65536.0;
        if (encoded < INT32_MIN || encoded > INT32_MAX)
          return false;
        write<int32_t>(definition, 56 + component * 4,
                       static_cast<int32_t>(std::round(encoded)));
      }
    }
  }
  return true;
}

bool apply_parameter_animation(
    std::vector<std::array<std::byte, kParamSize>> &definitions, int32_t time,
    uint32_t scale) {
  if (scale == 0)
    return false;
  for (const auto &timeline : g_parameter_timelines) {
    if (timeline.slot <= 0 ||
        static_cast<std::size_t>(timeline.slot) >= definitions.size())
      return false;
    const auto &param = g_params[timeline.slot - 1];
    if (timeline.keys.front().kind == AnimationValueKind::Arbitrary) {
      if (param.type != 11 || std::any_of(timeline.keys.begin(), timeline.keys.end(),
          [](const auto &key) { return key.kind != AnimationValueKind::Arbitrary; }))
        return false;
      continue;
    }
    if (param.type == 11 || !write_animation_value(definitions[timeline.slot], param,
                                                   evaluate_animation(timeline, time, scale)))
      return false;
  }
  return true;
}

int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output) {
  uint32_t exception_code{};
  const int32_t error = invoke_entry_seh(entry, kGlobalSetdown, input, output,
                                         nullptr, nullptr, nullptr,
                                         &exception_code);
  const bool compute_cache_teardown_safe =
      aexcompat::worker_runtime::compute_cache::teardown_owner_from_entry(
          reinterpret_cast<const void*>(entry));
  on_global_setdown();
  aexcompat::pf_helper::reset();
  return error != 0 ? error
                    : (compute_cache_teardown_safe
                           ? aexcompat::worker_runtime::compute_cache::kErrNone
                           : aexcompat::worker_runtime::compute_cache::kErrStruct);
}

int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr) {
  uint32_t local_exception{};
  uint32_t* observed_exception = exception_code ? exception_code : &local_exception;
  if (selector == kSequenceSetdown) invalidate_effect_sequence(&g_effect);
  const int32_t error = invoke_entry_seh(entry, selector, input, output, nullptr, nullptr,
                                         nullptr, observed_exception);
  if (selector == kSequenceSetup || selector == kSequenceResetup) {
    if (error == 0 && *observed_exception == 0) {
      void* sequence_handle{};
      std::memcpy(&sequence_handle,
                  static_cast<const std::byte*>(output) + kOutSequenceData,
                  sizeof(sequence_handle));
      if (!sequence_handle) {
        invalidate_effect_sequence(&g_effect);
      } else if (!publish_effect_sequence(&g_effect, sequence_handle)) {
        invalidate_effect_sequence(&g_effect);
        return kPfBadCallbackParam;
      }
    } else {
      invalidate_effect_sequence(&g_effect);
    }
  }
  return error;
}

std::string escape(const std::string& input) {
  std::string output;
  for (unsigned char ch : input) {
    if (ch == '"' || ch == '\\') output.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) output.push_back(static_cast<char>(ch));
  }
  return output;
}

bool sha256(const std::filesystem::path& path, std::string& result) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0 ||
      BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                        &returned, 0) < 0) return false;
  std::vector<unsigned char> object(object_size);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0) < 0) return false;
  std::ifstream input(path, std::ios::binary);
  std::array<unsigned char, 65536> buffer{};
  while (input) {
    input.read(reinterpret_cast<char*>(buffer.data()), buffer.size());
    if (input.gcount() > 0 && BCryptHashData(hash, buffer.data(), static_cast<ULONG>(input.gcount()), 0) < 0) return false;
  }
  std::array<unsigned char, 32> digest{};
  const bool ok = input.eof() && BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0;
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  if (!ok) return false;
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  result = text.str();
  return true;
}

std::string sha256_bytes(const unsigned char* data, std::size_t size) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
  BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                    reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &returned, 0);
  std::vector<unsigned char> object(object_size);
  BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0);
  BCryptHashData(hash, const_cast<PUCHAR>(data), static_cast<ULONG>(size), 0);
  std::array<unsigned char, 32> digest{};
  BCryptFinishHash(hash, digest.data(), digest.size(), 0);
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  return text.str();
}

void dump_world_snapshot(const std::string& stage, const unsigned char* packed_argb,
                         int32_t width, int32_t height, int32_t pixel_bytes) {
  auto& state = aexcompat::render::telemetry_state();
  aexcompat::render::RenderTelemetry telemetry{
      &state.dump_worlds_dir, &state.world_dumps_written, &state.world_dumps_skipped,
      &state.world_dump_bytes, state.output_checksum_detail, &state.output_row_crc32,
      &state.output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  aexcompat::render::dump_world_snapshot(telemetry, stage, packed_argb, width,
                                          height, pixel_bytes);
}

// Record per-row CRC32 and per-channel SHA-256 of the RGBA-ordered output
// transport bytes, so a differing region can be narrowed to rows and channels
// without shipping any pixel content in the report.
void record_output_checksum_detail(const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes) {
  auto& state = aexcompat::render::telemetry_state();
  aexcompat::render::RenderTelemetry telemetry{
      &state.dump_worlds_dir, &state.world_dumps_written, &state.world_dumps_skipped,
      &state.world_dump_bytes, state.output_checksum_detail, &state.output_row_crc32,
      &state.output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  aexcompat::render::record_output_checksum_detail(telemetry, rgba, width,
                                                    height, pixel_bytes);
}

// Single insertion point for both image report emitters: dump counters are
// always present, checksum detail only when the opt-in trailer enabled it.
std::string world_debug_report_json() {
  auto& state = aexcompat::render::telemetry_state();
  const aexcompat::render::RenderTelemetry telemetry{
      &state.dump_worlds_dir, &state.world_dumps_written, &state.world_dumps_skipped,
      &state.world_dump_bytes, state.output_checksum_detail, &state.output_row_crc32,
      &state.output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  return aexcompat::render::world_debug_report_json(telemetry);
}

}  // namespace aexcompat::l2_detail
