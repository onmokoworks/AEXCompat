#include "render_subsystem.h"

#include "worker_pf_world_facade.hpp"

#include <windows.h>

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <fstream>
#include <limits>
#include <sstream>
#include <utility>

namespace aexcompat::render {

namespace {
template <typename T>
void store_world_field(aexcompat::world_safety::EffectWorldStorage& world, std::size_t offset, T value) {
  std::memcpy(world.data() + offset, &value, sizeof(value));
}

bool valid_world_layout(const WorldLayout& layout) {
  return layout.pixel_bytes == 4 || layout.pixel_bytes == 8 || layout.pixel_bytes == 16
      ? layout.width > 0 && layout.height > 0 && layout.width <= 4096 &&
            layout.height <= 4096 && layout.rowbytes >= layout.width * layout.pixel_bytes
      : false;
}

}  // namespace

DiagnosticWorldLayout& diagnostic_world_layout() {
  static DiagnosticWorldLayout layout;
  return layout;
}

bool parse_diagnostic_world_layout(const wchar_t* encoded) {
  if (!encoded) return false;
  const std::wstring value(encoded);
  const bool v3 = value.compare(0, 3, L"v3|") == 0;
  const bool v2 = value.compare(0, 3, L"v2|") == 0;
  if (!v3 && !v2 && value.compare(0, 3, L"v1|") != 0) return false;
  if (value.size() > 168) return false;
  std::array<int32_t, 14> fields{};
  const std::size_t field_count = v3 ? 14 : (v2 ? 13 : 12);
  std::size_t start = 3;
  for (std::size_t index = 0; index < field_count; ++index) {
    const auto end = value.find(L'|', start);
    if ((index + 1 == field_count) != (end == std::wstring::npos)) return false;
    const auto token = value.substr(start, end == std::wstring::npos ? end : end - start);
    if (token.empty() || token.size() > 11) return false;
    const auto digits = token[0] == L'-' ? 1u : 0u;
    if (digits == token.size()) return false;
    for (std::size_t digit = digits; digit < token.size(); ++digit)
      if (token[digit] < L'0' || token[digit] > L'9') return false;
    errno = 0;
    wchar_t* tail = nullptr;
    const long parsed = std::wcstol(token.c_str(), &tail, 10);
    if (errno == ERANGE || !tail || *tail != L'\0' ||
        parsed < std::numeric_limits<int32_t>::min() ||
        parsed > std::numeric_limits<int32_t>::max()) return false;
    fields[index] = static_cast<int32_t>(parsed);
    start = end + 1;
  }
  if (fields[0] < 0 || fields[0] > 256 || fields[1] < 0 || fields[1] > 256 ||
      fields[2] < -4096 || fields[2] > 4096 ||
      fields[3] < -4096 || fields[3] > 4096 ||
      ((v2 || v3) && (fields[12] < 0 || fields[12] > 255)) ||
      (v3 && fields[13] != 4 && fields[13] != 8 && fields[13] != 16)) return false;
  auto parse_rect = [&](std::size_t offset, bool& present,
                        std::array<int32_t, 4>& rect) {
    if (fields[offset] == -1 && fields[offset + 1] == -1 &&
        fields[offset + 2] == -1 && fields[offset + 3] == -1) {
      present = false;
      return true;
    }
    rect = {fields[offset], fields[offset + 1],
            fields[offset + 2], fields[offset + 3]};
    present = true;
    return rect[0] >= 0 && rect[1] >= 0 && rect[2] > rect[0] &&
           rect[3] > rect[1] && rect[2] <= 4096 && rect[3] <= 4096;
  };
  DiagnosticWorldLayout parsed{};
  parsed.enabled = true;
  parsed.input_row_padding = fields[0];
  if (v2 || v3) parsed.input_padding_byte = static_cast<uint8_t>(fields[12]);
  if (v3) parsed.input_pixel_bytes = fields[13];
  parsed.output_row_padding = fields[1];
  parsed.input_origin_x = fields[2];
  parsed.input_origin_y = fields[3];
  if (!parse_rect(4, parsed.has_request_rect, parsed.request_rect) ||
      !parse_rect(8, parsed.has_extent_hint, parsed.extent_hint)) return false;
  diagnostic_world_layout() = parsed;
  return true;
}

bool smart_geometry_rect_valid(const std::array<int32_t, 4>& rect) {
  const int64_t width = static_cast<int64_t>(rect[2]) - rect[0];
  const int64_t height = static_cast<int64_t>(rect[3]) - rect[1];
  return rect[2] >= rect[0] && rect[3] >= rect[1] &&
      rect[0] >= -kMaxSmartRectMagnitude && rect[1] >= -kMaxSmartRectMagnitude &&
      rect[2] <= kMaxSmartRectMagnitude && rect[3] <= kMaxSmartRectMagnitude &&
      width <= 4096 && height <= 4096 && width * height <= 16'777'216;
}

namespace {
bool smart_geometry_envelope_valid(const std::array<int32_t, 4>& rect) {
  return rect[2] >= rect[0] && rect[3] >= rect[1] &&
      rect[0] >= -kMaxSmartRectMagnitude && rect[1] >= -kMaxSmartRectMagnitude &&
      rect[2] <= kMaxSmartRectMagnitude && rect[3] <= kMaxSmartRectMagnitude;
}
}  // namespace

bool smart_rect_contained(const std::array<int32_t, 4>& inner,
                          const std::array<int32_t, 4>& outer) {
  const bool inner_empty = inner[0] >= inner[2] || inner[1] >= inner[3];
  return inner_empty ||
      (inner[0] >= outer[0] && inner[1] >= outer[1] &&
       inner[2] <= outer[2] && inner[3] <= outer[3]);
}

bool smart_geometry_rect_self_test() {
  bool passed = smart_geometry_rect_valid({0, 0, 640, 360});
  passed = smart_geometry_rect_valid({-8, -8, 4088, 352}) && passed;
  passed = smart_geometry_rect_valid({0, 0, 4096, 4096}) && passed;
  passed = smart_geometry_rect_valid({5, 5, 5, 5}) && passed;
  passed = !smart_geometry_rect_valid({10, 0, 0, 10}) && passed;
  passed = !smart_geometry_rect_valid({0, 10, 10, 0}) && passed;
  passed = !smart_geometry_rect_valid({0, 0, 4097, 1}) && passed;
  passed = !smart_geometry_rect_valid({0, 0, 1, 4097}) && passed;
  passed = !smart_geometry_rect_valid(
      {kMaxSmartRectMagnitude - 10, 0, kMaxSmartRectMagnitude + 10, 10}) && passed;
  passed = !smart_geometry_rect_valid(
      {-kMaxSmartRectMagnitude - 10, 0, -kMaxSmartRectMagnitude + 10, 10}) && passed;
  passed = smart_rect_contained({1, 1, 5, 5}, {0, 0, 10, 10}) && passed;
  passed = smart_rect_contained({0, 0, 10, 10}, {0, 0, 10, 10}) && passed;
  passed = !smart_rect_contained({-1, 0, 5, 5}, {0, 0, 10, 10}) && passed;
  passed = !smart_rect_contained({0, 0, 11, 10}, {0, 0, 10, 10}) && passed;
  passed = smart_rect_contained({7, 7, 7, 7}, {0, 0, 1, 1}) && passed;
  // Output world sizing follows the AE 25.3 observation (issue #102):
  // result_rect dimensions and origin, not the max_result_rect extent.
  const auto bounds_for = [](const std::array<int32_t, 4>& result,
                             const std::array<int32_t, 4>& maximum,
                             int32_t pixel_bytes) {
    std::array<unsigned char, 48> pre_output{};
    std::memcpy(pre_output.data(), result.data(), sizeof(result));
    std::memcpy(pre_output.data() + 16, maximum.data(), sizeof(maximum));
    return prepare_smart_output_bounds(pre_output.data(), pre_output.size(),
                                       pixel_bytes);
  };
  const SmartOutputBounds inset = bounds_for({8, 4, 632, 356}, {0, 0, 640, 360}, 4);
  passed = inset.valid && !inset.empty_result && inset.width == 624 &&
      inset.height == 352 && inset.rowbytes == 2496 && inset.origin_x == 8 &&
      inset.origin_y == 4 && passed;
  const SmartOutputBounds full = bounds_for({0, 0, 16, 12}, {0, 0, 16, 12}, 4);
  passed = full.valid && full.width == 16 && full.height == 12 &&
      full.rowbytes == 64 && full.origin_x == 0 && full.origin_y == 0 && passed;
  const SmartOutputBounds expanded =
      bounds_for({-2, -2, 18, 14}, {-2, -2, 18, 14}, 8);
  passed = expanded.valid && expanded.width == 20 && expanded.height == 16 &&
      expanded.rowbytes == 160 && expanded.origin_x == -2 &&
      expanded.origin_y == -2 && passed;
  const SmartOutputBounds empty = bounds_for({0, 0, 0, 0}, {0, 0, 16, 12}, 4);
  passed = empty.valid && empty.empty_result && passed;
  passed = !bounds_for({4, 4, 20, 20}, {0, 0, 16, 12}, 4).valid && passed;
  passed = !bounds_for({0, 0, 16, 12}, {0, 0, 16, 12}, 3).valid && passed;
  return passed;
}

bool prepare_world_layout(aexcompat::world_safety::EffectWorldStorage& world,
                          const WorldLayout& layout, void* pixels) {
  if (!pixels || !valid_world_layout(layout)) return false;
  world.fill(std::byte{});
  store_world_field(world, 16, layout.world_flags);
  store_world_field(world, 24, pixels);
  store_world_field(world, 32, layout.rowbytes);
  store_world_field(world, 36, layout.width);
  store_world_field(world, 40, layout.height);
  const std::array<int32_t, 4> extent{0, 0, layout.width, layout.height};
  std::memcpy(world.data() + 44, extent.data(), sizeof(extent));
  // AE's PF_World shape around the LayerDef (worker_pf_world_facade, issue
  // #1276): the depth's vtable in the storage prefix and reserved_long4
  // pointing at it. The layout was validated above, so the depth is one of
  // the three the facade knows.
  return aexcompat::worker_runtime::pf_world_facade::embed(world.data(), layout.pixel_bytes);
}

bool capture_host_world(const aexcompat::world_safety::EffectWorldStorage& world,
                        const unsigned char* owned_pixels, std::size_t owned_size,
                        int32_t pixel_bytes,
                        CapturedWorld& captured) {
  if (!owned_pixels || (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16))
    return false;
  const auto* bytes = world.data();
  void* world_pixels = nullptr;
  int32_t rowbytes = 0, width = 0, height = 0, origin_x = 0, origin_y = 0;
  std::array<int32_t, 4> extent{};
  std::memcpy(&world_pixels, bytes + 24, sizeof(world_pixels));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  std::memcpy(extent.data(), bytes + 44, sizeof(extent));
  std::memcpy(&origin_x, bytes + 104, sizeof(origin_x));
  std::memcpy(&origin_y, bytes + 108, sizeof(origin_y));
  if (world_pixels != owned_pixels || width <= 0 || height <= 0 ||
      width > 4096 || height > 4096 ||
      static_cast<int64_t>(width) * height > 16'777'216 ||
      rowbytes < width * pixel_bytes || rowbytes > width * pixel_bytes + 256 ||
      rowbytes % pixel_bytes != 0 ||
      origin_x < -4096 || origin_x > 4096 ||
      origin_y < -4096 || origin_y > 4096 ||
      extent[0] < 0 || extent[1] < 0 ||
      extent[2] <= extent[0] || extent[3] <= extent[1] ||
      extent[2] > width || extent[3] > height)
    return false;
  const uint64_t size = static_cast<uint64_t>(rowbytes) * height;
  if (size > (1ull << 30) || size > owned_size) return false;
  CapturedWorld result{};
  result.width = width;
  result.height = height;
  result.pixel_bytes = pixel_bytes;
  result.rowbytes = rowbytes;
  result.origin_x = origin_x;
  result.origin_y = origin_y;
  result.extent = extent;
  result.raw_argb.assign(owned_pixels, owned_pixels + static_cast<std::size_t>(size));
  captured = std::move(result);
  return true;
}

bool write_captured_world_handle(uint64_t handle_value, const CapturedWorld& captured) {
  if (handle_value == 0 || captured.width <= 0 || captured.height <= 0 ||
      captured.width > 4096 || captured.height > 4096 ||
      (captured.pixel_bytes != 4 && captured.pixel_bytes != 8 &&
       captured.pixel_bytes != 16) ||
      captured.rowbytes < captured.width * captured.pixel_bytes ||
      captured.rowbytes > captured.width * captured.pixel_bytes + 256 ||
      captured.rowbytes % captured.pixel_bytes != 0 ||
      captured.origin_x < -4096 || captured.origin_x > 4096 ||
      captured.origin_y < -4096 || captured.origin_y > 4096 ||
      captured.extent[0] < 0 || captured.extent[1] < 0 ||
      captured.extent[2] <= captured.extent[0] ||
      captured.extent[3] <= captured.extent[1] ||
      captured.extent[2] > captured.width ||
      captured.extent[3] > captured.height ||
      captured.raw_argb.size() !=
          static_cast<uint64_t>(captured.rowbytes) * captured.height ||
      captured.raw_argb.size() > (1ull << 30))
    return false;
  const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(handle_value));
  std::array<unsigned char, 56> header{};
  std::memcpy(header.data(), "AEXWRAW1", 8);
  const std::array<int32_t, 10> shape{
      captured.width, captured.height, captured.pixel_bytes, captured.rowbytes,
      captured.origin_x, captured.origin_y, captured.extent[0], captured.extent[1],
      captured.extent[2], captured.extent[3]};
  std::memcpy(header.data() + 8, shape.data(), sizeof(shape));
  const uint64_t count = captured.raw_argb.size();
  std::memcpy(header.data() + 48, &count, sizeof(count));
  const auto write_all = [handle](const unsigned char* bytes, std::size_t size) {
    while (size != 0) {
      DWORD written = 0;
      const DWORD chunk = static_cast<DWORD>(std::min<std::size_t>(size, 1u << 20));
      if (!WriteFile(handle, bytes, chunk, &written, nullptr) || written == 0)
        return false;
      bytes += written;
      size -= written;
    }
    return true;
  };
  return write_all(header.data(), header.size()) &&
      write_all(captured.raw_argb.data(), captured.raw_argb.size());
}

