#include "worker_pf_ae_channel_runtime.hpp"

#include "strict_json.hpp"
#include "worker_handle_runtime.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <set>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>

namespace aexcompat::pf_ae_channel {

using aexcompat::strict_json::JsonValue;
using aexcompat::strict_json::StrictJsonParser;
using aexcompat::strict_json::json_exact_keys;
using aexcompat::strict_json::json_i32;
using aexcompat::strict_json::json_member;
using aexcompat::strict_json::json_string;
using aexcompat::strict_json::json_u64;
using namespace aexcompat::worker_runtime::handles;

constexpr int32_t kPfInvalidIndex = 513;
constexpr int32_t kPfUnrecognizedParamType = 514;
constexpr int32_t kPfBadCallbackParam = 516;

HostHooks g_host_hooks{};
std::atomic<uint32_t> g_channel_count_queries{0};
PfAeChannelSuite1 g_channel_suite1{&get_layer_channel_count, &get_layer_channel_indexed,
    &get_layer_channel_typed, &checkout_layer_channel, &checkin_layer_channel};

void configure_host_hooks(HostHooks hooks) { g_host_hooks = hooks; }
void* host_effect_ref() { return g_host_hooks.effect_ref ? g_host_hooks.effect_ref() : nullptr; }
std::size_t parameter_count() { return g_host_hooks.parameter_count ? g_host_hooks.parameter_count() : 0; }
bool parameter_is_layer(std::size_t index) { return g_host_hooks.parameter_is_layer && g_host_hooks.parameter_is_layer(index); }
bool sha256_file(const std::filesystem::path& path, std::string& hash) { return g_host_hooks.sha256_file && g_host_hooks.sha256_file(path, hash); }
constexpr std::size_t kMaxAuxChannels = 16, kMaxAuxChannelsPerParam = 8;
constexpr std::size_t kMaxAuxSamplesPerChannel = 64, kMaxAuxSamples = 256;
constexpr uint64_t kMaxAuxSampleBytes = 256ULL * 1024 * 1024;
constexpr uint64_t kMaxAuxTotalBytes = 512ULL * 1024 * 1024;

struct ExternalAuxSample {
  int32_t time{};
  uint32_t time_scale{};
  std::string sampling;
  std::shared_ptr<const std::vector<float>> values;
};
struct ExternalAuxChannel {
  int32_t param_index{}, channel_type{}, dimension{}, width{}, height{};
  int32_t native_data_type{0x464c5434};
  int32_t signed_row_bytes{};
  int32_t origin_x{}, origin_y{};
  int32_t downsample_x_num{1}, downsample_x_den{1};
  int32_t downsample_y_num{1}, downsample_y_den{1};
  std::string coordinate_space{"source_pixel"};
  std::string units{"unitless"};
  std::string name;
  std::vector<ExternalAuxSample> samples;
  uint64_t identity{};
};
struct ExternalAuxSet { std::vector<ExternalAuxChannel> channels; };
ExternalAuxSet g_external_aux_set;
thread_local ExternalAuxSet g_native_aux_set;
// Parsed once before native dispatch, then immutable for every MFR render thread.
std::vector<int32_t> g_alpha_as_coverage_params;
bool g_external_aux_loaded = false;
thread_local bool g_external_aux_active = false;

bool load_aux_manifest(const std::filesystem::path &manifest_path,
                       ExternalAuxSet &result) {
  std::error_code ec;
  const auto absolute = std::filesystem::absolute(manifest_path, ec);
  const auto canonical = std::filesystem::canonical(manifest_path, ec);
  if (ec || !manifest_path.is_absolute() ||
      absolute.lexically_normal() != canonical)
    return false;
  const auto manifest_size = std::filesystem::file_size(canonical, ec);
  if (ec || manifest_size == 0 || manifest_size > 1024 * 1024)
    return false;
  std::ifstream input(canonical, std::ios::binary);
  if (!input)
    return false;
  std::string text((std::istreambuf_iterator<char>(input)), {});
  JsonValue root;
  if (input.bad() || !StrictJsonParser(std::move(text)).parse(root) ||
      !std::holds_alternative<JsonValue::Object>(root.value))
    return false;
  const auto &object = std::get<JsonValue::Object>(root.value);
  std::string schema, nonce;
  if (!json_exact_keys(object, {"schema", "nonce", "channels"}) ||
      !json_string(object, "schema", schema) || schema != "aux-manifest-v1" ||
      !json_string(object, "nonce", nonce) || nonce.empty() ||
      !std::all_of(nonce.begin(), nonce.end(),
                   [](unsigned char c) { return std::isdigit(c) != 0; }))
    return false;
  const auto *channels_value = json_member(object, "channels");
  if (!channels_value ||
      !std::holds_alternative<JsonValue::Array>(channels_value->value))
    return false;
  const auto &channels = std::get<JsonValue::Array>(channels_value->value);
  if (channels.empty() || channels.size() > kMaxAuxChannels)
    return false;
  ExternalAuxSet parsed;
  std::map<int32_t, std::size_t> per_param;
  std::set<std::pair<int32_t, int32_t>> keys;
  std::set<std::filesystem::path> paths;
  std::size_t total_samples = 0;
  uint64_t total_bytes = 0, identity = 1;
  for (const auto &cv : channels) {
    if (!std::holds_alternative<JsonValue::Object>(cv.value))
      return false;
    const auto &co = std::get<JsonValue::Object>(cv.value);
    const bool rich_descriptor = json_exact_keys(co, {"param_index", "type", "name", "data_type",
        "dimension", "width", "height", "row_bytes", "origin_x", "origin_y",
        "downsample_x_num", "downsample_x_den", "downsample_y_num", "downsample_y_den",
        "coordinate_space", "units", "samples"});
    if (!rich_descriptor && !json_exact_keys(co, {"param_index", "type", "name", "data_type",
                                                   "dimension", "width", "height", "samples"}))
      return false;
    ExternalAuxChannel channel;
    int32_t param{}, dim{}, width{}, height{};
    std::string type;
    if (!json_i32(co, "param_index", param) || param < 0 ||
        !json_i32(co, "type", channel.channel_type) ||
        !json_string(co, "name", channel.name) || channel.name.size() > 63 ||
        channel.name.find('\0') != std::string::npos ||
        !json_string(co, "data_type", type) || type != "f32le" ||
        !json_i32(co, "dimension", dim) || dim < 1 || dim > 4 ||
        !json_i32(co, "width", width) || width < 1 || width > 4096 ||
        !json_i32(co, "height", height) || height < 1 || height > 4096)
      return false;
    channel.param_index = param;
    channel.dimension = dim;
    channel.width = width;
    channel.height = height;
    channel.signed_row_bytes = width * dim * 4;
    if (rich_descriptor) {
      if (!json_i32(co,"row_bytes",channel.signed_row_bytes) ||
          !json_i32(co,"origin_x",channel.origin_x) || !json_i32(co,"origin_y",channel.origin_y) ||
          !json_i32(co,"downsample_x_num",channel.downsample_x_num) ||
          !json_i32(co,"downsample_x_den",channel.downsample_x_den) ||
          !json_i32(co,"downsample_y_num",channel.downsample_y_num) ||
          !json_i32(co,"downsample_y_den",channel.downsample_y_den) ||
          !json_string(co,"coordinate_space",channel.coordinate_space) ||
          !json_string(co,"units",channel.units) || channel.signed_row_bytes == INT32_MIN ||
          std::abs(channel.signed_row_bytes) < width * dim * 4 ||
          channel.downsample_x_num <= 0 || channel.downsample_x_den <= 0 ||
          channel.downsample_y_num <= 0 || channel.downsample_y_den <= 0 ||
          channel.coordinate_space.empty() || channel.coordinate_space.size() > 64 ||
          channel.units.empty() || channel.units.size() > 64) return false;
    }
    channel.identity = identity++;
    if (++per_param[param] > kMaxAuxChannelsPerParam ||
        !keys.emplace(param, channel.channel_type).second)
      return false;
    const uint64_t bytes = static_cast<uint64_t>(std::abs(channel.signed_row_bytes)) * height;
    if (bytes > kMaxAuxSampleBytes)
      return false;
    const auto* sv=json_member(co,"samples"); if(!sv||!std::holds_alternative<JsonValue::Array>(sv->value))return false; const auto& samples=std::get<JsonValue::Array>(sv->value); if(samples.empty()||samples.size()>kMaxAuxSamplesPerChannel)return false; std::set<std::pair<int32_t,uint32_t>> times;
    for(const auto& sample_value:samples){ if(++total_samples>kMaxAuxSamples||total_bytes>kMaxAuxTotalBytes-bytes)return false; total_bytes+=bytes; if(!std::holds_alternative<JsonValue::Object>(sample_value.value))return false; const auto& so=std::get<JsonValue::Object>(sample_value.value); if(!json_exact_keys(so,{"time","time_scale","path","sampling","interpretation","expected_byte_length","sha256"}))return false;
      ExternalAuxSample sample; int32_t scale{}; uint64_t declared_size{}; std::string path_text,sampling,interpretation,declared_hash;
      if(!json_i32(so,"time",sample.time)||!json_i32(so,"time_scale",scale)||scale<=0||!times.emplace(sample.time,static_cast<uint32_t>(scale)).second||!json_string(so,"path",path_text)||!json_string(so,"sampling",sampling)||(sampling!="exact"&&sampling!="hold")||!json_string(so,"interpretation",interpretation)||(interpretation!="depth"&&interpretation!="normals"&&interpretation!="motion_vectors"&&interpretation!="generic")||!json_u64(so,"expected_byte_length",declared_size)||declared_size!=bytes||!json_string(so,"sha256",declared_hash)||declared_hash.size()!=64||!std::all_of(declared_hash.begin(),declared_hash.end(),[](unsigned char c){return std::isxdigit(c)!=0;}))return false;
      if ((interpretation=="depth" && dim!=1) || (interpretation=="normals" && dim!=3) ||
          (interpretation=="motion_vectors" && dim!=2) ||
          (channel.channel_type==0x44505448 && (interpretation!="depth" || dim!=1))) return false;
      sample.sampling = sampling;
      sample.time_scale=static_cast<uint32_t>(scale); const std::filesystem::path path=std::filesystem::u8path(path_text); const auto abs=std::filesystem::absolute(path,ec); const auto canon=std::filesystem::canonical(path,ec); if(ec||!path.is_absolute()||abs.lexically_normal()!=canon||canon.parent_path()!=canonical.parent_path()||!paths.insert(canon).second||std::filesystem::file_size(canon,ec)!=bytes)return false; std::string actual_hash; if(ec||!sha256_file(canon,actual_hash)||_stricmp(actual_hash.c_str(),declared_hash.c_str())!=0)return false;
      std::ifstream raw(canon,std::ios::binary); std::vector<std::byte> raw_bytes(static_cast<std::size_t>(bytes)); if(!raw.read(reinterpret_cast<char*>(raw_bytes.data()),static_cast<std::streamsize>(bytes))||raw.peek()!=EOF)return false; auto values=std::make_shared<std::vector<float>>(static_cast<std::size_t>(width)*height*dim); const std::size_t stride=static_cast<std::size_t>(std::abs(channel.signed_row_bytes)), packed=static_cast<std::size_t>(width)*dim*sizeof(float); for(int32_t y=0;y<height;++y){const int32_t physical=channel.signed_row_bytes<0?height-1-y:y;std::memcpy(values->data()+static_cast<std::size_t>(y)*width*dim,raw_bytes.data()+static_cast<std::size_t>(physical)*stride,packed);} if(std::any_of(values->begin(),values->end(),[](float v){return !std::isfinite(v);}))return false; sample.values=std::move(values); channel.samples.push_back(std::move(sample)); }
    parsed.channels.push_back(std::move(channel));
  }
  result=std::move(parsed); return true;
}
constexpr uint64_t kChannelRefMagic = 0x504641454348414eULL;  // "PFAECHAN"
constexpr int32_t kChannelDepth = 0x44505448;                // 'DPTH'
constexpr int32_t kDataFloat = 0x464c5434;
constexpr int32_t kDataDouble = 0x44424c38;
constexpr int32_t kDataLong = 0x4c4f4e34;
constexpr int32_t kDataShort = 0x53485432;
constexpr int32_t kDataFixed = 0x46495834;
constexpr int32_t kDataChar = 0x43485231;
constexpr int32_t kDataUByte = 0x55425431;
constexpr int32_t kDataUShort = 0x55535432;
constexpr int32_t kDataUFixed = 0x55465834;

struct PfChannelRef {
  uint64_t magic;
  uint64_t generation;
  int32_t param_index;
  int32_t channel_index;
  int32_t channel_type;
  int32_t reserved;
  std::array<uint64_t, 4> seal;
};
struct PfChannelDesc {
  int32_t channel_type;
  char name[64];
  int32_t data_type;
  int32_t dimension;
};
struct PfChannelChunk {
  PfChannelRef channel_ref;
  int32_t width;
  int32_t height;
  int32_t dimension;
  int32_t row_bytes;
  int32_t data_type;
  void* data_handle;
  void* data;
};
static_assert(sizeof(PfChannelRef) == 64);
static_assert(sizeof(PfChannelDesc) == 76);
static_assert(offsetof(PfChannelDesc, data_type) == 68);
static_assert(sizeof(PfChannelChunk) == 104);
static_assert(offsetof(PfChannelChunk, width) == 64);
static_assert(offsetof(PfChannelChunk, data_handle) == 88);
static_assert(offsetof(PfChannelChunk, data) == 96);

struct LiveChannelChunk {
  PfChannelRef ref{};
  int32_t data_type{};
  int32_t row_bytes{};
  int32_t width{};
  int32_t height{};
  int32_t dimension{};
  std::size_t bytes{};
  void** handle{};
  void* locked_data{};
  std::shared_ptr<const std::vector<float>> sample_receipt;
  int32_t sample_time{};
  uint32_t sample_time_scale{};
  int32_t duration{};
  std::thread::id owner_thread{};
};
std::mutex g_channel_mutex;
std::unordered_map<PfChannelChunk*, LiveChannelChunk> g_live_channel_chunks;
thread_local uint64_t g_channel_generation = 1;
thread_local bool g_channel_fixture_enabled = false;
constexpr int32_t kChannelFixtureWidth = 3;
constexpr int32_t kChannelFixtureHeight = 2;
constexpr std::array<float, 6> kChannelFixture{0.0f, 0.25f, 1.0f, -1.0f, 2.0f, 0.5f};
int32_t g_channel_transport_row_bytes{}, g_channel_transport_origin_x{},
    g_channel_transport_origin_y{}, g_channel_transport_duration{};

template <typename Callback>
void for_each_active_aux_channel(Callback&& callback) {
  if (g_external_aux_active)
    for (const auto& channel : g_external_aux_set.channels) callback(channel);
  for (const auto& channel : g_native_aux_set.channels) callback(channel);
}

void publish_alpha_coverage_provider(const std::vector<unsigned char>& argb,
                                     int32_t width, int32_t height,
                                     int32_t pixel_bytes, int32_t time,
                                     uint32_t time_scale) {
  g_native_aux_set = {};
  if (g_alpha_as_coverage_params.empty() || width <= 0 || height <= 0 ||
      (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16) ||
      argb.size() != static_cast<std::size_t>(width) * height * pixel_bytes)
    return;
  auto values = std::make_shared<std::vector<float>>(
      static_cast<std::size_t>(width) * height);
  for (std::size_t pixel = 0; pixel < values->size(); ++pixel) {
    const auto* source = argb.data() + pixel * pixel_bytes;
    if (pixel_bytes == 4) {
      (*values)[pixel] = static_cast<float>(source[0]) / 255.0f;
    } else if (pixel_bytes == 8) {
      uint16_t alpha{};
      std::memcpy(&alpha, source, sizeof(alpha));
      (*values)[pixel] = static_cast<float>(alpha) / 32768.0f;
    } else {
      float alpha{};
      std::memcpy(&alpha, source, sizeof(alpha));
      (*values)[pixel] = alpha;
    }
  }
  uint64_t identity = 0x434f565200000000ULL;
  for (const int32_t param : g_alpha_as_coverage_params) {
    if (param < 0 || static_cast<std::size_t>(param) > parameter_count() ||
        (param > 0 && !parameter_is_layer(static_cast<std::size_t>(param - 1))) ||
        std::any_of(g_external_aux_set.channels.begin(), g_external_aux_set.channels.end(),
                    [&](const auto& existing) { return existing.param_index == param &&
                        existing.channel_type == 0x434f5652; }))
      continue;
    ExternalAuxChannel channel;
    channel.param_index = param;
    channel.channel_type = 0x434f5652;  // 'COVR'
    channel.dimension = 1;
    channel.width = width;
    channel.height = height;
    channel.signed_row_bytes = width * 4;
    channel.name = "Coverage";
    channel.coordinate_space = "pre_effect_source_pixel";
    channel.units = "normalized_coverage";
    channel.identity = ++identity;
    channel.samples.push_back(ExternalAuxSample{
        time, time_scale, "exact", values});
    g_native_aux_set.channels.push_back(std::move(channel));
  }
}

void clear_native_aux_provider() { g_native_aux_set = {}; }

bool parse_alpha_coverage_params(const wchar_t* text) {
  if (!text || !*text) return false;
  std::vector<int32_t> parsed;
  const std::wstring value(text);
  std::size_t begin = 0;
  while (begin < value.size()) {
    const std::size_t end = value.find(L',', begin);
    const std::wstring token = value.substr(begin, end - begin);
    try {
      std::size_t used = 0;
      const long slot = std::stol(token, &used);
      if (used != token.size() || slot < 0 || slot > 1024 ||
          std::find(parsed.begin(), parsed.end(), static_cast<int32_t>(slot)) != parsed.end())
        return false;
      parsed.push_back(static_cast<int32_t>(slot));
    } catch (...) { return false; }
    if (end == std::wstring::npos) break;
    begin = end + 1;
  }
  g_alpha_as_coverage_params = std::move(parsed);
  return !g_alpha_as_coverage_params.empty();
}

PfChannelRef make_channel_ref(int32_t param_index, int32_t channel_index = 0,
                              int32_t channel_type = kChannelDepth, uint64_t identity = 1) {
  PfChannelRef ref{kChannelRefMagic, g_channel_generation, param_index, channel_index, channel_type, 0};
  ref.seal = {kChannelRefMagic ^ ref.generation,
      (static_cast<uint64_t>(static_cast<uint32_t>(param_index)) << 32) ^ static_cast<uint32_t>(channel_index),
      static_cast<uint64_t>(static_cast<uint32_t>(channel_type)) ^ ref.generation,
      identity ^ ~kChannelRefMagic};
  return ref;
}

bool valid_channel_ref(const PfChannelRef& ref) {
  if (ref.magic != kChannelRefMagic || ref.generation != g_channel_generation) return false;
  if (g_channel_fixture_enabled) { const auto expected=make_channel_ref(0); return std::memcmp(&ref, &expected, sizeof(ref)) == 0; }
  int32_t index = 0;
  bool valid = false;
  for_each_active_aux_channel([&](const auto& channel) { if(channel.param_index == ref.param_index) {
    const PfChannelRef expected = make_channel_ref(ref.param_index, index, channel.channel_type, channel.identity);
    if (std::memcmp(&ref, &expected, sizeof(ref)) == 0) valid = true;
    ++index;
  }});
  return valid;
}

void describe_channel(PfChannelDesc* desc, int32_t type, const std::string& name,
                      int32_t dimension, int32_t native_data_type = kDataFloat) {
  std::memset(desc, 0, sizeof(*desc));
  desc->channel_type = type;
  std::memcpy(desc->name, name.data(), name.size());
  desc->data_type = native_data_type;
  desc->dimension = dimension;
}

const ExternalAuxChannel* channel_from_ref(const PfChannelRef& ref) {
  if (!valid_channel_ref(ref) || g_channel_fixture_enabled) return nullptr;
  int32_t index=0; const ExternalAuxChannel* result=nullptr;
  for_each_active_aux_channel([&](const auto& channel) { if (!result && channel.param_index==ref.param_index && index++==ref.channel_index) result=&channel; });
  return result;
}

int32_t channel_element_size(int32_t type) {
  switch (type) {
    case kDataFloat: case kDataLong: case kDataFixed: case kDataUFixed: return 4;
    case kDataDouble: return 8;
    case kDataShort: case kDataUShort: return 2;
    case kDataChar: case kDataUByte: return 1;
    default: return 0;
  }
}

struct NativeAuxSampleRequest { int32_t time{}, duration{}; uint32_t time_scale{}; };
const ExternalAuxSample* select_native_aux_sample(
    const ExternalAuxChannel& channel, const NativeAuxSampleRequest& request);

void store_channel_value(std::byte* destination, int32_t type, float value) {
  const double clamped_signed = std::max<double>(std::numeric_limits<int32_t>::min(),
      std::min<double>(std::numeric_limits<int32_t>::max(), value));
  switch (type) {
    case kDataFloat: std::memcpy(destination, &value, 4); break;
    case kDataDouble: { const double converted = value; std::memcpy(destination, &converted, 8); break; }
    case kDataLong: { const int32_t converted = static_cast<int32_t>(clamped_signed); std::memcpy(destination, &converted, 4); break; }
    case kDataShort: { const int16_t converted = static_cast<int16_t>(std::max(-32768.0f, std::min(32767.0f, value))); std::memcpy(destination, &converted, 2); break; }
    case kDataFixed: { const double scaled = static_cast<double>(value) * 65536.0; const int32_t converted = static_cast<int32_t>(std::max<double>(INT32_MIN, std::min<double>(INT32_MAX, scaled))); std::memcpy(destination, &converted, 4); break; }
    case kDataChar: { const int8_t converted = static_cast<int8_t>(std::max(-128.0f, std::min(127.0f, value))); std::memcpy(destination, &converted, 1); break; }
    case kDataUByte: { const uint8_t converted = static_cast<uint8_t>(std::max(0.0f, std::min(255.0f, value))); std::memcpy(destination, &converted, 1); break; }
    case kDataUShort: { const uint16_t converted = static_cast<uint16_t>(std::max(0.0f, std::min(65535.0f, value))); std::memcpy(destination, &converted, 2); break; }
    case kDataUFixed: { const double scaled = std::max(0.0, static_cast<double>(value) * 65536.0); const uint32_t converted = static_cast<uint32_t>(std::min<double>(UINT32_MAX, scaled)); std::memcpy(destination, &converted, 4); break; }
  }
}

int32_t __cdecl get_layer_channel_count(void* effect_ref, int32_t param_index,
                                        int32_t* channel_count) {
  if (effect_ref != host_effect_ref() || !channel_count) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > parameter_count())
    return kPfInvalidIndex;
  *channel_count = 0;
  if (g_channel_fixture_enabled && param_index == 0) *channel_count = 1;
  else for_each_active_aux_channel([&](const auto& channel) {
    if (channel.param_index == param_index) ++*channel_count;
  });
  ++g_channel_count_queries;
  return 0;
}

