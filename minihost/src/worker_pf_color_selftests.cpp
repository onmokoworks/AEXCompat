#include "worker_pf_color_selftests.hpp"

#include "worker_pf_suites_internal.hpp"

#include <array>
#include <cstddef>
#include <cstring>
#include <limits>

namespace aexcompat::pf_color_selftests {
namespace {

constexpr int32_t kPfBadCallbackParam = 516;
constexpr std::size_t kParamSize = worker_runtime::parameters::kDefinitionSize;
constexpr std::size_t kParamType = 12;

Hooks g_hooks{};

template <typename T>
void write(std::array<std::byte, kParamSize>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

}  // namespace

void configure(Hooks hooks) { g_hooks = hooks; }

bool verify_pf_color_suite() {
  const void* acquired8{}; const void* acquired16{}; const void* acquired_float{};
  const bool acquired = g_hooks.acquire_suite("PF Color Suite", 1, &acquired8) == 0 &&
      g_hooks.acquire_suite("PF Color16 Suite", 1, &acquired16) == 0 &&
      g_hooks.acquire_suite("PF ColorFloat Suite", 1, &acquired_float) == 0 &&
      acquired8 == &g_color_suite8 && acquired16 == &g_color_suite16 &&
      acquired_float == &g_color_suite_float;
  PfPixel8 red8{77, 255, 0, 0}, round8{91, 0, 0, 0};
  PfFixed hls[3]{}, yiq[3]{};
  int32_t lum8{}, hue8{}, light8{}, sat8{};
  bool ok = acquired && g_color_suite8.RGBtoHLS(nullptr, &red8, hls) == 0 &&
      hls[0] == 0 && hls[1] == pf_color_to_fixed(0.5) && hls[2] == pf_color_to_fixed(1.0) &&
      g_color_suite8.HLStoRGB(nullptr, hls, &round8) == 0 && round8.alpha == 91 &&
      round8.red == 255 && round8.green <= 1 && round8.blue <= 1 &&
      g_color_suite8.RGBtoYIQ(nullptr, &red8, yiq) == 0 &&
      g_color_suite8.Luminance(nullptr, &red8, &lum8) == 0 &&
      g_color_suite8.Hue(nullptr, &red8, &hue8) == 0 &&
      g_color_suite8.Lightness(nullptr, &red8, &light8) == 0 &&
      g_color_suite8.Saturation(nullptr, &red8, &sat8) == 0 &&
      lum8 == 7622 && hue8 == 0 && light8 == 128 && sat8 == 255;
  PfPixel16 green16{1234, 0, 32768, 0}, round16{4321, 0, 0, 0};
  ok = ok && g_color_suite16.RGBtoHLS(nullptr, &green16, hls) == 0 &&
      hls[0] == pf_color_to_fixed(120.0) &&
      g_color_suite16.HLStoRGB(nullptr, hls, &round16) == 0 && round16.alpha == 4321 &&
      round16.green >= 32767 && round16.red <= 1 && round16.blue <= 1;
  int32_t hue16{};
  ok = ok && g_color_suite16.Hue(nullptr, &green16, &hue16) == 0 && hue16 == 85;
  PfPixelFloat hdr{2.0f, 1.5f, 2.0f, -1.0f};
  float lumf{};
  ok = ok && g_color_suite_float.RGBtoYIQ(nullptr, &hdr, yiq) == 0 &&
      yiq[0] > 65536 && g_color_suite_float.Luminance(nullptr, &hdr, &lumf) == 0 &&
      lumf > 1.0f;
  PfPixelFloat invalid{1.0f, std::numeric_limits<float>::infinity(), 0.0f, 0.0f};
  PfFixed sentinel[3]{11, 22, 33};
  ok = ok && g_color_suite_float.RGBtoHLS(nullptr, &invalid, sentinel) == kPfBadCallbackParam &&
      sentinel[0] == 11 && sentinel[1] == 22 && sentinel[2] == 33 &&
      g_color_suite8.RGBtoHLS(nullptr, nullptr, sentinel) == kPfBadCallbackParam &&
      g_color_suite8.RGBtoHLS(nullptr, &red8, nullptr) == kPfBadCallbackParam;
  ok = g_hooks.release_suite("PF ColorFloat Suite", 1) == 0 &&
      g_hooks.release_suite("PF Color16 Suite", 1) == 0 &&
      g_hooks.release_suite("PF Color Suite", 1) == 0 && ok;
  return ok;
}

bool verify_pf_color_param_suite() {
  auto& params = *g_hooks.params;
  const auto saved_params = params;
  params.clear();
  auto add_color = [&params](int32_t disk_id, std::array<unsigned char, 4> current8,
                             std::array<unsigned char, 4> default8,
                             std::array<float, 4> current_float,
                             std::array<float, 4> default_float) {
    ParamRecord record{};
    record.index = static_cast<int32_t>(params.size() + 1);
    record.disk_id = disk_id;
    record.type = 5;
    record.has_color = true;
    record.current_color = current8;
    record.default_color = default8;
    record.current_float_color = current_float;
    record.default_float_color = default_float;
    params.push_back(record);
  };
  add_color(101, {255, 64, 128, 192}, {128, 10, 20, 30},
            {1.0f, 64.0f / 255.0f, 128.0f / 255.0f, 192.0f / 255.0f},
            {128.0f / 255.0f, 10.0f / 255.0f, 20.0f / 255.0f, 30.0f / 255.0f});
  add_color(102, {255, 17, 33, 65}, {255, 1, 2, 3},
            {1.0f, 4097.0f / 32768.0f, 8193.0f / 32768.0f, 16385.0f / 32768.0f},
            {1.0f, 1.0f / 32768.0f, 2.0f / 32768.0f, 3.0f / 32768.0f});
  add_color(103, {255, 200, 100, 50}, {255, 40, 50, 60},
            {0.75f, 1.5f, -0.25f, 2.0f}, {1.0f, 0.1f, 0.2f, 0.3f});
  // #1060 collision fixture: a POINT and a COLOUR share disk_id 0, exactly as
  // Beam leaves every parameter's uu.id. A by-id lookup for the colour finds
  // the POINT first, whose type is not 5, so the old code answered
  // PF_Err_UNRECOGNIZED_PARAM_TYPE for a valid colour. Value matching must skip
  // the POINT and resolve the colour.
  {
    ParamRecord point{};
    point.index = static_cast<int32_t>(params.size() + 1);
    point.disk_id = 0;
    point.type = 6;  // POINT: no colour payload
    point.has_color = false;
    params.push_back(point);
  }
  add_color(0, {50, 60, 70, 80}, {1, 2, 3, 4},
            {50.0f / 255.0f, 60.0f / 255.0f, 70.0f / 255.0f, 80.0f / 255.0f},
            {1.0f / 255.0f, 2.0f / 255.0f, 3.0f / 255.0f, 4.0f / 255.0f});

  const void* acquired = nullptr;
  bool ok = g_hooks.acquire_suite("PF ColorParamSuite", 1, &acquired) == 0 &&
      acquired == g_hooks.color_param_suite1;
  auto definition = [](const ParamRecord& record, bool current) {
    std::array<std::byte, kParamSize> bytes{};
    write<int32_t>(bytes, 0, record.disk_id);
    write<int32_t>(bytes, kParamType, record.type);
    const auto& color = current ? record.current_color : record.default_color;
    std::memcpy(bytes.data() + 56, color.data(), color.size());
    return bytes;
  };
  PixelFloat out{};
  auto current8 = definition(params[0], true);
  auto default8 = definition(params[0], false);
  ok = ok && g_hooks.floating_point_from_color(g_hooks.effect, current8.data(), &out) == 0 &&
      out.alpha == 1.0f && out.red == 64.0f / 255.0f &&
      out.green == 128.0f / 255.0f && out.blue == 192.0f / 255.0f &&
      g_hooks.floating_point_from_color(g_hooks.effect, default8.data(), &out) == 0 &&
      out.alpha == 128.0f / 255.0f && out.red == 10.0f / 255.0f;
  auto current16 = definition(params[1], true);
  ok = ok && g_hooks.floating_point_from_color(g_hooks.effect, current16.data(), &out) == 0 &&
      out.red == 4097.0f / 32768.0f && out.green == 8193.0f / 32768.0f &&
      out.blue == 16385.0f / 32768.0f;
  auto current_float = definition(params[2], true);
  ok = ok && g_hooks.floating_point_from_color(g_hooks.effect, current_float.data(), &out) == 0 &&
      out.alpha == 0.75f && out.red == 1.5f && out.green == -0.25f && out.blue == 2.0f;

  // #1060 regression: the colour fixture carries disk_id 0, shared with the
  // POINT that precedes it. A by-id lookup lands on the POINT and (old code)
  // returned kPfUnrecognizedParamType; value matching skips it and resolves the
  // colour's float.
  auto collided = definition(params[4], true);
  ok = ok && g_hooks.floating_point_from_color(g_hooks.effect, collided.data(), &out) == 0 &&
      out.alpha == 50.0f / 255.0f && out.red == 60.0f / 255.0f &&
      out.green == 70.0f / 255.0f && out.blue == 80.0f / 255.0f;

  // #1060 core: an unknown disk_id must still resolve by value, since the id is
  // no longer the lookup key. by_id short-circuits to end() and the value scan
  // carries it to the same colour.
  auto unknown_id = collided;
  write<int32_t>(unknown_id, 0, 7777);
  ok = ok && g_hooks.floating_point_from_color(g_hooks.effect, unknown_id.data(), &out) == 0 &&
      out.alpha == 50.0f / 255.0f && out.red == 60.0f / 255.0f &&
      out.green == 70.0f / 255.0f && out.blue == 80.0f / 255.0f;

  const PixelFloat sentinel{9.0f, 8.0f, 7.0f, 6.0f};
  out = sentinel;
  // A type-5 definition whose value matches no colour parameter is the only
  // remaining error path: kPfBadCallbackParam, output untouched. (The former
  // "unknown disk_id -> kPfInvalidIndex" contract is gone: the id is no longer
  // the lookup key, so an unknown id that still carries a known value resolves.)
  auto unmatched = current8;
  const std::array<unsigned char, 4> stranger{3, 5, 7, 9};
  std::memcpy(unmatched.data() + 56, stranger.data(), stranger.size());
  auto invalid_type = current8;
  write<int32_t>(invalid_type, kParamType, 6);
  ok = ok && g_hooks.floating_point_from_color(nullptr, current8.data(), &out) == kPfBadCallbackParam &&
      g_hooks.floating_point_from_color(g_hooks.effect, nullptr, &out) == kPfBadCallbackParam &&
      g_hooks.floating_point_from_color(g_hooks.effect, current8.data(), nullptr) == kPfBadCallbackParam &&
      g_hooks.floating_point_from_color(g_hooks.effect, unmatched.data(), &out) == kPfBadCallbackParam &&
      g_hooks.floating_point_from_color(g_hooks.effect, invalid_type.data(), &out) ==
          kPfUnrecognizedParamType && std::memcmp(&out, &sentinel, sizeof(out)) == 0;
  ok = g_hooks.release_suite("PF ColorParamSuite", 1) == 0 && ok;
  params = saved_params;
  return ok;
}

}  // namespace aexcompat::pf_color_selftests