namespace {
struct WorldCaptureTarget {
  std::string stage;
  uint64_t handle{};
  bool written{};
};
std::vector<WorldCaptureTarget>& world_capture_targets() {
  static std::vector<WorldCaptureTarget> targets;
  return targets;
}
}  // namespace

bool parse_world_capture_target(const wchar_t* encoded) {
  if (!encoded || world_capture_targets().size() >= 16) return false;
  const std::wstring value(encoded);
  const auto separator = value.find(L'|');
  if (separator == std::wstring::npos || separator == 0 || separator > 96 ||
      separator + 1 >= value.size() ||
      value.find(L'|', separator + 1) != std::wstring::npos)
    return false;
  std::string stage;
  for (std::size_t index = 0; index < separator; ++index) {
    const wchar_t letter = value[index];
    if (!((letter >= L'a' && letter <= L'z') ||
          (letter >= L'A' && letter <= L'Z') ||
          (letter >= L'0' && letter <= L'9') || letter == L'-'))
      return false;
    stage.push_back(static_cast<char>(letter));
  }
  const std::wstring number = value.substr(separator + 1);
  if (number.empty() || !std::all_of(number.begin(), number.end(), [](wchar_t digit) {
        return digit >= L'0' && digit <= L'9';
      })) return false;
  try {
    const uint64_t handle_value = std::stoull(number);
    if (handle_value == 0) return false;
    const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(handle_value));
    DWORD flags = 0;
    if (!GetHandleInformation(handle, &flags)) return false;
    for (const auto& target : world_capture_targets())
      if (target.stage == stage || target.handle == handle_value) return false;
    world_capture_targets().push_back({std::move(stage), handle_value, false});
    return true;
  } catch (...) {
    return false;
  }
}