int32_t __cdecl get_layer_channel_indexed(void* effect_ref, int32_t param_index,
    int32_t channel_index, uint8_t* found, void* channel_ref, void* channel_desc) {
  if (found) *found = 0;
  if (effect_ref != host_effect_ref() || !found || !channel_ref || !channel_desc) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > parameter_count())
    return kPfInvalidIndex;
  if (g_channel_fixture_enabled) {
    if (param_index != 0) return 0; if (channel_index < 0 || channel_index >= 1) return kPfInvalidIndex;
    *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(0); describe_channel(static_cast<PfChannelDesc*>(channel_desc),kChannelDepth,"Depth",1);
  } else {
    int32_t index=0; const ExternalAuxChannel* selected=nullptr;
    for_each_active_aux_channel([&](const auto& channel) { if(!selected && channel.param_index==param_index && index++==channel_index) selected=&channel; });
    if(!selected)return kPfInvalidIndex; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(param_index,channel_index,selected->channel_type,selected->identity); describe_channel(static_cast<PfChannelDesc*>(channel_desc),selected->channel_type,selected->name,selected->dimension,selected->native_data_type);
  }
  *found = 1;
  return 0;
}

int32_t __cdecl get_layer_channel_typed(void* effect_ref, int32_t param_index,
    int32_t channel_type, uint8_t* found, void* channel_ref, void* channel_desc) {
  if (found) *found = 0;
  if (effect_ref != host_effect_ref() || !found || !channel_ref || !channel_desc) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > parameter_count())
    return kPfInvalidIndex;
  if (g_channel_fixture_enabled) { if(param_index!=0||channel_type!=kChannelDepth)return 0; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(0); describe_channel(static_cast<PfChannelDesc*>(channel_desc),kChannelDepth,"Depth",1); }
  else { int32_t index=0; const ExternalAuxChannel* selected=nullptr; for_each_active_aux_channel([&](const auto& channel){if(channel.param_index==param_index){if(!selected&&channel.channel_type==channel_type)selected=&channel;else if(!selected)++index;}}); if(!selected)return 0; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(param_index,index,selected->channel_type,selected->identity); describe_channel(static_cast<PfChannelDesc*>(channel_desc),selected->channel_type,selected->name,selected->dimension,selected->native_data_type); }
  *found = 1;
  return 0;
}

