#pragma once

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace aexcompat::pf_ae_channel {

struct HostHooks {
  void* (*effect_ref)(){};
  std::size_t (*parameter_count)(){};
  bool (*parameter_is_layer)(std::size_t zero_based_index){};
  bool (*sha256_file)(const std::filesystem::path&, std::string&){};
};

void configure_host_hooks(HostHooks hooks);

int32_t __cdecl get_layer_channel_count(void*, int32_t, int32_t*);
int32_t __cdecl get_layer_channel_indexed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl get_layer_channel_typed(void*, int32_t, int32_t, uint8_t*, void*, void*);
int32_t __cdecl checkout_layer_channel(void*, void*, int32_t, int32_t, uint32_t, int32_t, void*);
int32_t __cdecl checkin_layer_channel(void*, void*, void*);

struct PfAeChannelSuite1 {
  decltype(&get_layer_channel_count) count;
  decltype(&get_layer_channel_indexed) indexed;
  decltype(&get_layer_channel_typed) typed;
  decltype(&checkout_layer_channel) checkout;
  decltype(&checkin_layer_channel) checkin;
};

static_assert(sizeof(PfAeChannelSuite1) == 5 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, count) == 0 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, indexed) == 1 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, typed) == 2 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, checkout) == 3 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, checkin) == 4 * sizeof(void*));

extern PfAeChannelSuite1 g_channel_suite1;

struct TransportStatistics {
  int32_t row_bytes{};
  int32_t origin_x{};
  int32_t origin_y{};
  int32_t duration{};
};

bool load_external_aux_manifest(const std::filesystem::path& path);
bool parse_alpha_coverage_params(const wchar_t* text);
bool validate_external_aux_parameters();
void activate_external_aux();
void deactivate_external_aux();
void reclaim_layer_channels();
void publish_alpha_coverage_provider(const std::vector<unsigned char>& argb,
                                     int32_t width, int32_t height,
                                     int32_t pixel_bytes, int32_t time,
                                     uint32_t time_scale);
void clear_native_aux_provider();

bool verify_pf_ae_channel_suite();
bool verify_pf_ae_channel_transport(const std::filesystem::path& manifest);
bool verify_pf_ae_channel_native_provider();

TransportStatistics transport_statistics();
uint32_t channel_count_queries();

}  // namespace aexcompat::pf_ae_channel