bool capture_requested_world(const std::string& stage,
                             const aexcompat::world_safety::EffectWorldStorage& world,
                             const unsigned char* owned_pixels, std::size_t owned_size,
                             int32_t pixel_bytes) {
  auto& targets = world_capture_targets();
  const auto found = std::find_if(targets.begin(), targets.end(), [&](const auto& target) {
    return target.stage == stage;
  });
  if (found == targets.end()) return true;
  auto& target = *found;
  if (target.written) return false;
  CapturedWorld captured;
  if (!capture_host_world(world, owned_pixels, owned_size, pixel_bytes, captured) ||
      !write_captured_world_handle(target.handle, captured))
    return false;
  target.written = true;
  return true;
}

bool prepare_connected_map_world(const std::string& case_id, int32_t input_width,
                                 int32_t input_height, MapWorld& map) {
  if (input_width <= 0 || input_height <= 0 || input_width > 4096 || input_height > 4096)
    return false;
  map.width = case_id == "connected_map" ? 5 : input_width;
  map.height = case_id == "connected_map" ? 3 : input_height;
  map.pixels.resize(static_cast<std::size_t>(map.width) * map.height * 4);
  for (int32_t y = 0; y < map.height; ++y) {
    for (int32_t x = 0; x < map.width; ++x) {
      auto* pixel = map.pixels.data() + (static_cast<std::size_t>(y) * map.width + x) * 4;
      const unsigned char value = static_cast<unsigned char>(
          (x + y) * 255 / (map.width + map.height - 2));
      pixel[0] = 255; pixel[1] = value; pixel[2] = value; pixel[3] = value;
    }
  }
  return prepare_world_layout(map.world, {0, 4, map.width, map.height, map.width * 4},
                              map.pixels.data());
}