int32_t __cdecl checkout_layer_channel(void* effect_ref, void* channel_ref, int32_t time,
    int32_t duration, uint32_t time_scale, int32_t data_type, void* channel_chunk) {
  if (effect_ref != host_effect_ref() || !channel_ref || !channel_chunk || time_scale == 0)
    return kPfBadCallbackParam;
  const auto& ref = *static_cast<const PfChannelRef*>(channel_ref);
  if (!valid_channel_ref(ref)) return kPfBadCallbackParam;
  const int32_t element_size = channel_element_size(data_type);
  if (element_size == 0) return kPfUnrecognizedParamType;
  const ExternalAuxChannel* external=channel_from_ref(ref);
  const int32_t width=external?external->width:kChannelFixtureWidth, height=external?external->height:kChannelFixtureHeight, dimension=external?external->dimension:1;
  const std::size_t pixels = static_cast<std::size_t>(width)*height*dimension;
  if (pixels > std::numeric_limits<std::size_t>::max() / static_cast<std::size_t>(element_size)) return 4;
  const int32_t native_packed_row = width * dimension * 4;
  const int32_t native_stride = external ? external->signed_row_bytes : native_packed_row;
  const int32_t padding = std::abs(native_stride) - native_packed_row;
  const int64_t converted_stride64 = static_cast<int64_t>(width) * dimension * element_size + padding;
  if (converted_stride64 <= 0 || converted_stride64 > INT32_MAX) return 4;
  const int32_t converted_stride = static_cast<int32_t>(converted_stride64);
  const int32_t signed_converted_stride = native_stride < 0 ? -converted_stride : converted_stride;
  const std::size_t bytes = static_cast<std::size_t>(converted_stride) * height;
  const float* source=kChannelFixture.data();
  std::shared_ptr<const std::vector<float>> sample_receipt;
  int32_t selected_time=0; uint32_t selected_scale=1;
  if (external) {
    const ExternalAuxSample* selected = select_native_aux_sample(
        *external, NativeAuxSampleRequest{time, duration, time_scale});
    if (!selected || !selected->values) return kPfInvalidIndex;
    sample_receipt = selected->values;
    source = sample_receipt->data();
    selected_time = selected->time;
    selected_scale = selected->time_scale;
  }
  auto* chunk = static_cast<PfChannelChunk*>(channel_chunk);
  { std::lock_guard<std::mutex> lock(g_channel_mutex); if(g_live_channel_chunks.count(chunk))return kPfBadCallbackParam; }
  void** handle=new_handle(bytes); if(!handle)return 4; void* locked=lock_handle(handle); if(!locked){dispose_handle(handle);return 4;}
  std::memset(locked, 0, bytes);
  auto* exposed = static_cast<std::byte*>(locked) +
      (signed_converted_stride < 0 ? static_cast<std::size_t>(height - 1) * converted_stride : 0);
  for (int32_t y = 0; y < height; ++y)
    for (int32_t x = 0; x < width * dimension; ++x)
      store_channel_value(exposed + static_cast<std::ptrdiff_t>(y) * signed_converted_stride +
                              static_cast<std::size_t>(x) * element_size,
                          data_type, source[static_cast<std::size_t>(y) * width * dimension + x]);
  bool duplicate = false;
  try {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    duplicate = g_live_channel_chunks.find(chunk) != g_live_channel_chunks.end();
    if (!duplicate) {
      *chunk = {ref,width,height,dimension,signed_converted_stride,data_type,handle,exposed};
      LiveChannelChunk live{ref,data_type,chunk->row_bytes,width,height,dimension,bytes,handle,exposed,
                            std::move(sample_receipt),selected_time,selected_scale,duration,
                            std::this_thread::get_id()};
      g_live_channel_chunks.emplace(chunk, std::move(live));
    }
  } catch (const std::bad_alloc&) {
    unlock_handle(handle); dispose_handle(handle); *chunk={}; return 4;
  }
  if (duplicate) { unlock_handle(handle); dispose_handle(handle); return kPfBadCallbackParam; }
  return 0;
}

