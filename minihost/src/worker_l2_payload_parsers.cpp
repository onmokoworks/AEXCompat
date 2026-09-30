#include "worker_request_parser.hpp"

#include "render_subsystem.h"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_aegp_scene_transaction.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_parameter_limits.hpp"

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <limits>
#include <string>
#include <unordered_set>
#include <vector>

// Broker payload parsers moved from worker_main (issue #165): the layer
// transport key, mask/spatial/render-environment context payloads, and the
// requested-parameter payload with their bounded numeric readers. Scene and
// render-context state resolves through the same owners as before.
namespace aexcompat::l2_detail {

using ExternalLayerInput = aexcompat::worker_runtime::request_parser::LayerInput;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
using RequestedKind = aexcompat::worker_runtime::parameters::RequestedKind;

namespace {
constexpr std::size_t kMaxParams =
    aexcompat::worker_runtime::parameters::kMaxParameterCount;
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;
auto& g_pixel_aspect_ratio = g_render_context_state.pixel_aspect_ratio;
auto& g_downsample_x = g_render_context_state.downsample_x;
auto& g_downsample_y = g_render_context_state.downsample_y;
auto& g_pre_effect_source_origin_x = g_render_context_state.pre_effect_source_origin_x;
auto& g_pre_effect_source_origin_y = g_render_context_state.pre_effect_source_origin_y;
auto& g_render_quality = g_render_context_state.render_quality;
auto& g_render_field = g_render_context_state.render_field;
auto& g_shutter_angle = g_render_context_state.shutter_angle;
auto& g_shutter_phase = g_render_context_state.shutter_phase;
}  // namespace

bool parse_layer_transport_key(const wchar_t* text, ExternalLayerInput& layer) {
  if (!text) return false;
  if (std::wstring(text).compare(0, 3, L"v1|") != 0) {
    try { layer.slot = std::stoi(text); } catch (...) { return false; }
    return layer.slot > 0;
  }
  int consumed = 0;
  if (swscanf_s(text, L"v1|%d|%d|%u%n", &layer.slot, &layer.time,
          &layer.time_scale, &consumed) != 3 || text[consumed] != L'\0' ||
      layer.slot <= 0 || layer.time_scale == 0) return false;
  layer.timed = true;
  return true;
}

bool parse_i32_arg(const wchar_t* text, int32_t minimum, int32_t maximum, int32_t& output) {
  if (!text || !*text) return false;
  wchar_t* end = nullptr;
  errno = 0;
  const long long value = std::wcstoll(text, &end, 10);
  if (errno != 0 || !end || *end != L'\0' || value < minimum || value > maximum) return false;
  output = static_cast<int32_t>(value);
  return true;
}

bool parse_double_arg(const wchar_t* text, double minimum, double maximum, double& output) {
  if (!text || !*text) return false;
  wchar_t* end = nullptr;
  errno = 0;
  const double value = std::wcstod(text, &end);
  if (errno != 0 || !end || *end != L'\0' || !std::isfinite(value) || value < minimum || value > maximum)
    return false;
  output = value;
  return true;
}

bool parse_mask_context_payload(const wchar_t* text) {
  if (!text) return false;
  if (!g_stream_refs.empty() || !g_stream_values.empty() ||
      !g_add_keyframe_transactions.empty()) return false;
  const std::wstring encoded(text);
  if (encoded.size() < 3 || encoded.size() > 8192 || encoded.compare(0, 3, L"v2|") != 0)
    return false;
  configure_mask_runtime_hooks();
  std::vector<HostMask> masks;
  std::size_t total_vertices = 0;
  const std::wstring payload = encoded.substr(3);
  if (payload.empty()) {
    g_mask_scene.clear();
    g_mask_lifetime = {};
    aexcompat::mask_runtime::set_mask_scene_id("request_v4");
    return true;
  }
  std::size_t mask_offset = 0;
  while (mask_offset < payload.size()) {
    const std::size_t mask_separator = payload.find(L';', mask_offset);
    const std::size_t mask_end = mask_separator == std::wstring::npos
        ? payload.size() : mask_separator;
    const std::wstring item = payload.substr(mask_offset, mask_end - mask_offset);
    if (item.size() < 5 || (item.compare(0, 2, L"0:") != 0 &&
                            item.compare(0, 2, L"1:") != 0)) return false;
    HostMask mask;
    mask.id = g_next_mask_id++;
    mask.outline_stream_id = g_next_stream_id++;
    mask.feather_stream_id = g_next_stream_id++;
    mask.opacity_stream_id = g_next_stream_id++;
    mask.expansion_stream_id = g_next_stream_id++;
    mask.dynamic_order = static_cast<int32_t>(masks.size());
    mask.open = item[0] == L'1';
    std::size_t vertex_offset = 2;
    while (vertex_offset < item.size()) {
      const std::size_t vertex_separator = item.find(L'/', vertex_offset);
      const std::size_t vertex_end = vertex_separator == std::wstring::npos
          ? item.size() : vertex_separator;
      const std::wstring point = item.substr(vertex_offset, vertex_end - vertex_offset);
      std::array<double, 6> components{};
      std::size_t component_offset = 0;
      for (std::size_t component = 0; component < components.size(); ++component) {
        const std::size_t comma = point.find(L',', component_offset);
        const bool final_component = component + 1 == components.size();
        if ((final_component && comma != std::wstring::npos) ||
            (!final_component && comma == std::wstring::npos)) return false;
        const std::size_t component_end = final_component ? point.size() : comma;
        if (!parse_double_arg(point.substr(component_offset, component_end - component_offset).c_str(),
                              -32768.0, 32768.0, components[component])) return false;
        component_offset = component_end + 1;
      }
      mask.vertices.push_back({components[0], components[1], components[2],
                               components[3], components[4], components[5]});
      if (mask.vertices.size() > 64 || ++total_vertices > 128) return false;
      if (vertex_separator == std::wstring::npos) break;
      vertex_offset = vertex_separator + 1;
      if (vertex_offset == item.size()) return false;
    }
    if (mask.vertices.size() < (mask.open ? 2u : 3u)) return false;
    if (!mask.open) mask.vertices.push_back(mask.vertices.front());
    masks.push_back(std::move(mask));
    if (masks.size() > 8) return false;
    if (mask_separator == std::wstring::npos) break;
    mask_offset = mask_separator + 1;
    if (mask_offset == payload.size()) return false;
  }
  g_mask_scene = std::move(masks);
  g_mask_scene.reserve(kMaxHostMasks);
  g_mask_lifetime = {};
  aexcompat::mask_runtime::set_mask_scene_id("request_v4");
  return true;
}

bool parse_spatial_context_payload(const wchar_t* text) {
  if (!text) return false;
  const std::wstring encoded(text);
  const bool version3 = encoded.compare(0, 11, L"spatial:v3|") == 0;
  const bool version2 = encoded.compare(0, 11, L"spatial:v2|") == 0;
  if ((!version3 && !version2 && encoded.compare(0, 11, L"spatial:v1|") != 0) || encoded.size() > 160) return false;
  const std::wstring payload = encoded.substr(11);
  std::array<int32_t, 10> values{};
  const std::size_t value_count = version3 ? 10 : (version2 ? 8 : 6);
  std::size_t offset = 0;
  for (std::size_t index = 0; index < value_count; ++index) {
    const auto comma = payload.find(L',', offset);
    const bool final = index + 1 == value_count;
    if ((final && comma != std::wstring::npos) || (!final && comma == std::wstring::npos)) return false;
    const auto end = final ? payload.size() : comma;
    const int32_t minimum = index < 6 ? 1 : (index < 8 ? (version3 ? 0 : 1) : -32768);
    const int32_t maximum = index < 6 ? 1'000'000 : 32768;
    if (!parse_i32_arg(payload.substr(offset, end - offset).c_str(), minimum, maximum, values[index])) return false;
    offset = end + 1;
  }
  g_downsample_x = {values[0], static_cast<uint32_t>(values[1])};
  g_downsample_y = {values[2], static_cast<uint32_t>(values[3])};
  g_pixel_aspect_ratio = {values[4], static_cast<uint32_t>(values[5])};
  g_full_resolution_width = version2 || version3 ? values[6] : 0;
  g_full_resolution_height = version2 || version3 ? values[7] : 0;
  g_pre_effect_source_origin_x = version3 ? values[8] : 0;
  g_pre_effect_source_origin_y = version3 ? values[9] : 0;
  if (g_full_resolution_width > 32768 || g_full_resolution_height > 32768) return false;
  return true;
}

bool decode_active_camera_payload(const wchar_t* text,
    std::array<uint64_t, 51>& fields, std::array<double, 13>& values,
    std::array<std::array<double, 13>, 2>& keyframe_values, bool& animated) {
  if (!text) return false;
  const std::wstring encoded(text);
  constexpr wchar_t kPrefix[] = L"scene-camera:v1|";
  animated = encoded.compare(0, 16, L"scene-camera:v2|") == 0;
  if ((!animated && encoded.compare(0, std::size(kPrefix) - 1, kPrefix) != 0) ||
      encoded.size() > (animated ? 1536u : 512u)) return false;
  const std::size_t field_count = animated ? 51 : 21;
  const std::wstring payload = encoded.substr(std::size(kPrefix) - 1);
  std::size_t start = 0;
  for (std::size_t index = 0; index < field_count; ++index) {
    const std::size_t comma = payload.find(L',', start);
    const bool final = index + 1 == field_count;
    if ((final && comma != std::wstring::npos) ||
        (!final && comma == std::wstring::npos)) return false;
    const std::size_t end = final ? payload.size() : comma;
    if (start == end) return false;
    uint64_t value = 0;
    for (std::size_t cursor = start; cursor < end; ++cursor) {
      const wchar_t digit = payload[cursor];
      if (digit < L'0' || digit > L'9') return false;
      const uint64_t decimal = static_cast<uint64_t>(digit - L'0');
      if (value > (std::numeric_limits<uint64_t>::max() - decimal) / 10)
        return false;
      value = value * 10 + decimal;
    }
    fields[index] = value;
    start = end + 1;
  }
  if (fields[0] == 0 || fields[0] > INT32_MAX ||
      fields[1] == 0 || fields[1] > INT32_MAX ||
      fields[2] == 0 || fields[2] > UINT32_MAX ||
      fields[3] >= 3 || fields[4] > 10'000'000 ||
      fields[5] == 0 || fields[5] > 1'000'000 ||
      fields[6] == 0 || fields[6] > 10'000'000 ||
      fields[7] == 0 || fields[7] > 1'000'000 ||
      fields[4] * fields[7] + fields[6] * fields[5] >
          10 * fields[5] * fields[7]) return false;

  static_assert(sizeof(double) == sizeof(uint64_t));
  const auto decode = [&](std::size_t offset, auto& destination) {
    for (std::size_t index = 0; index < destination.size(); ++index) {
      std::memcpy(&destination[index], &fields[index + offset], sizeof(double));
      const double bound = index == 0 ? 1'000'000'000.0 :
          index >= 10 ? 36'000.0 :
          index >= 7 ? 10'000.0 : 1'000'000.0;
      if (!std::isfinite(destination[index]) || std::abs(destination[index]) > bound)
        return false;
    }
    if (destination[0] <= 0.0 ||
        destination[7] < 0.01 || destination[8] < 0.01 || destination[9] < 0.01)
      return false;
    const double scale_determinant =
        destination[7] * destination[8] * destination[9] / 1'000'000.0;
    return std::isfinite(scale_determinant) && scale_determinant > 1.001e-12;
  };
  if (!decode(8, values)) return false;
  if (animated) {
    for (std::size_t key = 0; key < 2; ++key) {
      const std::size_t offset = 21 + key * 15;
      if (fields[offset + 1] == 0 || fields[offset + 1] > 1'000'000 ||
          fields[offset] >= 10 * fields[offset + 1] ||
          !decode(offset + 2, keyframe_values[key])) return false;
    }
    if (fields[21] * fields[37] >= fields[36] * fields[22]) return false;
  }

  return true;
}

bool parse_authored_layer_graph(const wchar_t* text);

bool parse_active_camera_payload(const wchar_t* text) {
  if (text && (std::wstring(text).compare(0, 15, L"scene-graph:v1|") == 0 ||
      std::wstring(text).compare(0, 15, L"scene-graph:v2|") == 0 ||
      std::wstring(text).compare(0, 15, L"scene-graph:v3|") == 0))
    return parse_authored_layer_graph(text);
  std::array<uint64_t, 51> fields{};
  std::array<double, 13> values{};
  std::array<std::array<double, 13>, 2> keyframe_values{};
  bool animated = false;
  if (!decode_active_camera_payload(text, fields, values, keyframe_values, animated))
    return false;
  auto& state = aexcompat::scene_runtime::scene_runtime_state();
  if (!state.scene_registry_initialized || state.authored_camera_live ||
      state.authored_layer_graph_live)
    return false;
  const std::size_t index = static_cast<std::size_t>(fields[3]);
  scene_model::Identity identity{};
  auto& registry = scene_model::registry();
  if (!registry.identity_for_legacy(
          &state.layers[index], scene_model::ObjectKind::layer, identity))
    return false;
  const scene_model::Identity authored{
      fields[0], fields[1], static_cast<uint32_t>(fields[2]),
      scene_model::ObjectKind::layer, {}};
  if (!registry.bind_authored_layer_identity(identity, authored, identity))
    return false;
  const auto transform_from = [](const auto& snapshot) {
    aexcompat::scene_runtime::AegpLayerTransform transform{};
    for (std::size_t component = 0; component < 3; ++component) {
      transform.anchor[component] = snapshot[1 + component];
      transform.position[component] = snapshot[4 + component];
      transform.scale[component] = snapshot[7 + component];
      transform.rotation_degrees[component] = snapshot[10 + component];
    }
    transform.is_3d = true;
    return transform;
  };
  state.layer_transforms[index] = transform_from(values);
  if (animated) {
    for (std::size_t key = 0; key < 2; ++key) {
      const std::size_t offset = 21 + key * 15;
      const aexcompat::suite_abi::AegpTime time{
          static_cast<int32_t>(fields[offset]),
          static_cast<uint32_t>(fields[offset + 1])};
      state.layer_transform_keyframes[index][key] = {
          time, transform_from(keyframe_values[key]), true};
      state.layer_camera_zoom_keyframes[index][key] = {
          time, keyframe_values[key][0], true};
    }
  }
  state.layer_in_points[index] = {
      static_cast<int32_t>(fields[4]), static_cast<uint32_t>(fields[5])};
  state.layer_durations[index] = {
      static_cast<int32_t>(fields[6]), static_cast<uint32_t>(fields[7])};
  state.layer_camera_zoom[index] = values[0];
  state.authored_camera_identity = identity;
  state.authored_camera_live = true;
  state.active_camera_layer_index = static_cast<int32_t>(index);
  return true;
}

bool parse_authored_layer_graph(const wchar_t* text) {
  using scene_model::Identity;
  using scene_model::ObjectKind;
  using aexcompat::scene_runtime::AegpLayerTransform;
  if (!text) return false;
  const std::wstring encoded(text);
  const bool layer_animated = encoded.compare(0, 15, L"scene-graph:v3|") == 0;
  const bool oriented = layer_animated || encoded.compare(0, 15, L"scene-graph:v2|") == 0;
  if (encoded.size() > 4096 || (!oriented && encoded.compare(0, 15, L"scene-graph:v1|") != 0))
    return false;
  const auto separator = encoded.find(L'!', 15);
  if (separator == std::wstring::npos || encoded.find(L'!', separator + 1) != std::wstring::npos)
    return false;
  const std::wstring camera_text = encoded.substr(15, separator - 15);
  std::array<uint64_t, 51> camera_fields{};
  std::array<double, 13> camera_values{};
  std::array<std::array<double, 13>, 2> camera_keys{};
  bool animated = false;
  const bool has_camera = !camera_text.empty();
  if (has_camera && !decode_active_camera_payload(camera_text.c_str(),
          camera_fields, camera_values, camera_keys, animated)) return false;
  std::array<Identity, 3> authored{};
  std::array<Identity, 3> current{};
  std::array<Identity, 3> parents{};
  std::array<std::size_t, 3> slots{};
  std::array<std::size_t, 3> encoded_parent_slots{};
  std::array<AegpLayerTransform, 3> transforms{};
  std::array<std::array<aexcompat::scene_runtime::AegpLayerTransformKeyframe, 2>, 3> layer_keys{};
  std::array<aexcompat::suite_abi::AegpTime, 7> breakpoints{};
  breakpoints[0] = {0, 1};
  std::size_t breakpoint_count = 1;
  std::array<bool, 3> occupied{};
  std::size_t count = 0;
  std::size_t start = separator + 1;
  const auto transform_from = [](const auto& snapshot, bool is_3d) {
    AegpLayerTransform result{};
    for (std::size_t component = 0; component < 3; ++component) {
      result.anchor[component] = snapshot[1 + component];
      result.position[component] = snapshot[4 + component];
      result.scale[component] = snapshot[7 + component];
      result.rotation_degrees[component] = snapshot[10 + component];
    }
    result.is_3d = is_3d;
    return result;
  };
  const auto decode_transform = [&](const uint64_t* fields, bool is_3d,
      bool has_orientation, AegpLayerTransform& output) {
    std::array<double, 13> values{};
    values[0] = 1.0;
    for (std::size_t component = 1; component < values.size(); ++component) {
      std::memcpy(&values[component], &fields[component - 1], sizeof(double));
      const double limit = component >= 10 ? 36'000.0 :
          component >= 7 ? 10'000.0 : 1'000'000.0;
      if (!std::isfinite(values[component]) || std::abs(values[component]) > limit)
        return false;
    }
    for (std::size_t component = 7; component < 10; ++component)
      if (values[component] < 0.01) return false;
    const double determinant = values[7] * values[8] * values[9] / 1'000'000.0;
    if (!std::isfinite(determinant) || determinant <= 1.001e-12) return false;
    if (!is_3d && (values[3] != 0.0 || values[6] != 0.0 ||
        values[9] != 100.0 || values[10] != 0.0 || values[11] != 0.0)) return false;
    output = transform_from(values, is_3d);
    if (has_orientation) {
      for (std::size_t component = 0; component < 3; ++component) {
        double orientation = 0.0;
        std::memcpy(&orientation, &fields[12 + component], sizeof(double));
        if (!std::isfinite(orientation) || std::abs(orientation) > 36'000.0 ||
            (!is_3d && orientation != 0.0)) return false;
        output.orientation_degrees[component] = orientation;
      }
    }
    return true;
  };
  while (start < encoded.size()) {
    if (count >= 3 - static_cast<std::size_t>(has_camera)) return false;
    const auto delimiter = encoded.find(L';', start);
    const auto end = delimiter == std::wstring::npos ? encoded.size() : delimiter;
    const std::wstring record = encoded.substr(start, end - start);
    std::array<uint64_t, 59> fields{};
    const std::size_t field_count = layer_animated ? 59 : oriented ? 24 : 21;
    std::size_t offset = 0;
    for (std::size_t field = 0; field < field_count; ++field) {
      const auto comma = record.find(L',', offset);
      const bool final = field + 1 == field_count;
      if ((final && comma != std::wstring::npos) ||
          (!final && comma == std::wstring::npos)) return false;
      const auto field_end = final ? record.size() : comma;
      if (field_end == offset) return false;
      uint64_t value = 0;
      for (std::size_t cursor = offset; cursor < field_end; ++cursor) {
        const wchar_t digit = record[cursor];
        if (digit < L'0' || digit > L'9') return false;
        const uint64_t decimal = static_cast<uint64_t>(digit - L'0');
        if (value > (UINT64_MAX - decimal) / 10) return false;
        value = value * 10 + decimal;
      }
      fields[field] = value;
      offset = field_end + 1;
    }
    if (fields[0] == 0 || fields[0] > INT32_MAX ||
        fields[1] == 0 || fields[1] > INT32_MAX ||
        fields[2] == 0 || fields[2] > UINT32_MAX || fields[3] >= 3 ||
        fields[4] > INT32_MAX || fields[5] > INT32_MAX ||
        fields[6] > UINT32_MAX || fields[7] >= 3 || fields[8] > 1)
      return false;
    const bool no_parent = fields[4] == 0 && fields[5] == 0 &&
        fields[6] == 0 && fields[7] == 0;
    if (!no_parent && (fields[4] == 0 || fields[5] == 0 || fields[6] == 0))
      return false;
    const auto slot = static_cast<std::size_t>(fields[3]);
    if (occupied[slot]) return false;
    occupied[slot] = true;
    slots[count] = slot;
    encoded_parent_slots[count] = static_cast<std::size_t>(fields[7]);
    authored[count] = {fields[0], fields[1], static_cast<uint32_t>(fields[2]),
        ObjectKind::layer, {}};
    if (!no_parent) parents[count] = {fields[4], fields[5],
        static_cast<uint32_t>(fields[6]), ObjectKind::layer, {}};
    if (!decode_transform(fields.data() + 9, fields[8] != 0, oriented,
            transforms[count])) return false;
    if (layer_animated) {
      if (fields[24] > 1) return false;
      if (!fields[24]) {
        for (std::size_t field = 25; field < fields.size(); ++field)
          if (fields[field] != 0) return false;
      } else {
        for (std::size_t key = 0; key < 2; ++key) {
          const std::size_t key_offset = 25 + key * 17;
          const uint64_t value = fields[key_offset];
          const uint64_t scale = fields[key_offset + 1];
          if (value > INT32_MAX || scale == 0 || scale > 1'000'000 ||
              value >= 10 * scale) return false;
          auto& snapshot = layer_keys[count][key];
          snapshot.time = {static_cast<int32_t>(value), static_cast<uint32_t>(scale)};
          if (!decode_transform(fields.data() + key_offset + 2,
                  fields[8] != 0, true, snapshot.transform)) return false;
          snapshot.valid = true;
          breakpoints[breakpoint_count++] = snapshot.time;
        }
        const auto& first = layer_keys[count][0].time;
        const auto& second = layer_keys[count][1].time;
        if (static_cast<int64_t>(first.value) * second.scale >=
            static_cast<int64_t>(second.value) * first.scale) return false;
      }
    }
    ++count;
    if (delimiter == std::wstring::npos) break;
    start = delimiter + 1;
    if (start == encoded.size()) return false;
  }
  if (count == 0) return false;
  const std::size_t ordinary_count = count;
  // Resolve parent slot AND full identity; an index alone is not ownership.
  std::array<int32_t, 3> parent_slots{{-1, -1, -1}};
  for (std::size_t index = 0; index < count; ++index) {
    if (parents[index].kind == ObjectKind::none) continue;
    std::size_t parent = 0;
    while (parent < count && authored[parent] != parents[index]) ++parent;
    if (parent == count) return false;
    if (encoded_parent_slots[index] != slots[parent]) return false;
    parent_slots[index] = static_cast<int32_t>(slots[parent]);
  }
  // Positive affine scales make log(composed determinant) concave between
  // key breakpoints. Checking every breakpoint proves the full interval.
  for (std::size_t breakpoint = 0; breakpoint < breakpoint_count; ++breakpoint) {
    for (std::size_t index = 0; index < count; ++index) {
      std::array<bool, 3> visited{};
      std::size_t cursor = index;
      double determinant = 1.0;
      for (;;) {
        if (visited[cursor]) return false;
        visited[cursor] = true;
        AegpLayerTransform evaluated{};
        if (!aexcompat::scene_runtime::sample_layer_transform(transforms[cursor],
                layer_keys[cursor], breakpoints[breakpoint], evaluated)) return false;
        const auto& scale = evaluated.scale;
        determinant *= scale[0] * scale[1] * scale[2] / 1'000'000.0;
        if (parent_slots[cursor] < 0) break;
        std::size_t parent = 0;
        while (parent < count && slots[parent] != static_cast<std::size_t>(parent_slots[cursor]))
          ++parent;
        if (parent == count) return false;
        cursor = parent;
      }
      if (!std::isfinite(determinant) || determinant <= 1.001e-12) return false;
    }
  }
  if (has_camera) {
    const auto slot = static_cast<std::size_t>(camera_fields[3]);
    if (occupied[slot]) return false;
    slots[count] = slot;
    authored[count] = {camera_fields[0], camera_fields[1],
        static_cast<uint32_t>(camera_fields[2]), ObjectKind::layer, {}};
    transforms[count] = transform_from(camera_values, true);
    ++count;
  }
  aexcompat::scene_transaction::MutationLock mutation_lock;
  auto& state = aexcompat::scene_runtime::scene_runtime_state();
  auto& registry = scene_model::registry();
  if (!state.scene_registry_initialized || state.authored_camera_live ||
      state.authored_layer_graph_live) return false;
  for (std::size_t index = 0; index < count; ++index)
    if (!registry.identity_for_legacy(&state.layers[slots[index]],
            ObjectKind::layer, current[index])) return false;
  if (!registry.bind_authored_layer_graph(current, authored, parents, count))
    return false;
  for (std::size_t index = 0; index < count; ++index) {
    const auto slot = slots[index];
    state.layer_transforms[slot] = transforms[index];
    constexpr uint32_t kLayerIs3d = 0x00000800u;
    state.layer_flags[slot] = (state.layer_flags[slot] & ~kLayerIs3d) |
        (transforms[index].is_3d ? kLayerIs3d : 0u);
    state.layer_transform_keyframes[slot] = layer_keys[index];
    state.layer_parent_indices[slot] = parent_slots[index];
  }
  if (has_camera) {
    const auto slot = slots[ordinary_count];
    if (animated) {
      for (std::size_t key = 0; key < 2; ++key) {
        const auto offset = 21 + key * 15;
        const aexcompat::suite_abi::AegpTime time{
            static_cast<int32_t>(camera_fields[offset]),
            static_cast<uint32_t>(camera_fields[offset + 1])};
        state.layer_transform_keyframes[slot][key] = {
            time, transform_from(camera_keys[key], true), true};
        state.layer_camera_zoom_keyframes[slot][key] = {time, camera_keys[key][0], true};
      }
    }
    state.layer_in_points[slot] = {static_cast<int32_t>(camera_fields[4]),
        static_cast<uint32_t>(camera_fields[5])};
    state.layer_durations[slot] = {static_cast<int32_t>(camera_fields[6]),
        static_cast<uint32_t>(camera_fields[7])};
    state.layer_camera_zoom[slot] = camera_values[0];
    state.authored_camera_identity = authored[ordinary_count];
    state.authored_camera_live = true;
    state.active_camera_layer_index = static_cast<int32_t>(slot);
  }
  state.authored_layer_graph_live = true;
  return true;
}

bool parse_render_environment_payload(const wchar_t* text) {
  if (!text) return false;
  const std::wstring encoded(text);
  if (encoded.compare(0, 10, L"render:v1|") != 0 || encoded.size() > 96) return false;
  const std::wstring payload = encoded.substr(10);
  std::array<int32_t, 4> values{};
  std::size_t offset = 0;
  for (std::size_t index = 0; index < values.size(); ++index) {
    const auto comma = payload.find(L',', offset);
    const bool final = index + 1 == values.size();
    if ((final && comma != std::wstring::npos) || (!final && comma == std::wstring::npos)) return false;
    const auto end = final ? payload.size() : comma;
    if (!parse_i32_arg(payload.substr(offset, end - offset).c_str(),
                       index == 3 ? -65536 : 0,
                       index < 2 ? (index == 0 ? 1 : 2) : 65536,
                       values[index])) return false;
    offset = end + 1;
  }
  g_render_quality = values[0];
  g_render_field = values[1];
  g_shutter_angle = values[2];
  g_shutter_phase = values[3];
  return true;
}

bool valid_parameter_id(const std::wstring& id) {
  if (id.empty() || id.size() > 64 || id.front() < L'a' || id.front() > L'z') return false;
  return std::all_of(id.begin(), id.end(), [](wchar_t character) {
    return (character >= L'a' && character <= L'z') ||
           (character >= L'0' && character <= L'9') || character == L'_';
  });
}

bool parse_parameter_payload(const wchar_t* text, RequestedAssignments& output) {
  if (!text) return false;
  const std::wstring encoded(text);
  const bool version5 = encoded.compare(0, 3, L"v5|") == 0;
  const bool version4 = encoded.compare(0, 3, L"v4|") == 0;
  const bool version3 = encoded.compare(0, 3, L"v3|") == 0;
  if (encoded.size() < 3 || encoded.size() > 16384 ||
      (!version5 && !version4 && !version3 && encoded.compare(0, 3, L"v2|") != 0)) return false;
  const std::wstring payload = encoded.substr(3);
  if (payload.empty()) return true;
  std::unordered_set<std::wstring> seen_ids;
  std::unordered_set<int32_t> seen_indices;
  std::size_t offset = 0;
  while (offset < payload.size()) {
    const std::size_t separator = payload.find(L';', offset);
    const std::size_t end = separator == std::wstring::npos ? payload.size() : separator;
    const std::wstring assignment = payload.substr(offset, end - offset);
    const std::size_t at = assignment.find(L'@');
    const std::size_t colon = assignment.find(L':', at == std::wstring::npos ? 0 : at + 1);
    const std::size_t equals = assignment.find(L'=', colon == std::wstring::npos ? 0 : colon + 1);
    if (at == std::wstring::npos || colon == std::wstring::npos || equals == std::wstring::npos ||
        at == 0 || colon <= at + 1 || equals <= colon + 1 || equals + 1 >= assignment.size() ||
        assignment.find(L'=', equals + 1) != std::wstring::npos) return false;
    const std::wstring id = assignment.substr(0, at);
    const std::wstring index_text = assignment.substr(at + 1, colon - at - 1);
    const std::wstring kind_text = assignment.substr(colon + 1, equals - colon - 1);
    const std::wstring value = assignment.substr(equals + 1);
    int32_t index{};
    if (!valid_parameter_id(id) || value.size() > 8192 ||
        !parse_i32_arg(index_text.c_str(), 1, static_cast<int32_t>(kMaxParams), index) ||
        !seen_ids.insert(id).second || !seen_indices.insert(index).second) return false;
    RequestedKind kind{};
    if (kind_text == L"i32") kind = RequestedKind::Integer;
    else if (kind_text == L"f64") kind = RequestedKind::Float;
    else if ((version3 || version4 || version5) && kind_text == L"argb8") kind = RequestedKind::Color;
    else if ((version4 || version5) && kind_text == L"angle") kind = RequestedKind::Angle;
    else if ((version4 || version5) && kind_text == L"point") kind = RequestedKind::Point;
    else if ((version4 || version5) && kind_text == L"point3d") kind = RequestedKind::Point3D;
    else if (version5 && kind_text == L"arbhex") kind = RequestedKind::ArbitraryText;
    else return false;
    double parsed{};
    std::array<unsigned char, 4> color{};
    std::array<double, 3> components{};
    std::string arbitrary_text;
    if (kind == RequestedKind::Color) {
      std::size_t start = 0;
      for (std::size_t channel = 0; channel < color.size(); ++channel) {
        const std::size_t comma = value.find(L',', start);
        const bool final_channel = channel + 1 == color.size();
        if ((final_channel && comma != std::wstring::npos) ||
            (!final_channel && comma == std::wstring::npos)) return false;
        const std::size_t finish = final_channel ? value.size() : comma;
        int32_t component{};
        if (!parse_i32_arg(value.substr(start, finish - start).c_str(), 0, 255, component))
          return false;
        color[channel] = static_cast<unsigned char>(component);
        start = finish + 1;
      }
    } else if (kind == RequestedKind::Angle || kind == RequestedKind::Point || kind == RequestedKind::Point3D) {
      const std::size_t count = kind == RequestedKind::Point3D ? 3 : (kind == RequestedKind::Point ? 2 : 1);
      std::size_t start = 0;
      for (std::size_t component = 0; component < count; ++component) {
        const std::size_t comma = value.find(L',', start);
        const bool final_component = component + 1 == count;
        if ((final_component && comma != std::wstring::npos) || (!final_component && comma == std::wstring::npos)) return false;
        const std::size_t finish = final_component ? value.size() : comma;
        if (!parse_double_arg(value.substr(start, finish - start).c_str(), -32768.0, 32768.0, components[component])) return false;
        start = finish + 1;
      }
    } else if (kind == RequestedKind::ArbitraryText) {
      if (value.empty() || value.size() > 8192 || value.size() % 2 != 0) return false;
      arbitrary_text.reserve(value.size() / 2);
      const auto nibble = [](wchar_t ch) -> int {
        if (ch >= L'0' && ch <= L'9') return ch - L'0';
        if (ch >= L'a' && ch <= L'f') return ch - L'a' + 10;
        return -1;
      };
      for (std::size_t pos = 0; pos < value.size(); pos += 2) {
        const int high = nibble(value[pos]), low = nibble(value[pos + 1]);
        if (high < 0 || low < 0) return false;
        arbitrary_text.push_back(static_cast<char>((high << 4) | low));
      }
      if (arbitrary_text.empty() || arbitrary_text.size() > 4096 ||
          arbitrary_text.find('\0') != std::string::npos) return false;
    } else {
      const double minimum = kind == RequestedKind::Integer
          ? static_cast<double>((std::numeric_limits<int32_t>::min)())
          : -(std::numeric_limits<double>::max)();
      const double maximum = kind == RequestedKind::Integer
          ? static_cast<double>((std::numeric_limits<int32_t>::max)())
          : (std::numeric_limits<double>::max)();
      if (!parse_double_arg(value.c_str(), minimum, maximum, parsed) ||
          (kind == RequestedKind::Integer && std::trunc(parsed) != parsed)) return false;
    }
    output.push_back({id, index, kind, parsed, color, components, arbitrary_text});
    if (output.size() > kMaxParams) return false;
    if (separator == std::wstring::npos) break;
    offset = separator + 1;
    if (offset == payload.size()) return false;
  }
  return true;
}

}  // namespace aexcompat::l2_detail