ParameterProfile prepare_parameter_profile(const std::string& case_id) {
  ParameterProfile profile;
  if (case_id == "identity") profile.amount = 0;
  else if (case_id == "horizontal") { profile.amount = 9; profile.direction = 1; profile.seed = 17; }
  else if (case_id == "vertical_no_repeat") { profile.amount = 7; profile.direction = 2; profile.repeat = 0; }
  else if (case_id == "mixed") { profile.amount = 12; profile.seed = 991; profile.mix = 37.5; }
  else if (case_id == "amount_max") profile.amount = 500;
  else if (case_id == "seed_max") profile.seed = 10000;
  else if (case_id == "mix_zero") { profile.amount = 500; profile.seed = 10000; profile.mix = 0.0; }
  else if (case_id == "odd_dimensions" || case_id == "padded_stride") { profile.amount = 4; profile.seed = 3; }
  else if (case_id == "inverted_map") profile.inverted_map = true;
  return profile;
}

bool is_fixed_image_case(const std::string& case_id) {
  return case_id == "default" || case_id == "identity" ||
      case_id == "horizontal" || case_id == "vertical_no_repeat" ||
      case_id == "mixed" || case_id == "amount_max" ||
      case_id == "seed_max" || case_id == "mix_zero" ||
      case_id == "odd_dimensions" || case_id == "padded_stride" ||
      case_id == "inverted_map" || case_id == "connected_map";
}