int32_t __cdecl checkin_layer_channel(void* effect_ref, void* channel_ref, void* channel_chunk) {
  if (effect_ref != host_effect_ref() || !channel_ref || !channel_chunk) return kPfBadCallbackParam;
  auto* chunk = static_cast<PfChannelChunk*>(channel_chunk);
  LiveChannelChunk live;
  {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    const auto found = g_live_channel_chunks.find(chunk);
    if (found == g_live_channel_chunks.end()) return kPfBadCallbackParam;
    live = found->second;
    if (live.owner_thread != std::this_thread::get_id() ||
        std::memcmp(channel_ref, &live.ref, sizeof(live.ref)) != 0 ||
        std::memcmp(&chunk->channel_ref, &live.ref, sizeof(live.ref)) != 0 ||
        chunk->data != live.locked_data || chunk->data_handle != live.handle ||
        chunk->row_bytes != live.row_bytes || chunk->width != live.width ||
        chunk->height != live.height || chunk->dimension != live.dimension ||
        chunk->data_type != live.data_type) return kPfBadCallbackParam;
    g_live_channel_chunks.erase(found);
  }
  unlock_handle(live.handle); dispose_handle(live.handle);
  *chunk = {};
  return 0;
}

void reclaim_layer_channels() {
  std::vector<LiveChannelChunk> live;
  {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    for (auto item = g_live_channel_chunks.begin(); item != g_live_channel_chunks.end();) {
      if (item->second.owner_thread == std::this_thread::get_id()) {
        live.push_back(item->second);
        item = g_live_channel_chunks.erase(item);
      } else {
        ++item;
      }
    }
  }
  for(auto& item:live){unlock_handle(item.handle);dispose_handle(item.handle);}
  ++g_channel_generation;
  if (g_channel_generation == 0) ++g_channel_generation;
}

