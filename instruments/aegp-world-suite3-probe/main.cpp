#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <type_traits>

namespace {
constexpr A_Err kBadParameter = 516;

struct FixtureWorld {
  AEGP_WorldType type{};
  A_long width{};
  A_long height{};
  A_u_long row_bytes{};
  void* pixels{};
};

FixtureWorld* unwrap(AEGP_WorldH world) {
  return reinterpret_cast<FixtureWorld*>(world);
}

A_Err get_type(AEGP_WorldH world, AEGP_WorldType* type) {
  if (!world || !type) return kBadParameter;
  *type = unwrap(world)->type;
  return A_Err_NONE;
}

A_Err get_size(AEGP_WorldH world, A_long* width, A_long* height) {
  if (!world || !width || !height) return kBadParameter;
  *width = unwrap(world)->width;
  *height = unwrap(world)->height;
  return A_Err_NONE;
}

A_Err get_row_bytes(AEGP_WorldH world, A_u_long* row_bytes) {
  if (!world || !row_bytes) return kBadParameter;
  *row_bytes = unwrap(world)->row_bytes;
  return A_Err_NONE;
}

template <AEGP_WorldType Expected, typename Pixel>
A_Err get_base(AEGP_WorldH world, Pixel** pixels) {
  if (!world || !pixels || unwrap(world)->type != Expected) return kBadParameter;
  *pixels = static_cast<Pixel*>(unwrap(world)->pixels);
  return A_Err_NONE;
}

A_Err get_base8(AEGP_WorldH world, PF_Pixel8** pixels) {
  return get_base<AEGP_WorldType_8>(world, pixels);
}
A_Err get_base16(AEGP_WorldH world, PF_Pixel16** pixels) {
  return get_base<AEGP_WorldType_16>(world, pixels);
}
A_Err get_base32(AEGP_WorldH world, PF_PixelFloat** pixels) {
  return get_base<AEGP_WorldType_32>(world, pixels);
}

A_Err fill_effect_world(AEGP_WorldH world, PF_EffectWorld* output) {
  if (!world || !output) return kBadParameter;
  const auto* source = unwrap(world);
  *output = {};
  output->width = source->width;
  output->height = source->height;
  output->rowbytes = static_cast<A_long>(source->row_bytes);
  output->data = static_cast<PF_PixelPtr>(source->pixels);
  output->world_flags = source->type == AEGP_WorldType_8 ? 0 : PF_WorldFlag_DEEP;
  return A_Err_NONE;
}

std::uint64_t hash_bytes(const void* data, std::size_t size) {
  auto hash = UINT64_C(14695981039346656037);
  const auto* bytes = static_cast<const std::uint8_t*>(data);
  for (std::size_t index = 0; index < size; ++index) {
    hash ^= bytes[index];
    hash *= UINT64_C(1099511628211);
  }
  return hash;
}

struct Observation {
  AEGP_WorldType type{};
  A_long width{};
  A_long height{};
  A_u_long row_bytes{};
  bool typed_address_matches{};
  bool wrong_typed_addresses_rejected{};
  bool projection_matches{};
  std::uint64_t pixel_hash{};
};

Observation observe(const AEGP_WorldSuite3& suite, FixtureWorld& fixture) {
  const auto handle = reinterpret_cast<AEGP_WorldH>(&fixture);
  Observation result{};
  PF_EffectWorld projected{};
  if (suite.AEGP_GetType(handle, &result.type) ||
      suite.AEGP_GetSize(handle, &result.width, &result.height) ||
      suite.AEGP_GetRowBytes(handle, &result.row_bytes) ||
      suite.AEGP_FillOutPFEffectWorld(handle, &projected)) {
    return result;
  }

  PF_Pixel8* pixels8 = nullptr;
  PF_Pixel16* pixels16 = nullptr;
  PF_PixelFloat* pixels32 = nullptr;
  const A_Err err8 = suite.AEGP_GetBaseAddr8(handle, &pixels8);
  const A_Err err16 = suite.AEGP_GetBaseAddr16(handle, &pixels16);
  const A_Err err32 = suite.AEGP_GetBaseAddr32(handle, &pixels32);
  const void* selected = fixture.type == AEGP_WorldType_8
                             ? static_cast<void*>(pixels8)
                             : fixture.type == AEGP_WorldType_16
                                   ? static_cast<void*>(pixels16)
                                   : static_cast<void*>(pixels32);
  result.typed_address_matches = selected == fixture.pixels;
  result.wrong_typed_addresses_rejected =
      (fixture.type == AEGP_WorldType_8 ? err8 == 0 : err8 == kBadParameter) &&
      (fixture.type == AEGP_WorldType_16 ? err16 == 0 : err16 == kBadParameter) &&
      (fixture.type == AEGP_WorldType_32 ? err32 == 0 : err32 == kBadParameter);
  result.projection_matches =
      projected.width == fixture.width && projected.height == fixture.height &&
      projected.rowbytes == static_cast<A_long>(fixture.row_bytes) &&
      projected.data == fixture.pixels &&
      ((fixture.type == AEGP_WorldType_8) ==
       ((projected.world_flags & PF_WorldFlag_DEEP) == 0));
  result.pixel_hash = hash_bytes(fixture.pixels,
                                 fixture.row_bytes * fixture.height);
  return result;
}
}  // namespace