bool output_extent_unchanged(int32_t current_width, int32_t current_height,
                             int32_t requested_width, int32_t requested_height) {
  if (requested_width == 0 && requested_height == 0) return true;
  return requested_width == current_width && requested_height == current_height;
}

bool validate_output_extent(int32_t current_width, int32_t current_height,
                            int32_t requested_width, int32_t requested_height,
                            uint32_t output_flags) {
  // Declining is not a malformed extent, and it is one rule rather than two:
  // the call site checks the same predicate to decide whether to resize at all,
  // so stating it twice would let the two disagree about what a 0x0 answer
  // means - a frame that renders in one build and fails validation in the next.
  if (output_extent_unchanged(current_width, current_height, requested_width,
                              requested_height))
    return true;
  if (requested_width <= 0 || requested_height <= 0 || requested_width > 4096 ||
      requested_height > 4096 ||
      static_cast<int64_t>(requested_width) * requested_height > 16'777'216)
    return false;
  const bool expands = requested_width > current_width || requested_height > current_height;
  const bool shrinks = requested_width < current_width || requested_height < current_height;
  constexpr uint32_t kExpandBuffer = 1u << 9;
  constexpr uint32_t kShrinkBuffer = 1u << 12;
  return (!expands || (output_flags & kExpandBuffer) != 0) &&
      (!shrinks || (output_flags & kShrinkBuffer) != 0);
}

std::array<int32_t, 4> extent_hint_within(const std::array<int32_t, 4>& hint,
                                          int32_t output_width, int32_t output_height) {
  if (output_width < 0 || output_height < 0) return hint;
  std::array<int32_t, 4> shrunk = hint;
  // The top-left only moves if it is already outside the buffer, and then only
  // as far as the buffer's own edge; the bottom-right comes back to the edge or
  // to the top-left, whichever is further in. That keeps the rect non-inverted
  // without ever growing it or shifting a rect that already fitted.
  shrunk[0] = std::min(shrunk[0], output_width);
  shrunk[1] = std::min(shrunk[1], output_height);
  shrunk[2] = std::min(std::max(shrunk[2], shrunk[0]), output_width);
  shrunk[3] = std::min(std::max(shrunk[3], shrunk[1]), output_height);
  return shrunk;
}

bool validate_output_origin(int32_t origin_x, int32_t origin_y,
                            int32_t source_width, int32_t source_height,
                            int32_t output_width, int32_t output_height) {
  if (source_width < 0 || source_height < 0) return false;
  if (output_width < 0 || output_height < 0) return false;
  if (origin_x < -kMaxSmartRectMagnitude || origin_x > kMaxSmartRectMagnitude ||
      origin_y < -kMaxSmartRectMagnitude || origin_y > kMaxSmartRectMagnitude)
    return false;
  // The input rectangle, placed at this origin, has to reach the output at all.
  // Both directions are the same statement: an expand insets the input inside a
  // larger output (positive origin), a crop-shrink takes a window out of it
  // (negative origin), and either way the two overlap.
  return static_cast<int64_t>(origin_x) < output_width &&
      static_cast<int64_t>(origin_x) + source_width > 0 &&
      static_cast<int64_t>(origin_y) < output_height &&
      static_cast<int64_t>(origin_y) + source_height > 0;
}

