#include "worker_request_parser.hpp"

#include "render_subsystem.h"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_parameter_runtime.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <cwchar>
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
constexpr std::size_t kMaxParams = 1024;
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