bool validate_external_aux_parameters() {
  if (!g_external_aux_loaded) return true;
  return std::all_of(g_external_aux_set.channels.begin(), g_external_aux_set.channels.end(),
      [](const auto& channel) {
        return channel.param_index == 0 ||
            (channel.param_index > 0 && static_cast<std::size_t>(channel.param_index) <= parameter_count() &&
             parameter_is_layer(static_cast<std::size_t>(channel.param_index - 1)));
      });
}

bool verify_pf_ae_channel_suite() {
  reclaim_layer_channels();
  g_channel_fixture_enabled = true;
  int32_t count = -1;
  uint8_t found = 7;
  PfChannelRef ref{};
  PfChannelDesc desc{};
  bool passed = get_layer_channel_count(host_effect_ref(), 0, &count) == 0 && count == 1 &&
      get_layer_channel_indexed(host_effect_ref(), 0, 0, &found, &ref, &desc) == 0 && found == 1 &&
      desc.channel_type == kChannelDepth && desc.data_type == kDataFloat && desc.dimension == 1;
  found = 7;
  passed = passed && get_layer_channel_typed(host_effect_ref(), 0, 0x554e4b4e, &found,
      &ref, &desc) == 0 && found == 0;
  passed = passed && get_layer_channel_indexed(host_effect_ref(), 0, 1, &found,
      &ref, &desc) == kPfInvalidIndex && found == 0;
  found = 0;
  passed = passed && get_layer_channel_typed(host_effect_ref(), 0, kChannelDepth, &found,
      &ref, &desc) == 0 && found == 1;
  const std::array<int32_t, 9> types{kDataFloat, kDataDouble, kDataLong, kDataShort,
      kDataFixed, kDataChar, kDataUByte, kDataUShort, kDataUFixed};
  for (const int32_t type : types) {
    PfChannelChunk chunk{};
    passed = passed && checkout_layer_channel(host_effect_ref(), &ref, 0, 1, 1, type, &chunk) == 0 &&
        chunk.data != nullptr && chunk.row_bytes == kChannelFixtureWidth * channel_element_size(type);
    if (type == kDataFloat && chunk.data) {
      float value = -1.0f;
      std::memcpy(&value, chunk.data, sizeof(value));
      passed = passed && value == 0.0f;
    }
    PfChannelChunk forged = chunk;
    passed = passed && checkin_layer_channel(host_effect_ref(), &ref, &forged) == kPfBadCallbackParam &&
        checkin_layer_channel(host_effect_ref(), &ref, &chunk) == 0 &&
        checkin_layer_channel(host_effect_ref(), &ref, &chunk) == kPfBadCallbackParam;
  }
  PfChannelChunk unsupported{};
  passed = passed && checkout_layer_channel(host_effect_ref(), &ref, 0, 1, 1,
      0x52424720, &unsupported) == kPfUnrecognizedParamType;
  PfChannelRef forged_ref = ref;
  forged_ref.magic ^= 1;
  passed = passed && checkout_layer_channel(host_effect_ref(), &forged_ref, 0, 1, 1,
      kDataFloat, &unsupported) == kPfBadCallbackParam;
  PfChannelChunk leaked{};
  passed = passed && checkout_layer_channel(host_effect_ref(), &ref, 0, 1, 1,
      kDataFloat, &leaked) == 0;
  reclaim_layer_channels();
  passed = passed && g_live_channel_chunks.empty() &&
      checkout_layer_channel(host_effect_ref(), &ref, 0, 1, 1,
          kDataFloat, &leaked) == kPfBadCallbackParam;
  g_channel_fixture_enabled = false;
  count = -1;
  passed = passed && get_layer_channel_count(host_effect_ref(), 0, &count) == 0 && count == 0;
  return passed;
}