SmartOutputBounds prepare_smart_output_bounds(const void* pre_render_output,
                                              std::size_t output_size,
                                              int32_t pixel_bytes) {
  SmartOutputBounds bounds;
  if (!pre_render_output || output_size < 32 ||
      (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16)) return bounds;
  std::memcpy(bounds.result_rect.data(), pre_render_output, sizeof(bounds.result_rect));
  std::memcpy(bounds.max_result_rect.data(),
              static_cast<const std::byte*>(pre_render_output) + 16,
              sizeof(bounds.max_result_rect));
  // Only result_rect sizes the output allocation. max_result_rect is an
  // availability envelope and may legitimately be much larger while still
  // containing a small render result, so keep its coordinate/inversion checks
  // without applying the allocation width/height/area caps.
  if (!smart_geometry_rect_valid(bounds.result_rect) ||
      !smart_geometry_envelope_valid(bounds.max_result_rect) ||
      bounds.result_rect[0] < bounds.max_result_rect[0] ||
      bounds.result_rect[1] < bounds.max_result_rect[1] ||
      bounds.result_rect[2] > bounds.max_result_rect[2] ||
      bounds.result_rect[3] > bounds.max_result_rect[3]) return bounds;
  if (bounds.result_rect[0] >= bounds.result_rect[2] ||
      bounds.result_rect[1] >= bounds.result_rect[3]) {
    // Legal empty answer: geometry is well-formed and nothing will render, so
    // no output world is sized at all.
    bounds.empty_result = true;
    bounds.valid = true;
    return bounds;
  }
  // AE 25.3 observation (issue #102): with result_rect strictly inside
  // max_result_rect, AE supplies the output world at the result_rect
  // dimensions with the world origin at the result_rect top-left, not at
  // the max_result_rect extent.
  bounds.width = bounds.result_rect[2] - bounds.result_rect[0];
  bounds.height = bounds.result_rect[3] - bounds.result_rect[1];
  if (bounds.width <= 0 || bounds.height <= 0) return bounds;
  bounds.rowbytes = bounds.width * pixel_bytes;
  bounds.origin_x = bounds.result_rect[0];
  bounds.origin_y = bounds.result_rect[1];
  bounds.valid = true;
  return bounds;
}

bool copy_packed_world(const unsigned char* strided_source, int32_t rowbytes,
                       int32_t width, int32_t height, int32_t pixel_bytes,
                       std::vector<unsigned char>& packed_destination) {
  if (!strided_source || width < 0 || height < 0) return false;
  if (width == 0 || height == 0) {
    packed_destination.clear();
    return true;
  }
  if (!valid_world_layout({0, pixel_bytes, width, height, rowbytes})) return false;
  packed_destination.resize(static_cast<std::size_t>(width) * height * pixel_bytes);
  for (int32_t y = 0; y < height; ++y)
    std::memcpy(packed_destination.data() + static_cast<std::size_t>(y) * width * pixel_bytes,
                strided_source + static_cast<std::size_t>(y) * rowbytes,
                static_cast<std::size_t>(width) * pixel_bytes);
  return true;
}

bool finite_float_world(const std::vector<unsigned char>& packed) {
  if (packed.empty() || packed.size() % sizeof(float) != 0) return false;
  for (std::size_t offset = 0; offset < packed.size(); offset += sizeof(float)) {
    float value{};
    std::memcpy(&value, packed.data() + offset, sizeof(value));
    if (!std::isfinite(value)) return false;
  }
  return true;
}

int dispatch(RenderContext& context) {
  if (!context.request || !context.hooks.guarded_effect_main ||
      !context.hooks.cleanup || !context.hooks.dependencies_ready)
    return -1;

  // Module audit happens in the supplied guarded EffectMain path.  The flag is
  // retained here to make that dependency explicit at the translation-unit
  // boundary and to reject an unprepared audit request before any selector.
  if (context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;
  if (!context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;

  context.selector_started = true;
  context.primary_error = context.hooks.guarded_effect_main(context.request);

  // Cleanup is unconditional after selector admission.  The host hook owns
  // sequence/frame setdown, pre-render-data deletion, GPU setdown, suite
  // release, world unregistering, and automatic parameter checkins.
  context.cleanup_started = true;
  context.cleanup_error = context.hooks.cleanup(context.request);
  return context.primary_error != 0 ? context.primary_error : context.cleanup_error;
}

int prepare_image_request(const std::string& case_id, bool has_external_input,
                          int32_t external_width, int32_t external_height,
                          int32_t external_pixel_bytes, ImageRequest& request) {
  request.connected_map = case_id == "connected_map" || case_id == "inverted_map";
  request.partial_extent_hint = case_id == "partial_extent_hint";
  request.width = has_external_input ? external_width :
      (request.connected_map ? 11 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16));
  request.height = has_external_input ? external_height :
      (request.connected_map ? 7 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12));
  const auto& diagnostic = diagnostic_world_layout();
  request.pixel_bytes = diagnostic.enabled && diagnostic.input_pixel_bytes != 0
      ? diagnostic.input_pixel_bytes : (has_external_input ? external_pixel_bytes : 4);
  if (request.width <= 0 || request.height <= 0 || request.width > 4096 ||
      request.height > 4096 ||
      (request.pixel_bytes != 4 && request.pixel_bytes != 8 && request.pixel_bytes != 16) ||
      (external_pixel_bytes != 4 && external_pixel_bytes != 8 && external_pixel_bytes != 16))
    return -3;
  if (!is_fixed_image_case(case_id) && case_id != "request" &&
      !request.partial_extent_hint)
    return -2;
  request.rowbytes = case_id == "padded_stride" ? 64 :
      request.width * request.pixel_bytes;
  if (diagnostic.enabled) {
    if (diagnostic.has_request_rect ||
        diagnostic.input_row_padding % request.pixel_bytes != 0 ||
        diagnostic.output_row_padding % external_pixel_bytes != 0 ||
        (diagnostic.has_extent_hint &&
         (diagnostic.extent_hint[2] > request.width ||
          diagnostic.extent_hint[3] > request.height))) return -3;
    request.rowbytes += diagnostic.input_row_padding;
  }
  return 0;
}