int main() {
  static_assert(sizeof(AEGP_WorldSuite3) == 13 * sizeof(void*));
  static_assert(offsetof(AEGP_WorldSuite3, AEGP_GetType) == 2 * sizeof(void*));
  static_assert(offsetof(AEGP_WorldSuite3, AEGP_FillOutPFEffectWorld) ==
                8 * sizeof(void*));
  static_assert(std::is_same_v<decltype(AEGP_WorldSuite3::AEGP_GetBaseAddr32),
                               A_Err (*)(AEGP_WorldH, PF_PixelFloat**)>);

  std::array<PF_Pixel8, 6> pixels8{{{255, 11, 22, 33}, {200, 44, 55, 66}}};
  std::array<PF_Pixel16, 6> pixels16{{{32768, 101, 202, 303},
                                      {30000, 404, 505, 606}}};
  std::array<PF_PixelFloat, 6> pixels32{{{1.0F, -0.25F, 0.5F, 1.25F},
                                         {0.75F, 0.125F, 0.25F, 0.375F}}};
  FixtureWorld world8{AEGP_WorldType_8, 2, 3, 2 * sizeof(PF_Pixel8),
                      pixels8.data()};
  FixtureWorld world16{AEGP_WorldType_16, 2, 3, 2 * sizeof(PF_Pixel16),
                       pixels16.data()};
  FixtureWorld world32{AEGP_WorldType_32, 2, 3, 2 * sizeof(PF_PixelFloat),
                       pixels32.data()};

  AEGP_WorldSuite3 suite{};
  suite.AEGP_GetType = get_type;
  suite.AEGP_GetSize = get_size;
  suite.AEGP_GetRowBytes = get_row_bytes;
  suite.AEGP_GetBaseAddr8 = get_base8;
  suite.AEGP_GetBaseAddr16 = get_base16;
  suite.AEGP_GetBaseAddr32 = get_base32;
  suite.AEGP_FillOutPFEffectWorld = fill_effect_world;

  const std::array<Observation, 3> observations{
      observe(suite, world8), observe(suite, world16), observe(suite, world32)};
  bool passed = true;
  for (const auto& value : observations) {
    passed = passed && value.width == 2 && value.height == 3 &&
             value.typed_address_matches && value.wrong_typed_addresses_rejected &&
             value.projection_matches && value.pixel_hash != 0;
  }

  std::cout << "{\n  \"schema_version\": 1,\n"
               "  \"source_kind\": \"native_sdk_abi_fixture\",\n"
               "  \"suite\": \"AEGP_WorldSuite3\",\n"
               "  \"slots_exercised\": [2,3,4,5,6,7,8],\n"
               "  \"observations\": [\n";
  for (std::size_t index = 0; index < observations.size(); ++index) {
    const auto& value = observations[index];
    std::cout << "    {\"type\":" << value.type << ",\"width\":" << value.width
              << ",\"height\":" << value.height << ",\"row_bytes\":"
              << value.row_bytes << ",\"typed_address_matches\":"
              << (value.typed_address_matches ? "true" : "false")
              << ",\"wrong_typed_addresses_rejected\":"
              << (value.wrong_typed_addresses_rejected ? "true" : "false")
              << ",\"projection_matches\":"
              << (value.projection_matches ? "true" : "false")
              << ",\"pixel_hash\":" << value.pixel_hash << "}"
              << (index + 1 == observations.size() ? "\n" : ",\n");
  }
  std::cout << "  ],\n  \"passed\": " << (passed ? "true" : "false") << "\n}\n";
  return passed ? 0 : 1;
}
