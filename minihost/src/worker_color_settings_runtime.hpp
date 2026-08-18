#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <vector>

namespace aexcompat::color_settings {

enum class ColorProfileKind : std::uint8_t { Srgb, LinearSrgb, ImportedRgb };

struct AegpGuidValue { std::array<std::uint8_t, 16> bytes{}; };
struct AegpItemViewToken { std::uint32_t tag{0x56494557}; };

struct HostHooks {
  void* (*composition_handle)(){};
  // Whether a caller-supplied handle names this worker's composition. A plug-in
  // that walked from its layer to its comp holds a registry-borrowed handle,
  // which is not the same pointer `composition_handle` returns, so identity
  // against that one pointer is not the test (issue #894).
  bool (*is_composition_handle)(void*){};
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
};

void configure_host_hooks(HostHooks hooks);

int32_t __cdecl color_get_blending_tables(void*, void**);
int32_t __cdecl color_does_view_have_xform(void*, std::uint8_t*);
int32_t __cdecl color_xform_working_to_view(void*, void**, void**);
int32_t __cdecl color_get_new_working_space_profile(int32_t, void*, void**);
int32_t __cdecl color_get_new_profile_from_icc(int32_t, int32_t, const void*, void**);
int32_t __cdecl color_get_new_icc_from_profile(int32_t, void*, void**);
int32_t __cdecl color_get_new_profile_description(int32_t, void*, void**);
int32_t __cdecl color_dispose_profile(void*);
int32_t __cdecl color_get_profile_approximate_gamma(void*, float*);
int32_t __cdecl color_is_rgb_profile(void*, std::uint8_t*);
int32_t __cdecl color_set_working_color_space(int32_t, void*, void*);
int32_t __cdecl color_is_ocio_used(int32_t, std::uint8_t*);
int32_t __cdecl color_get_ocio_configuration_file(int32_t, void**);
int32_t __cdecl color_get_ocio_configuration_file_path(int32_t, void**);
int32_t __cdecl color_get_ocio_working_colorspace(int32_t, void**);
int32_t __cdecl color_get_ocio_display_colorspace(int32_t, void**, void**);
int32_t __cdecl color_is_colorspace_aware_effects_enabled(int32_t, std::uint8_t*);
int32_t __cdecl color_get_lut_interpolation_method(int32_t, std::uint16_t*);
int32_t __cdecl color_get_graphics_white_luminance(int32_t, std::uint16_t*);
int32_t __cdecl color_get_working_colorspace_id(int32_t, AegpGuidValue*);

struct AegpColorSettingsSuite6 {
  decltype(&color_get_blending_tables) get_blending_tables;
  decltype(&color_does_view_have_xform) does_view_have_xform;
  decltype(&color_xform_working_to_view) xform_working_to_view;
  decltype(&color_get_new_working_space_profile) get_new_working_space_profile;
  decltype(&color_get_new_profile_from_icc) get_new_profile_from_icc;
  decltype(&color_get_new_icc_from_profile) get_new_icc_from_profile;
  decltype(&color_get_new_profile_description) get_new_profile_description;
  decltype(&color_dispose_profile) dispose_profile;
  decltype(&color_get_profile_approximate_gamma) get_profile_approximate_gamma;
  decltype(&color_is_rgb_profile) is_rgb_profile;
  decltype(&color_set_working_color_space) set_working_color_space;
  decltype(&color_is_ocio_used) is_ocio_used;
  decltype(&color_get_ocio_configuration_file) get_ocio_configuration_file;
  decltype(&color_get_ocio_configuration_file_path) get_ocio_configuration_file_path;
  decltype(&color_get_ocio_working_colorspace) get_ocio_working_colorspace;
  decltype(&color_get_ocio_display_colorspace) get_ocio_display_colorspace;
  decltype(&color_is_colorspace_aware_effects_enabled) is_colorspace_aware_effects_enabled;
  decltype(&color_get_lut_interpolation_method) get_lut_interpolation_method;
  decltype(&color_get_graphics_white_luminance) get_graphics_white_luminance;
  decltype(&color_get_working_colorspace_id) get_working_colorspace_id;
};

static_assert(sizeof(AegpColorSettingsSuite6) == 20 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_blending_tables) == 0 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, does_view_have_xform) == 1 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, xform_working_to_view) == 2 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_working_space_profile) == 3 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_profile_from_icc) == 4 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_icc_from_profile) == 5 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_profile_description) == 6 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, dispose_profile) == 7 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_profile_approximate_gamma) == 8 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_rgb_profile) == 9 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, set_working_color_space) == 10 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_ocio_used) == 11 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_configuration_file) == 12 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_configuration_file_path) == 13 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_working_colorspace) == 14 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_display_colorspace) == 15 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_colorspace_aware_effects_enabled) == 16 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_lut_interpolation_method) == 17 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_graphics_white_luminance) == 18 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_working_colorspace_id) == 19 * sizeof(void*));

struct Statistics {
  std::uint32_t profiles_created{};
  std::uint32_t profiles_disposed{};
  // Outstanding hand-outs - what the plug-in is holding - and not the number of
  // records behind them. With a real `COR_ACE_Profile*` several hand-outs share
  // one record (issue #1300), so counting records would under-report exactly
  // what this number is for.
  std::size_t profiles_live{};
  std::uint32_t invalid_operations{};
  std::uint32_t xform_calls{};
};

extern AegpItemViewToken g_aegp_item_view;
extern AegpColorSettingsSuite6 g_color_settings_suite6;

inline constexpr std::size_t kMaxColorProfiles = 32;
inline constexpr std::array<std::uint8_t, 16> kWorkingLinearSrgbGuid = {
    0x7a, 0x5c, 0xe3, 0x0c, 0x9d, 0x4d, 0x4f, 0x9a,
    0x8b, 0x1f, 0x6c, 0x2d, 0x4e, 0x8a, 0x11, 0x02};

const void* suite();
const std::vector<std::uint8_t>& color_settings_builtin_srgb_icc();
const std::vector<std::uint8_t>& color_settings_builtin_linear_icc();
bool color_settings_validate_icc(const void*, int32_t);
void color_settings_write_be32(std::vector<std::uint8_t>&, std::size_t, std::uint32_t);
bool color_settings_profiles_balanced();
Statistics color_settings_statistics();
void reset_working_space_to_srgb();

}  // namespace aexcompat::color_settings