bool build_argb_input(const ImageRequest& request,
                      const std::vector<unsigned char>* external_rgba,
                      std::vector<unsigned char>& logical_argb,
                      unsigned char* strided_destination) {
  if (!strided_destination) return false;
  const std::size_t pixels = static_cast<std::size_t>(request.width) * request.height;
  if (external_rgba && external_rgba->size() != pixels * 4) return false;
  logical_argb.assign(pixels * request.pixel_bytes, 0);
  for (int32_t y = 0; y < request.height; ++y) {
    for (int32_t x = 0; x < request.width; ++x) {
      auto* argb = logical_argb.data() +
          (static_cast<std::size_t>(y) * request.width + x) * request.pixel_bytes;
      if (external_rgba) {
        const auto* rgba = external_rgba->data() +
            (static_cast<std::size_t>(y) * request.width + x) * 4;
        if (request.pixel_bytes == 4) {
          argb[0] = rgba[3]; argb[1] = rgba[0]; argb[2] = rgba[1]; argb[3] = rgba[2];
        } else if (request.pixel_bytes == 8) {
          auto* value = reinterpret_cast<uint16_t*>(argb);
          value[0] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[3]) * 32768u + 127u) / 255u);
          value[1] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[0]) * 32768u + 127u) / 255u);
          value[2] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[1]) * 32768u + 127u) / 255u);
          value[3] = static_cast<uint16_t>((static_cast<uint32_t>(rgba[2]) * 32768u + 127u) / 255u);
        } else {
          auto* value = reinterpret_cast<float*>(argb);
          value[0] = rgba[3] / 255.0f; value[1] = rgba[0] / 255.0f;
          value[2] = rgba[1] / 255.0f; value[3] = rgba[2] / 255.0f;
        }
      } else {
        // The remaining bytes stay zero for the 16/32-bit default gradient,
        // exactly as the legacy request construction did.
        argb[0] = 255;
        argb[1] = static_cast<unsigned char>(x * 255 / (request.width - 1));
        argb[2] = static_cast<unsigned char>(y * 255 / (request.height - 1));
        argb[3] = static_cast<unsigned char>((x + y) * 255 /
                                              (request.width + request.height - 2));
      }
    }
    // The converted logical row is contiguous. Copy only its active bytes;
    // the guarded world's row padding keeps the caller's initialization.
    const std::size_t row_size = static_cast<std::size_t>(request.width) * request.pixel_bytes;
    std::memcpy(strided_destination + static_cast<std::size_t>(y) * request.rowbytes,
                logical_argb.data() + static_cast<std::size_t>(y) * row_size, row_size);
  }
  return true;
}

namespace {
constexpr uint32_t kMaxWorldDumps = 32;
constexpr uint64_t kMaxWorldDumpBytes = 1ull << 30;

const char* world_dump_extension(int32_t pixel_bytes) {
  return pixel_bytes == 16 ? "rgba32f-le" :
      (pixel_bytes == 8 ? "rgba16le" : "rgba8");
}

uint32_t crc32_ieee(const unsigned char* data, std::size_t size) {
  static const auto table = [] {
    std::array<uint32_t, 256> built{};
    for (uint32_t index = 0; index < 256; ++index) {
      uint32_t value = index;
      for (int bit = 0; bit < 8; ++bit)
        value = (value >> 1) ^ ((value & 1u) ? 0xEDB88320u : 0u);
      built[index] = value;
    }
    return built;
  }();
  uint32_t crc = 0xFFFFFFFFu;
  for (std::size_t index = 0; index < size; ++index)
    crc = (crc >> 8) ^ table[(crc ^ data[index]) & 0xFFu];
  return crc ^ 0xFFFFFFFFu;
}
}  // namespace