bool verify_pf_ae_channel_transport(const std::filesystem::path& manifest) {
  reclaim_layer_channels();
  g_channel_fixture_enabled = false;
  if (!load_aux_manifest(manifest, g_external_aux_set)) return false;
  g_external_aux_loaded = g_external_aux_active = true;
  int32_t count=-1; uint8_t found=0; PfChannelRef ref{}; PfChannelDesc desc{};
  bool passed=get_layer_channel_count(host_effect_ref(),0,&count)==0&&count>0&&
      get_layer_channel_indexed(host_effect_ref(),0,0,&found,&ref,&desc)==0&&found==1;
  PfChannelChunk exact{}, held{};
  passed=passed&&checkout_layer_channel(host_effect_ref(),&ref,0,1,30,kDataFloat,&exact)==0&&
      exact.data_handle!=nullptr&&exact.data!=nullptr;
  if (!g_external_aux_set.channels.empty()) {
    g_channel_transport_row_bytes = exact.row_bytes;
    g_channel_transport_origin_x = g_external_aux_set.channels.front().origin_x;
    g_channel_transport_origin_y = g_external_aux_set.channels.front().origin_y;
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    const auto live = g_live_channel_chunks.find(&exact);
    if (live != g_live_channel_chunks.end()) g_channel_transport_duration = live->second.duration;
  }
  if (passed && !g_external_aux_set.channels.empty() &&
      !g_external_aux_set.channels.front().samples.empty()) {
    const auto& expected = g_external_aux_set.channels.front().samples.front().values;
    passed = expected && expected->size() == static_cast<std::size_t>(exact.width) * exact.height * exact.dimension;
    for (int32_t y = 0; passed && y < exact.height; ++y)
      for (int32_t x = 0; passed && x < exact.width * exact.dimension; ++x) {
        float actual{};
        std::memcpy(&actual, static_cast<std::byte*>(exact.data) +
                    static_cast<std::ptrdiff_t>(y) * exact.row_bytes + x * 4, 4);
        passed = actual == (*expected)[static_cast<std::size_t>(y) * exact.width * exact.dimension + x];
      }
  }
  passed=passed&&
      checkin_layer_channel(host_effect_ref(),&ref,&exact)==0&&
      checkout_layer_channel(host_effect_ref(),&ref,15,1,30,kDataDouble,&held)==0&&
      held.data_handle!=nullptr&&held.data!=nullptr&&
      checkin_layer_channel(host_effect_ref(),&ref,&held)==0;
  reclaim_layer_channels(); g_external_aux_active=g_external_aux_loaded=false;
  g_external_aux_set={};
  return passed&&g_live_channel_chunks.empty();
}

