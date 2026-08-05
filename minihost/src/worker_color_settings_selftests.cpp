#include "worker_color_settings_selftests.hpp"
#include "worker_color_settings_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_world_registry.hpp"

#include <array>
#include <cstring>
#include <limits>
#include <string>

namespace aexcompat::color_settings::selftests {
namespace { Hooks g_hooks; }
void configure(Hooks hooks) { g_hooks = hooks; }
using namespace aexcompat::color_settings;
using namespace aexcompat::worker_runtime::handles;
using namespace aexcompat::world_registry;

#define acquire_suite g_hooks.acquire_suite
#define release_suite g_hooks.release_suite
bool verify_pf_color_settings_suite6() {
  reset_working_space_to_srgb();
  if (!color_settings_profiles_balanced() || !aegp_memory_balanced()) return false;
  const void* acquired = nullptr;
  if (acquire_suite("PF Color Settings Suite", 7, &acquired) != 0 ||
      acquired != &g_color_settings_suite6) return false;
  const auto& suite = g_color_settings_suite6;
  if (!suite.get_blending_tables || !suite.does_view_have_xform || !suite.xform_working_to_view ||
      !suite.get_new_working_space_profile || !suite.get_new_profile_from_icc ||
      !suite.get_new_icc_from_profile || !suite.get_new_profile_description ||
      !suite.dispose_profile || !suite.get_profile_approximate_gamma || !suite.is_rgb_profile ||
      !suite.set_working_color_space || !suite.is_ocio_used ||
      !suite.get_ocio_configuration_file || !suite.get_ocio_configuration_file_path ||
      !suite.get_ocio_working_colorspace || !suite.get_ocio_display_colorspace ||
      !suite.is_colorspace_aware_effects_enabled || !suite.get_lut_interpolation_method ||
      !suite.get_graphics_white_luminance || !suite.get_working_colorspace_id) return false;
  void* blending = reinterpret_cast<void*>(1);
  if (suite.get_blending_tables(nullptr, &blending) == 0 || blending != nullptr) return false;
  uint8_t has_xform = 2;
  if (suite.does_view_have_xform(&g_aegp_item_view, &has_xform) != 0 || has_xform != 0) return false;
  void* working = nullptr;
  if (suite.get_new_working_space_profile(1, g_hooks.composition, &working) != 0 || !working) return false;
  float gamma = 0.0f;
  uint8_t is_rgb = 0;
  if (suite.get_profile_approximate_gamma(working, &gamma) != 0 || gamma != 2.2f ||
      suite.is_rgb_profile(working, &is_rgb) != 0 || is_rgb != 1) return false;
  void* icc_handle = nullptr;
  if (suite.get_new_icc_from_profile(1, working, &icc_handle) != 0 || !icc_handle) return false;
  void* icc_bytes = nullptr;
  uint32_t icc_size = 0;
  if (lock_aegp_mem_handle(icc_handle, &icc_bytes) != 0 || !icc_bytes ||
      get_aegp_mem_handle_size(icc_handle, &icc_size) != 0 ||
      icc_size != color_settings_builtin_srgb_icc().size() ||
      std::memcmp(icc_bytes, color_settings_builtin_srgb_icc().data(), icc_size) != 0 ||
      unlock_aegp_mem_handle(icc_handle) != 0 || free_aegp_mem_handle(icc_handle) != 0) return false;
  void* desc_handle = nullptr;
  if (suite.get_new_profile_description(1, working, &desc_handle) != 0 || !desc_handle) return false;
  void* desc_bytes = nullptr;
  const std::u16string expected_desc = u"sRGB IEC61966-2.1";
  if (lock_aegp_mem_handle(desc_handle, &desc_bytes) != 0 || !desc_bytes ||
      std::memcmp(desc_bytes, expected_desc.c_str(),
                  (expected_desc.size() + 1) * sizeof(char16_t)) != 0 ||
      unlock_aegp_mem_handle(desc_handle) != 0 || free_aegp_mem_handle(desc_handle) != 0) return false;
  void* imported = nullptr;
  const auto& linear_icc = color_settings_builtin_linear_icc();
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                     linear_icc.data(), &imported) != 0 || !imported) return false;
  if (suite.set_working_color_space(1, g_hooks.composition, imported) != 0) return false;
  has_xform = 0;
  if (suite.does_view_have_xform(&g_aegp_item_view, &has_xform) != 0 || has_xform != 1) return false;
  AegpGuidValue guid{};
  if (suite.get_working_colorspace_id(1, &guid) != 0 || guid.bytes != kWorkingLinearSrgbGuid) return false;
  uint8_t ocio_used = 1;
  uint8_t aware = 1;
  uint16_t lut = 9;
  uint16_t white = 9;
  void* ocio_config = reinterpret_cast<void*>(1);
  void* ocio_path = reinterpret_cast<void*>(1);
  void* ocio_working = reinterpret_cast<void*>(1);
  void* ocio_display = reinterpret_cast<void*>(1);
  void* ocio_view = reinterpret_cast<void*>(1);
  if (suite.is_ocio_used(1, &ocio_used) != 0 || ocio_used != 0 ||
      suite.is_colorspace_aware_effects_enabled(1, &aware) != 0 || aware != 0 ||
      suite.get_lut_interpolation_method(1, &lut) != 0 || lut != 0 ||
      suite.get_graphics_white_luminance(1, &white) != 0 || white != 0 ||
      suite.get_ocio_configuration_file(1, &ocio_config) != 0 || !ocio_config ||
      suite.get_ocio_configuration_file_path(1, &ocio_path) != 0 || !ocio_path ||
      suite.get_ocio_working_colorspace(1, &ocio_working) != 0 || !ocio_working ||
      suite.get_ocio_display_colorspace(1, &ocio_display, &ocio_view) != 0 ||
      !ocio_display || !ocio_view) return false;
  for (void* handle : {ocio_config, ocio_path, ocio_working, ocio_display, ocio_view}) {
    void* empty = nullptr;
    uint32_t empty_size = 99;
    if (lock_aegp_mem_handle(handle, &empty) != 0 || !empty ||
        get_aegp_mem_handle_size(handle, &empty_size) != 0 || empty_size != sizeof(char16_t) ||
        unlock_aegp_mem_handle(handle) != 0 || free_aegp_mem_handle(handle) != 0) return false;
  }
  const uint8_t malformed[] = {0x00, 0x00, 0x00, 0x10, 'b', 'a', 'd', '!', 'b', 'a', 'd', '!', 'b', 'a', 'd', '!'};
  void* rejected = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(sizeof(malformed)), malformed,
                                     &rejected) == 0 || rejected != nullptr) return false;
  if (suite.dispose_profile(nullptr) == 0 || suite.dispose_profile(working) != 0 ||
      suite.dispose_profile(working) == 0 || suite.dispose_profile(imported) != 0) return false;
  float foreign_gamma = 99.0f;
  uint8_t foreign_rgb = 1;
  void* foreign = reinterpret_cast<void*>(static_cast<uintptr_t>(0x12347));
  if (suite.get_profile_approximate_gamma(foreign, &foreign_gamma) == 0 || foreign_gamma != 0.0f ||
      suite.is_rgb_profile(foreign, &foreign_rgb) == 0 || foreign_rgb != 0 ||
      suite.dispose_profile(foreign) == 0) return false;
  auto overflowing_icc = linear_icc;
  color_settings_write_be32(overflowing_icc, 136, static_cast<uint32_t>(overflowing_icc.size() - 2));
  color_settings_write_be32(overflowing_icc, 140, 20);
  rejected = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(overflowing_icc.size()),
                                     overflowing_icc.data(), &rejected) == 0 || rejected != nullptr)
    return false;
  std::array<void*, kMaxColorProfiles> bounded_profiles{};
  for (auto& profile : bounded_profiles) {
    if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                       linear_icc.data(), &profile) != 0 || !profile) return false;
  }
  void* over_capacity = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                     linear_icc.data(), &over_capacity) == 0 || over_capacity != nullptr)
    return false;
  for (void* profile : bounded_profiles)
    if (suite.dispose_profile(profile) != 0) return false;
  void** oversized_world = reinterpret_cast<void**>(1);
  if (aegp_world_new_owned(1, 3, (std::numeric_limits<int32_t>::max)(), 2,
                           &oversized_world) == 0 || oversized_world != nullptr) return false;
  auto fill_world = [](void*** handle, int32_t type, int32_t width, int32_t height,
                       auto fill_pixel) {
    if (aegp_world_new_owned(1, type, width, height, handle) != 0 || !*handle) return false;
    void* base = nullptr;
    if ((type == 1 && aegp_world_get_base_addr8(*handle, &base) != 0) ||
        (type == 2 && aegp_world_get_base_addr16(*handle, &base) != 0) ||
        (type == 3 && aegp_world_get_base_addr32(*handle, &base) != 0) || !base) return false;
    fill_pixel(base, width, height);
    return true;
  };
  void** world8 = nullptr;
  void** world16 = nullptr;
  void** world32 = nullptr;
  if (!fill_world(&world8, 1, 2, 1, [](void* base, int32_t width, int32_t) {
        auto* pixels = static_cast<uint8_t*>(base);
        pixels[0] = 200; pixels[1] = 64; pixels[2] = 32; pixels[3] = 16;
        pixels[4] = 180; pixels[5] = 128; pixels[6] = 64; pixels[7] = 32;
      }) ||
      !fill_world(&world16, 2, 1, 2, [](void* base, int32_t, int32_t height) {
        auto* pixels = static_cast<uint16_t*>(base);
        pixels[0] = 30000; pixels[1] = 10000; pixels[2] = 5000; pixels[3] = 2500;
        pixels[4] = 20000; pixels[5] = 15000; pixels[6] = 7500; pixels[7] = 3750;
      }) ||
      !fill_world(&world32, 3, 1, 1, [](void* base, int32_t, int32_t) {
        auto* pixels = static_cast<float*>(base);
        pixels[0] = 0.5f; pixels[1] = 0.25f; pixels[2] = 0.125f; pixels[3] = 0.0625f;
      })) return false;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, world8) != 0 ||
      suite.xform_working_to_view(&g_aegp_item_view, world16, world16) != 0 ||
      suite.xform_working_to_view(&g_aegp_item_view, world32, world32) != 0) return false;
  uint8_t pixel8[8]{};
  uint16_t pixel16[8]{};
  float pixel32[4]{};
  void* base8 = nullptr; void* base16 = nullptr; void* base32 = nullptr;
  if (aegp_world_get_base_addr8(world8, &base8) != 0 || !base8 ||
      aegp_world_get_base_addr16(world16, &base16) != 0 || !base16 ||
      aegp_world_get_base_addr32(world32, &base32) != 0 || !base32) return false;
  std::memcpy(pixel8, base8, sizeof(pixel8));
  std::memcpy(pixel16, base16, sizeof(pixel16));
  std::memcpy(pixel32, base32, sizeof(pixel32));
  if (pixel8[0] != 200 || pixel8[4] != 180 || pixel16[0] != 30000 || pixel16[4] != 20000 ||
      pixel32[0] != 0.5f) return false;
  if (pixel8[1] <= 64 || pixel8[2] <= 32 || pixel8[3] <= 16 ||
      pixel16[1] <= 10000 || pixel32[1] <= 0.25f) return false;
  void** dst8 = nullptr;
  if (!fill_world(&dst8, 1, 2, 1, [](void* base, int32_t width, int32_t) {
        std::memset(base, 0xcd, static_cast<std::size_t>(width) * 4);
      })) return false;
  auto* restored8 = static_cast<uint8_t*>(base8);
  restored8[0] = 200; restored8[1] = 64; restored8[2] = 32; restored8[3] = 16;
  restored8[4] = 180; restored8[5] = 128; restored8[6] = 64; restored8[7] = 32;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, dst8) != 0) return false;
  void* dst_base8 = nullptr;
  if (aegp_world_get_base_addr8(dst8, &dst_base8) != 0 || !dst_base8 ||
      std::memcmp(dst_base8, pixel8, sizeof(pixel8)) != 0) return false;
  void** mismatch = world16;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, mismatch) == 0) return false;
  if (aegp_world_dispose(world8) != 0 || aegp_world_dispose(world16) != 0 ||
      aegp_world_dispose(world32) != 0 || aegp_world_dispose(dst8) != 0) return false;
  if (!color_settings_profiles_balanced() || !aegp_memory_balanced() ||
      release_suite("PF Color Settings Suite", 7) != 0)
    return false;

  // Versions 4 and 6 are frozen prefixes of this table and are served from it,
  // so a plug-in acquiring either gets the same pointer and reads only its own
  // length (issue #716). What can be checked here is that all three resolve to
  // one table rather than to separately maintained copies that could drift; the
  // prefix relation itself is a property of the SDK headers.
  //
  // Each acquire is released on every path out, including the failing ones, so
  // a regression here does not also leave a lease behind.
  const void* acquired4 = nullptr;
  if (acquire_suite("PF Color Settings Suite", 4, &acquired4) != 0) return false;
  bool ok = acquired4 == &g_color_settings_suite6;
  const void* acquired6 = nullptr;
  if (acquire_suite("PF Color Settings Suite", 6, &acquired6) == 0) {
    ok = ok && acquired6 == &g_color_settings_suite6;
    ok = release_suite("PF Color Settings Suite", 6) == 0 && ok;
  } else {
    ok = false;
  }
  ok = release_suite("PF Color Settings Suite", 4) == 0 && ok;
  return ok && color_settings_profiles_balanced() && aegp_memory_balanced();
}
#undef release_suite
#undef acquire_suite
}  // namespace aexcompat::color_settings::selftests

