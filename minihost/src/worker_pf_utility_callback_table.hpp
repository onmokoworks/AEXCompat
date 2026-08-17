#pragma once

#include "generated/aex_abi_contract.hpp"

#include <array>
#include <cstddef>

namespace aexcompat::pf_utility_callbacks {

namespace contract = aexcompat::abi::x86_64_windows;

struct Sources {
  void* begin_sampling{};
  void* subpixel_sample{};
  void* area_sample{};
  void* end_sampling{};
  void* composite_rect{};
  void* blend{};
  void* convolve{};
  void* copy{};
  void* fill{};
  void* premultiply{};
  void* premultiply_color{};
  void* subpixel_sample16{};
  void* area_sample16{};
  void* fill16{};
  void* premultiply_color16{};
  void* iterate16{};
  void* iterate{};
  void* iterate_origin{};
  void* new_world{};
  void* dispose_world{};
  void* transfer_rect{};
  void* transform_world{};
  void* get_callback_addr{};
  void* ansi_ceil{};
  void* ansi_cos{};
  void* ansi_fabs{};
  void* ansi_hypot{};
  void* ansi_pow{};
  void* ansi_sin{};
  void* ansi_sqrt{};
  void* ansi_sprintf{};
  void* ansi_strcpy{};
  void* ansi_asin{};
  void* ansi_acos{};
  void* ansi_atan{};
  void* ansi_atan2{};
  void* ansi_exp{};
  void* ansi_floor{};
  void* ansi_fmod{};
  void* ansi_log{};
  void* ansi_log10{};
  void* ansi_tan{};
  void* get_platform_data{};
  void* get_pixel_data8{};
  void* get_pixel_data16{};
  void* host_new_handle{};
  void* host_lock_handle{};
  void* host_unlock_handle{};
  void* host_dispose_handle{};
  void* host_get_handle_size{};
  void* iterate_origin_non_clip_src{};
  void* iterate_generic{};
  void* host_resize_handle{};
  void* app{};
};

struct Binding {
  std::size_t offset;
  void* Sources::*source;
};

inline constexpr std::array<Binding, 54> BINDINGS{{
    {contract::UTILS_BEGIN_SAMPLING_OFFSET, &Sources::begin_sampling},
    {contract::UTILS_SUBPIXEL_SAMPLE_OFFSET, &Sources::subpixel_sample},
    {contract::UTILS_AREA_SAMPLE_OFFSET, &Sources::area_sample},
    {contract::UTILS_END_SAMPLING_OFFSET, &Sources::end_sampling},
    {contract::UTILS_COMPOSITE_RECT_OFFSET, &Sources::composite_rect},
    {contract::UTILS_BLEND_OFFSET, &Sources::blend},
    {contract::UTILS_CONVOLVE_OFFSET, &Sources::convolve},
    {contract::UTILS_COPY_OFFSET, &Sources::copy},
    {contract::UTILS_FILL_OFFSET, &Sources::fill},
    {contract::UTILS_PREMULTIPLY_OFFSET, &Sources::premultiply},
    {contract::UTILS_PREMULTIPLY_COLOR_OFFSET, &Sources::premultiply_color},
    {contract::UTILS_SUBPIXEL_SAMPLE16_OFFSET, &Sources::subpixel_sample16},
    {contract::UTILS_AREA_SAMPLE16_OFFSET, &Sources::area_sample16},
    {contract::UTILS_FILL16_OFFSET, &Sources::fill16},
    {contract::UTILS_PREMULTIPLY_COLOR16_OFFSET, &Sources::premultiply_color16},
    {contract::UTILS_ITERATE16_OFFSET, &Sources::iterate16},
    {contract::UTILS_ITERATE_OFFSET, &Sources::iterate},
    {contract::UTILS_ITERATE_ORIGIN_OFFSET, &Sources::iterate_origin},
    {contract::UTILS_NEW_WORLD_OFFSET, &Sources::new_world},
    {contract::UTILS_DISPOSE_WORLD_OFFSET, &Sources::dispose_world},
    {contract::UTILS_TRANSFER_RECT_OFFSET, &Sources::transfer_rect},
    {contract::UTILS_TRANSFORM_WORLD_OFFSET, &Sources::transform_world},
    {contract::UTILS_GET_CALLBACK_ADDR_OFFSET, &Sources::get_callback_addr},
    {contract::UTILS_ANSI_CEIL_OFFSET, &Sources::ansi_ceil},
    {contract::UTILS_ANSI_COS_OFFSET, &Sources::ansi_cos},
    {contract::UTILS_ANSI_FABS_OFFSET, &Sources::ansi_fabs},
    {contract::UTILS_ANSI_HYPOT_OFFSET, &Sources::ansi_hypot},
    {contract::UTILS_ANSI_POW_OFFSET, &Sources::ansi_pow},
    {contract::UTILS_ANSI_SIN_OFFSET, &Sources::ansi_sin},
    {contract::UTILS_ANSI_SQRT_OFFSET, &Sources::ansi_sqrt},
    {contract::UTILS_ANSI_SPRINTF_OFFSET, &Sources::ansi_sprintf},
    {contract::UTILS_ANSI_STRCPY_OFFSET, &Sources::ansi_strcpy},
    {contract::UTILS_ANSI_ASIN_OFFSET, &Sources::ansi_asin},
    {contract::UTILS_ANSI_ACOS_OFFSET, &Sources::ansi_acos},
    {contract::UTILS_ANSI_ATAN_OFFSET, &Sources::ansi_atan},
    {contract::UTILS_ANSI_ATAN2_OFFSET, &Sources::ansi_atan2},
    {contract::UTILS_ANSI_EXP_OFFSET, &Sources::ansi_exp},
    {contract::UTILS_ANSI_FLOOR_OFFSET, &Sources::ansi_floor},
    {contract::UTILS_ANSI_FMOD_OFFSET, &Sources::ansi_fmod},
    {contract::UTILS_ANSI_LOG_OFFSET, &Sources::ansi_log},
    {contract::UTILS_ANSI_LOG10_OFFSET, &Sources::ansi_log10},
    {contract::UTILS_ANSI_TAN_OFFSET, &Sources::ansi_tan},
    {contract::UTILS_GET_PLATFORM_DATA_OFFSET, &Sources::get_platform_data},
    {contract::UTILS_GET_PIXEL_DATA8_OFFSET, &Sources::get_pixel_data8},
    {contract::UTILS_GET_PIXEL_DATA16_OFFSET, &Sources::get_pixel_data16},
    {contract::UTILS_HOST_NEW_HANDLE_OFFSET, &Sources::host_new_handle},
    {contract::UTILS_HOST_LOCK_HANDLE_OFFSET, &Sources::host_lock_handle},
    {contract::UTILS_HOST_UNLOCK_HANDLE_OFFSET, &Sources::host_unlock_handle},
    {contract::UTILS_HOST_DISPOSE_HANDLE_OFFSET, &Sources::host_dispose_handle},
    {contract::UTILS_HOST_GET_HANDLE_SIZE_OFFSET, &Sources::host_get_handle_size},
    {contract::UTILS_ITERATE_ORIGIN_NON_CLIP_SRC_OFFSET,
     &Sources::iterate_origin_non_clip_src},
    {contract::UTILS_ITERATE_GENERIC_OFFSET, &Sources::iterate_generic},
    {contract::UTILS_HOST_RESIZE_HANDLE_OFFSET, &Sources::host_resize_handle},
    {contract::UTILS_APP_OFFSET, &Sources::app},
}};

constexpr std::size_t index_of(std::size_t offset) noexcept {
  for (std::size_t index = 0; index < contract::UTILITY_CALLBACK_OFFSETS.size(); ++index)
    if (contract::UTILITY_CALLBACK_OFFSETS[index] == offset) return index;
  return contract::UTILITY_CALLBACK_OFFSETS.size();
}

constexpr bool bindings_cover_contract_once() noexcept {
  for (std::size_t index = 0; index < BINDINGS.size(); ++index) {
    if (index_of(BINDINGS[index].offset) == contract::UTILITY_CALLBACK_OFFSETS.size())
      return false;
    for (std::size_t other = index + 1; other < BINDINGS.size(); ++other)
      if (BINDINGS[index].offset == BINDINGS[other].offset) return false;
  }
  return true;
}

inline std::array<void*, contract::UTILITY_CALLBACK_OFFSETS.size()> build(
    const Sources& sources) noexcept {
  std::array<void*, contract::UTILITY_CALLBACK_OFFSETS.size()> callbacks{};
  for (const auto& binding : BINDINGS)
    callbacks[index_of(binding.offset)] = sources.*(binding.source);
  return callbacks;
}

// A source this table names but nobody assigns installs a null pointer, which
// `bindings_cover_contract_once` below cannot see. That half of the invariant
// belongs to `effect_bootstrap::unwired_installed_offsets`, which reads the
// installed bytes rather than this array, and is asked of the shipping wiring
// by the worker's `--self-test-utility-callback-table` route.
static_assert(BINDINGS.size() == contract::UTILITY_CALLBACK_OFFSETS.size());
static_assert(bindings_cover_contract_once(),
              "every generated utility callback offset must have exactly one named source");

}  // namespace aexcompat::pf_utility_callbacks