const ExternalAuxSample* select_native_aux_sample(
    const ExternalAuxChannel& channel, const NativeAuxSampleRequest& request) {
  if (request.time_scale == 0) return nullptr;
  const ExternalAuxSample* selected = nullptr;
  for (const auto& sample : channel.samples) {
    const int64_t lhs = static_cast<int64_t>(sample.time) * request.time_scale;
    const int64_t rhs = static_cast<int64_t>(request.time) * sample.time_scale;
    if (lhs == rhs) return &sample;
    if (sample.sampling == "hold" && lhs < rhs &&
        (!selected || static_cast<int64_t>(selected->time) * sample.time_scale <
                         static_cast<int64_t>(sample.time) * selected->time_scale))
      selected = &sample;
  }
  (void)request.duration;
  return selected;
}

bool verify_pf_ae_channel_native_provider() {
  reclaim_layer_channels();
  g_channel_fixture_enabled = false;
  g_external_aux_active = false;
  g_alpha_as_coverage_params = {0};
  const std::array<float, 4> expected{0.0f, 0.25f, 0.5f, 1.0f};
  bool passed = true;
  for (const int32_t pixel_bytes : {4, 8, 16}) {
    std::vector<unsigned char> argb(static_cast<std::size_t>(4) * pixel_bytes, 0);
    for (std::size_t pixel = 0; pixel < expected.size(); ++pixel) {
      if (pixel_bytes == 4) {
        argb[pixel * 4] = static_cast<uint8_t>(std::lround(expected[pixel] * 255.0f));
      } else if (pixel_bytes == 8) {
        const uint16_t alpha = static_cast<uint16_t>(std::lround(expected[pixel] * 32768.0f));
        std::memcpy(argb.data() + pixel * 8, &alpha, sizeof(alpha));
      } else {
        std::memcpy(argb.data() + pixel * 16, &expected[pixel], sizeof(float));
      }
    }
    publish_alpha_coverage_provider(argb, 2, 2, pixel_bytes, 7, 30);
    int32_t count = 0; uint8_t found = 0; PfChannelRef ref{}; PfChannelDesc desc{};
    passed = passed && get_layer_channel_count(host_effect_ref(), 0, &count) == 0 && count == 1 &&
        get_layer_channel_typed(host_effect_ref(), 0, 0x434f5652, &found, &ref, &desc) == 0 &&
        found == 1 && desc.channel_type == 0x434f5652 && desc.data_type == kDataFloat &&
        desc.dimension == 1;
    PfChannelChunk chunk{};
    passed = passed && checkout_layer_channel(host_effect_ref(), &ref, 7, 5, 30, kDataFloat, &chunk) == 0;
    if (chunk.data) {
      for (std::size_t pixel = 0; pixel < expected.size(); ++pixel) {
        float value{}; std::memcpy(&value, static_cast<std::byte*>(chunk.data) + pixel * 4, 4);
        const float tolerance = pixel_bytes == 4 ? 1.0f / 255.0f : 0.00001f;
        passed = passed && std::abs(value - expected[pixel]) <= tolerance;
      }
    }
    {
      std::lock_guard<std::mutex> lock(g_channel_mutex);
      const auto live = g_live_channel_chunks.find(&chunk);
      passed = passed && live != g_live_channel_chunks.end() && live->second.sample_receipt &&
          live->second.duration == 5 && live->second.sample_time == 7 &&
          live->second.sample_time_scale == 30;
    }
    passed = passed && checkin_layer_channel(host_effect_ref(), &ref, &chunk) == 0 &&
        checkin_layer_channel(host_effect_ref(), &ref, &chunk) == kPfBadCallbackParam;
    clear_native_aux_provider();
  }
  std::atomic<bool> mfr_passed{true};
  std::vector<std::thread> mfr_threads;
  for (int thread_index = 0; thread_index < 8; ++thread_index) {
    mfr_threads.emplace_back([&] {
      const std::vector<unsigned char> argb{128, 1, 2, 3};
      for (int iteration = 0; iteration < 256; ++iteration) {
        publish_alpha_coverage_provider(argb, 1, 1, 4, iteration, 30);
        int32_t count = 0; uint8_t found = 0; PfChannelRef ref{}; PfChannelDesc desc{};
        PfChannelChunk chunk{};
        if (get_layer_channel_count(host_effect_ref(), 0, &count) != 0 || count != 1 ||
            get_layer_channel_typed(host_effect_ref(), 0, 0x434f5652, &found, &ref, &desc) != 0 ||
            !found || checkout_layer_channel(host_effect_ref(), &ref, iteration, 3, 30,
                                              kDataFloat, &chunk) != 0 ||
            checkin_layer_channel(host_effect_ref(), &ref, &chunk) != 0) {
          mfr_passed = false;
          break;
        }
        clear_native_aux_provider();
      }
      reclaim_layer_channels();
      clear_native_aux_provider();
    });
  }
  for (auto& thread : mfr_threads) thread.join();
  passed = passed && mfr_passed.load();
  g_alpha_as_coverage_params.clear();
  reclaim_layer_channels();
  return passed && g_live_channel_chunks.empty();
}


bool load_external_aux_manifest(const std::filesystem::path& path) {
  ExternalAuxSet parsed;
  if (!load_aux_manifest(path, parsed)) return false;
  g_external_aux_set = std::move(parsed);
  g_external_aux_loaded = true;
  return true;
}
void activate_external_aux() { g_external_aux_active = g_external_aux_loaded; }
void deactivate_external_aux() { g_external_aux_active = false; }
TransportStatistics transport_statistics() {
  return {g_channel_transport_row_bytes, g_channel_transport_origin_x,
          g_channel_transport_origin_y, g_channel_transport_duration};
}
uint32_t channel_count_queries() { return g_channel_count_queries.load(std::memory_order_relaxed); }

}  // namespace aexcompat::pf_ae_channel