void dump_world_snapshot(RenderTelemetry& telemetry, const std::string& stage,
                         const unsigned char* packed_argb, int32_t width,
                         int32_t height, int32_t pixel_bytes) {
  if (!telemetry.dump_directory || telemetry.dump_directory->empty() || !packed_argb ||
      width <= 0 || height <= 0 || !telemetry.dumps_written || !telemetry.dumps_skipped ||
      !telemetry.dump_bytes || !telemetry.hooks.argb_to_rgba_native) return;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if (*telemetry.dumps_written >= kMaxWorldDumps ||
      bytes > kMaxWorldDumpBytes - *telemetry.dump_bytes) {
    ++*telemetry.dumps_skipped;
    return;
  }
  std::vector<unsigned char> rgba(static_cast<std::size_t>(bytes));
  for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
    telemetry.hooks.argb_to_rgba_native(rgba.data() + pixel * pixel_bytes,
                                         packed_argb + pixel * pixel_bytes, pixel_bytes);
  char name[128];
  std::snprintf(name, sizeof(name), "%03u-%s-%dx%d.%s", *telemetry.dumps_written,
                stage.c_str(), width, height, world_dump_extension(pixel_bytes));
  std::ofstream file(*telemetry.dump_directory / name, std::ios::binary | std::ios::out);
  if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size())) {
    ++*telemetry.dumps_skipped;
    return;
  }
  ++*telemetry.dumps_written;
  *telemetry.dump_bytes += bytes;
}

void record_output_checksum_detail(RenderTelemetry& telemetry,
                                   const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes) {
  if (!telemetry.output_checksum_detail || !telemetry.output_row_crc32 ||
      !telemetry.output_channel_sha256 || !telemetry.hooks.sha256_bytes) return;
  // A legally empty output (0x0, e.g. an empty SmartFX result #278) has no rows
  // and no channel bytes. Record the empty detail (no row CRCs, each channel the
  // sha256 of zero bytes) rather than returning early, so a later report never
  // carries a previous frame's stale checksums.
  if (!rgba || width <= 0 || height <= 0) {
    telemetry.output_row_crc32->clear();
    const unsigned char empty_marker = 0;
    for (std::size_t channel = 0; channel < 4; ++channel)
      (*telemetry.output_channel_sha256)[channel] =
          telemetry.hooks.sha256_bytes(&empty_marker, 0);
    return;
  }
  const std::size_t row_bytes = static_cast<std::size_t>(width) * pixel_bytes;
  telemetry.output_row_crc32->clear();
  telemetry.output_row_crc32->reserve(static_cast<std::size_t>(height));
  for (int32_t row = 0; row < height; ++row)
    telemetry.output_row_crc32->push_back(crc32_ieee(rgba + row * row_bytes, row_bytes));
  const std::size_t sample_bytes = static_cast<std::size_t>(pixel_bytes) / 4;
  std::vector<unsigned char> plane(static_cast<std::size_t>(width) * height * sample_bytes);
  for (std::size_t channel = 0; channel < 4; ++channel) {
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
      std::memcpy(plane.data() + pixel * sample_bytes,
                  rgba + pixel * pixel_bytes + channel * sample_bytes, sample_bytes);
    (*telemetry.output_channel_sha256)[channel] =
        telemetry.hooks.sha256_bytes(plane.data(), plane.size());
  }
}

std::string world_debug_report_json(const RenderTelemetry& telemetry) {
  std::ostringstream json;
  const uint32_t written = telemetry.dumps_written ? *telemetry.dumps_written : 0;
  const uint32_t skipped = telemetry.dumps_skipped ? *telemetry.dumps_skipped : 0;
  const uint64_t bytes = telemetry.dump_bytes ? *telemetry.dump_bytes : 0;
  json << ",\"world_dumps_written\":" << written
       << ",\"world_dumps_skipped\":" << skipped
       << ",\"world_dump_bytes\":" << bytes;
  if (telemetry.output_checksum_detail && telemetry.output_row_crc32 &&
      telemetry.output_channel_sha256) {
    json << ",\"output_row_crc32\":[";
    for (std::size_t row = 0; row < telemetry.output_row_crc32->size(); ++row) {
      if (row) json << ',';
      char text[12];
      std::snprintf(text, sizeof(text), "\"%08x\"", (*telemetry.output_row_crc32)[row]);
      json << text;
    }
    json << "],\"output_channel_sha256\":[";
    for (std::size_t channel = 0; channel < 4; ++channel) {
      if (channel) json << ',';
      json << '"' << (*telemetry.output_channel_sha256)[channel] << '"';
    }
    json << ']';
  }
  return json.str();
}

RenderContextState& render_context_state() {
  static RenderContextState state;
  return state;
}

TelemetryState& telemetry_state() {
  static TelemetryState state;
  return state;
}

}  // namespace aexcompat::render
