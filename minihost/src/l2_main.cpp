#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <new>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_set>
#include <vector>

namespace {
constexpr std::size_t kInSize = 408;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kParamSize = 176;
constexpr std::size_t kMaxParams = 64;
constexpr std::size_t kInAddParam = 16;
constexpr std::size_t kInUtils = 176;
constexpr std::size_t kInEffectRef = 184;
constexpr std::size_t kInVersion = 196;
constexpr std::size_t kInApplicationId = 204;
constexpr std::size_t kInNumParams = 208;
constexpr std::size_t kInGlobalData = 312;
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kInFrameData = 328;
constexpr std::size_t kInPicaBasic = 384;
constexpr std::size_t kOutGlobalData = 40;
constexpr std::size_t kOutNumParams = 48;
constexpr std::size_t kOutSequenceData = 56;
constexpr std::size_t kOutFrameData = 72;
constexpr std::size_t kOutFlags = 96;
constexpr std::size_t kOutMessage = 100;
constexpr std::size_t kOutFlags2 = 400;
constexpr std::size_t kParamType = 12;
constexpr std::size_t kParamName = 16;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = 48;
constexpr int32_t kGlobalSetup = 1;
constexpr int32_t kGlobalSetdown = 3;
constexpr int32_t kParamsSetup = 4;
constexpr int32_t kAbout = 0;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceResetup = 6;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kFrameSetup = 10;
constexpr int32_t kFrameSetdown = 12;
constexpr int32_t kRender = 11;
constexpr int32_t kSmartPreRender = 23;
constexpr int32_t kSmartRender = 24;
constexpr int32_t kSmartRenderGpu = 31;
constexpr int32_t kGpuDeviceSetup = 32;
constexpr int32_t kGpuDeviceSetdown = 33;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsNewHandle = 160;
constexpr std::size_t kUtilsLockHandle = 168;
constexpr std::size_t kUtilsUnlockHandle = 176;
constexpr std::size_t kUtilsDisposeHandle = 184;

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AddParamCallback = int32_t(__cdecl*)(void*, int32_t, void*);

struct ParamRecord {
  int32_t index{};
  int32_t type{};
  uint32_t flags{};
  std::string name;
  bool has_numeric{};
  double valid_min{};
  double valid_max{};
  double slider_min{};
  double slider_max{};
  double default_value{};
  double current_value{};
  bool has_current{};
  int32_t precision{-1};
  std::string choices;
  std::string label;
  std::array<std::byte, kParamSize> raw{};
};
std::vector<ParamRecord> g_params;
std::array<std::byte, kParamSize> g_checkout_definition{};
std::atomic_bool g_checkout_map_available{false};
void* g_smart_input_world = nullptr;
void* g_smart_output_world = nullptr;
void* g_smart_map_world = nullptr;
int32_t g_smart_width = 16;
int32_t g_smart_height = 12;
int32_t g_smart_map_width = 0;
int32_t g_smart_map_height = 0;
int32_t g_checkout_time = 0;
int32_t g_checkout_time_step = 0;
uint32_t g_checkout_time_scale = 0;
std::array<int32_t, 4> g_input_checkout_request{-1, -1, -1, -1};
std::array<int32_t, 4> g_map_checkout_request{-1, -1, -1, -1};
std::string g_smart_pixel_format = "argb8";
int32_t g_smart_rowbytes = 64;

void write_rect(void* destination, int32_t width, int32_t height) {
  auto* bytes = static_cast<std::byte*>(destination);
  const int32_t values[4] = {0, 0, height, width};
  std::memcpy(bytes, values, sizeof(values));
}

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time, int32_t time_step, uint32_t time_scale,
                                   void* result) {
  if (request && index == 0 && checkout_id == 0)
    std::memcpy(g_input_checkout_request.data(), request, sizeof(g_input_checkout_request));
  if (request && index == 6 && checkout_id == 1)
    std::memcpy(g_map_checkout_request.data(), request, sizeof(g_map_checkout_request));
  if (index == 0 && checkout_id == 0) {
    g_checkout_time = what_time; g_checkout_time_step = time_step; g_checkout_time_scale = time_scale;
  }
  if (!result) return 4;
  if (index == 0 && checkout_id == 0) {
    write_rect(result, g_smart_width, g_smart_height);
    write_rect(static_cast<std::byte*>(result) + 16, g_smart_width, g_smart_height);
    return 0;
  }
  if (index == 6 && checkout_id == 1 && g_smart_map_world) {
    write_rect(result, g_smart_map_width, g_smart_map_height);
    write_rect(static_cast<std::byte*>(result) + 16, g_smart_map_width, g_smart_map_height);
    return 0;
  }
  return 4;
}
int32_t __cdecl smart_checkout_pixels(void*, int32_t checkout_id, void** world) {
  if (!world) return 4;
  if (checkout_id == 0 && g_smart_input_world) *world = g_smart_input_world;
  else if (checkout_id == 1 && g_smart_map_world) *world = g_smart_map_world;
  else return 4;
  return 0;
}
int32_t __cdecl smart_checkin_pixels(void*, int32_t) { return 0; }
int32_t __cdecl smart_checkout_output(void*, void** world) {
  if (!world || !g_smart_output_world) return 4;
  *world = g_smart_output_world;
  return 0;
}
struct HandleRecord { void* data{}; std::size_t size{}; };
std::unordered_set<HandleRecord*> g_handles;

void** __cdecl new_handle(uint64_t size) {
  std::cerr << "callback:new_handle size=" << size << "\n" << std::flush;
  if (size > 64 * 1024 * 1024) return nullptr;
  auto* record = new (std::nothrow) HandleRecord;
  if (!record) return nullptr;
  record->data = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!record->data && size != 0) { delete record; return nullptr; }
  if (record->data) std::memset(record->data, 0, static_cast<std::size_t>(size));
  record->size = static_cast<std::size_t>(size);
  g_handles.insert(record);
  return &record->data;
}

void* __cdecl lock_handle(void** handle) {
  std::cerr << "callback:lock_handle\n" << std::flush;
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  return record && g_handles.count(record) ? record->data : nullptr;
}

void __cdecl unlock_handle(void**) {}

void __cdecl dispose_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  if (!record || !g_handles.erase(record)) return;
  ::operator delete(record->data);
  delete record;
}

uint64_t __cdecl handle_size(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  return record && g_handles.count(record) ? record->size : 0;
}

int32_t __cdecl resize_handle(uint64_t size, void*** handle) {
  if (!handle || !*handle || size > 64 * 1024 * 1024) return 4;
  auto* record = reinterpret_cast<HandleRecord*>(*handle);
  if (!g_handles.count(record)) return 4;
  void* replacement = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!replacement && size) return 1;
  if (replacement) {
    std::memset(replacement, 0, static_cast<std::size_t>(size));
    std::memcpy(replacement, record->data, (std::min)(record->size, static_cast<std::size_t>(size)));
  }
  ::operator delete(record->data);
  record->data = replacement;
  record->size = static_cast<std::size_t>(size);
  return 0;
}

struct HandleSuite {
  decltype(&new_handle) create;
  decltype(&lock_handle) lock;
  decltype(&unlock_handle) unlock;
  decltype(&dispose_handle) dispose;
  decltype(&handle_size) size;
  decltype(&resize_handle) resize;
};
HandleSuite g_handle_suite{&new_handle, &lock_handle, &unlock_handle,
                           &dispose_handle, &handle_size, &resize_handle};

int32_t __cdecl acquire_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return 4;
  *suite = nullptr;
  if (name && std::strcmp(name, "PF Handle Suite") == 0 && version == 2) {
    *suite = &g_handle_suite;
    return 0;
  }
  return 1;
}

int32_t __cdecl release_suite(const char*, int32_t) { return 0; }

struct BasicSuite {
  decltype(&acquire_suite) acquire;
  decltype(&release_suite) release;
  void* unsupported[5]{};
};
BasicSuite g_basic_suite{&acquire_suite, &release_suite};

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

int32_t __cdecl add_param(void*, int32_t index, void* definition) {
  if (!definition || g_params.size() >= kMaxParams) return 4;
  std::array<std::byte, kParamSize> bytes{};
  std::memcpy(bytes.data(), definition, bytes.size());
  const char* name = reinterpret_cast<const char*>(bytes.data() + kParamName);
  const auto length = strnlen_s(name, kParamNameSize);
  ParamRecord record{index, read<int32_t>(bytes, kParamType),
                     read<uint32_t>(bytes, kParamFlags), std::string(name, length)};
  constexpr std::size_t u = 56;
  if (record.type == 1) {
    record.has_numeric = true;
    record.valid_min = read<int32_t>(bytes, u + 68);
    record.valid_max = read<int32_t>(bytes, u + 72);
    record.slider_min = read<int32_t>(bytes, u + 76);
    record.slider_max = read<int32_t>(bytes, u + 80);
    record.default_value = read<int32_t>(bytes, u + 84);
  } else if (record.type == 7) {
    record.has_numeric = true;
    record.valid_min = 1;
    record.valid_max = read<int16_t>(bytes, u + 4);
    record.slider_min = record.valid_min;
    record.slider_max = record.valid_max;
    record.default_value = read<int16_t>(bytes, u + 6);
    const char* choices = read<const char*>(bytes, u + 8);
    if (choices) record.choices.assign(choices, strnlen_s(choices, 4096));
  } else if (record.type == 4) {
    record.has_numeric = true;
    record.has_current = true;
    record.valid_min = 0;
    record.valid_max = 1;
    record.slider_min = 0;
    record.slider_max = 1;
    record.default_value = read<uint8_t>(bytes, u + 4) ? 1 : 0;
    record.current_value = read<int32_t>(bytes, u) != 0 ? 1 : 0;
    const char* label = read<const char*>(bytes, u + 8);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  } else if (record.type == 10) {
    record.has_numeric = true;
    record.valid_min = read<float>(bytes, u + 48);
    record.valid_max = read<float>(bytes, u + 52);
    record.slider_min = read<float>(bytes, u + 56);
    record.slider_max = read<float>(bytes, u + 60);
    record.default_value = read<float>(bytes, u + 64);
    record.precision = read<int16_t>(bytes, u + 68);
  }
  record.raw = bytes;
  g_params.push_back(std::move(record));
  return 0;
}

int32_t __cdecl checkout_param(void*, int32_t index, int32_t, int32_t, uint32_t, void* definition) {
  if (g_checkout_map_available && index == 6 && definition) {
    std::memcpy(definition, g_checkout_definition.data(), g_checkout_definition.size());
    return 0;
  }
  return 4;
}
int32_t __cdecl checkin_param(void*, void*) { return 0; }

std::string escape(const std::string& input) {
  std::string output;
  for (unsigned char ch : input) {
    if (ch == '"' || ch == '\\') output.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) output.push_back(static_cast<char>(ch));
  }
  return output;
}

bool sha256(const std::filesystem::path& path, std::string& result) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0 ||
      BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                        &returned, 0) < 0) return false;
  std::vector<unsigned char> object(object_size);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0) < 0) return false;
  std::ifstream input(path, std::ios::binary);
  std::array<unsigned char, 65536> buffer{};
  while (input) {
    input.read(reinterpret_cast<char*>(buffer.data()), buffer.size());
    if (input.gcount() > 0 && BCryptHashData(hash, buffer.data(), static_cast<ULONG>(input.gcount()), 0) < 0) return false;
  }
  std::array<unsigned char, 32> digest{};
  const bool ok = input.eof() && BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0;
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  if (!ok) return false;
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  result = text.str();
  return true;
}

std::string sha256_bytes(const unsigned char* data, std::size_t size) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
  BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                    reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &returned, 0);
  std::vector<unsigned char> object(object_size);
  BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0);
  BCryptHashData(hash, const_cast<PUCHAR>(data), static_cast<ULONG>(size), 0);
  std::array<unsigned char, 32> digest{};
  BCryptFinishHash(hash, digest.data(), digest.size(), 0);
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  return text.str();
}

#ifdef AEXCOMPAT_RENDER_WORKER
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& command_output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes,
                    std::string& input_hash, std::string& output_hash,
                    bool& guards_intact) {
  const bool connected_map = case_id == "connected_map" || case_id == "inverted_map";
  const bool partial_extent_hint = case_id == "partial_extent_hint";
  width = connected_map ? 11 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16);
  height = connected_map ? 7 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12);
  rowbytes = case_id == "padded_stride" ? 64 : width * 4;
  int32_t amount = 5, direction = 3, seed = 0, repeat = 1;
  double mix = 100.0;
  if (case_id == "identity") amount = 0;
  else if (case_id == "horizontal") { amount = 9; direction = 1; seed = 17; }
  else if (case_id == "vertical_no_repeat") { amount = 7; direction = 2; repeat = 0; }
  else if (case_id == "mixed") { amount = 12; seed = 991; mix = 37.5; }
  else if (case_id == "amount_max") amount = 500;
  else if (case_id == "seed_max") seed = 10000;
  else if (case_id == "mix_zero") { amount = 500; seed = 10000; mix = 0.0; }
  else if (case_id == "odd_dimensions" || case_id == "padded_stride") { amount = 4; seed = 3; }
  else if (case_id == "inverted_map") { }
  else if (case_id != "default" && case_id != "connected_map" && !partial_extent_hint) return -2;
  constexpr std::size_t guard = 64;
  std::vector<unsigned char> logical_source(width * height * 4);
  std::vector<unsigned char> source(rowbytes * height, 0x5A);
  for (int32_t y = 0; y < height; ++y) {
    for (int32_t x = 0; x < width; ++x) {
      auto* pixel = &logical_source[(y * width + x) * 4];
      pixel[0] = 255;
      pixel[1] = static_cast<unsigned char>(x * 255 / (width - 1));
      pixel[2] = static_cast<unsigned char>(y * 255 / (height - 1));
      pixel[3] = static_cast<unsigned char>((x + y) * 255 / (width + height - 2));
      std::memcpy(&source[y * rowbytes + x * 4], pixel, 4);
    }
  }
  std::vector<unsigned char> guarded(rowbytes * height + guard * 2, 0xA5);
  unsigned char* destination = guarded.data() + guard;
  std::memset(destination, 0xCC, rowbytes * height);

  std::array<std::byte, 120> input_world{}, output_world{};
  auto setup_world = [&](auto& world, void* pixels) {
    write<void*>(world, 24, pixels);
    write<int32_t>(world, 32, rowbytes);
    write<int32_t>(world, 36, width);
    write<int32_t>(world, 40, height);
    write<int32_t>(world, 44, 0);
    write<int32_t>(world, 48, 0);
    write<int32_t>(world, 52, height);
    write<int32_t>(world, 56, width);
  };
  setup_world(input_world, source.data());
  setup_world(output_world, destination);

  std::vector<unsigned char> map_pixels;
  std::array<std::byte, 120> map_world{};
  if (connected_map) {
    const int32_t map_width = case_id == "connected_map" ? 5 : width;
    const int32_t map_height = case_id == "connected_map" ? 3 : height;
    map_pixels.resize(map_width * map_height * 4);
    for (int32_t y = 0; y < map_height; ++y) {
      for (int32_t x = 0; x < map_width; ++x) {
        const unsigned char value = static_cast<unsigned char>((x + y) * 255 / (map_width + map_height - 2));
        auto* pixel = &map_pixels[(y * map_width + x) * 4];
        pixel[0] = 255; pixel[1] = value; pixel[2] = value; pixel[3] = value;
      }
    }
    write<void*>(map_world, 24, map_pixels.data());
    write<int32_t>(map_world, 32, map_width * 4);
    write<int32_t>(map_world, 36, map_width);
    write<int32_t>(map_world, 40, map_height);
    g_checkout_definition.fill(std::byte{});
    write<int32_t>(g_checkout_definition, 12, 0);
    std::memcpy(g_checkout_definition.data() + 56, map_world.data(), map_world.size());
    g_checkout_map_available = true;
  }

  std::array<std::array<std::byte, kParamSize>, 8> definitions{};
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  for (std::size_t i = 0; i < g_params.size() && i + 1 < definitions.size(); ++i) {
    definitions[i + 1] = g_params[i].raw;
    constexpr std::size_t u = 56;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(definitions[i + 1], u, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(definitions[i + 1], u, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 10)
      write<double>(definitions[i + 1], u, g_params[i].default_value);
  }
  write<int32_t>(definitions[1], 56, amount);
  write<int32_t>(definitions[2], 56, direction);
  write<int32_t>(definitions[3], 56, seed);
  write<int32_t>(definitions[4], 56, repeat);
  write<double>(definitions[5], 56, mix);
  if (case_id == "inverted_map") write<int32_t>(definitions[7], 56, 1);
  std::array<void*, 9> params{};
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  write<int32_t>(input, 224, 0);
  write<int32_t>(input, 228, 1);
  write<int32_t>(input, 232, 1);
  write<uint32_t>(input, 240, 1);
  write<int32_t>(input, 252, width);
  write<int32_t>(input, 256, height);
  if (partial_extent_hint) {
    const int32_t extent[4] = {2, 3, 8, 11};
    std::memcpy(input.data() + 260, extent, sizeof(extent));
  }
  input_hash = sha256_bytes(logical_source.data(), logical_source.size());
  const int32_t error = entry(kRender, input.data(), command_output.data(), params.data(),
                              output_world.data(), nullptr);
  g_checkout_map_available = false;
  std::vector<unsigned char> logical_output(width * height * 4);
  for (int32_t y = 0; y < height; ++y)
    std::memcpy(logical_output.data() + y * width * 4, destination + y * rowbytes, width * 4);
  output_hash = sha256_bytes(logical_output.data(), logical_output.size());
  const bool padding_intact = rowbytes == width * 4 || [&] {
    for (int32_t y = 0; y < height; ++y)
      for (int32_t x = width * 4; x < rowbytes; ++x)
        if (destination[y * rowbytes + x] != 0xCC) return false;
    return true;
  }();
  guards_intact = padding_intact && std::all_of(guarded.begin(), guarded.begin() + guard, [](auto b) { return b == 0xA5; }) &&
      std::all_of(guarded.end() - guard, guarded.end(), [](auto b) { return b == 0xA5; });
  return error;
}
#endif

#ifdef AEXCOMPAT_SMART_WORKER
struct SmartResult {
  int32_t gpu_setup_error{};
  int32_t pre_error{-1};
  int32_t render_error{-1};
  int32_t gpu_setdown_error{};
  std::string input_hash;
  std::string output_hash;
  bool rects_valid{};
  bool guards_intact{};
  bool gpu_render_possible{};
  bool gpu_render_dispatched{};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  bool roi_contract_valid{};
};

SmartResult smart_render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& command_output,
                              const std::string& case_id) {
  SmartResult result;
  const bool deep16 = case_id == "deep16_default";
  const bool gpu_negotiation = case_id == "gpu_fallback_float32";
  const bool missing_input = case_id == "error_missing_input";
  const bool crash_null_output = case_id == "crash_null_output_world";
  const bool temporal_context = case_id == "temporal_context";
  const bool partial_output_request = case_id == "partial_output_request";
  const bool float32 = case_id == "float32_default" || gpu_negotiation;
  const bool connected_map = case_id == "connected_map" || case_id == "inverted_map";
  const int32_t width = connected_map ? 11 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16);
  const int32_t height = connected_map ? 7 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12);
  const int32_t pixel_bytes = float32 ? 16 : (deep16 ? 8 : 4);
  const int32_t rowbytes = case_id == "padded_stride" ? 64 : width * pixel_bytes;
  int32_t amount = 5, direction = 3, seed = 0, repeat = 1; double mix = 100.0;
  if (case_id == "identity") amount = 0;
  else if (case_id == "horizontal") { amount = 9; direction = 1; seed = 17; }
  else if (case_id == "vertical_no_repeat") { amount = 7; direction = 2; repeat = 0; }
  else if (case_id == "mixed") { amount = 12; seed = 991; mix = 37.5; }
  else if (case_id == "amount_max") amount = 500;
  else if (case_id == "seed_max") seed = 10000;
  else if (case_id == "mix_zero") { amount = 500; seed = 10000; mix = 0.0; }
  else if (case_id == "odd_dimensions" || case_id == "padded_stride") { amount = 4; seed = 3; }
  else if (case_id != "default" && !deep16 && !float32 && !missing_input && !crash_null_output && !temporal_context && !partial_output_request && !connected_map) return result;
  constexpr std::size_t guard = 64;
  std::vector<unsigned char> source(rowbytes * height, 0x5A);
  for (int32_t y = 0; y < height; ++y) for (int32_t x = 0; x < width; ++x) {
    auto* pixel = &source[y * rowbytes + x * pixel_bytes];
    if (float32) {
      const float values[4] = {1.0f, static_cast<float>(x) / static_cast<float>(width - 1),
          static_cast<float>(y) / static_cast<float>(height - 1),
          static_cast<float>(x + y) / static_cast<float>(width + height - 2)};
      std::memcpy(pixel, values, sizeof(values));
    } else if (deep16) {
      const uint16_t values[4] = {32768, static_cast<uint16_t>(x * 32768 / (width - 1)),
          static_cast<uint16_t>(y * 32768 / (height - 1)),
          static_cast<uint16_t>((x + y) * 32768 / (width + height - 2))};
      std::memcpy(pixel, values, sizeof(values));
    } else {
      pixel[0] = 255; pixel[1] = static_cast<unsigned char>(x * 255 / (width - 1));
      pixel[2] = static_cast<unsigned char>(y * 255 / (height - 1));
      pixel[3] = static_cast<unsigned char>((x + y) * 255 / (width + height - 2));
    }
  }
  std::vector<unsigned char> guarded(rowbytes * height + guard * 2, 0xA5);
  auto* destination = guarded.data() + guard;
  std::memset(destination, 0xCC, rowbytes * height);
  std::array<std::byte, 120> input_world{}, output_world{};
  auto setup_world = [&](auto& world, void* pixels) {
    write<int32_t>(world, 16, (deep16 || float32) ? 1 : 0);
    write<void*>(world, 24, pixels); write<int32_t>(world, 32, rowbytes);
    write<int32_t>(world, 36, width); write<int32_t>(world, 40, height);
    write_rect(world.data() + 44, width, height);
  };
  setup_world(input_world, source.data()); setup_world(output_world, destination);

  std::vector<unsigned char> map_pixels; std::array<std::byte, 120> map_world{};
  if (connected_map) {
    g_smart_map_width = case_id == "connected_map" ? 5 : width;
    g_smart_map_height = case_id == "connected_map" ? 3 : height;
    map_pixels.resize(g_smart_map_width * g_smart_map_height * 4);
    for (int32_t y = 0; y < g_smart_map_height; ++y) for (int32_t x = 0; x < g_smart_map_width; ++x) {
      const auto value = static_cast<unsigned char>((x + y) * 255 / (g_smart_map_width + g_smart_map_height - 2));
      auto* p = &map_pixels[(y * g_smart_map_width + x) * 4]; p[0] = 255; p[1] = value; p[2] = value; p[3] = value;
    }
    write<void*>(map_world, 24, map_pixels.data()); write<int32_t>(map_world, 32, g_smart_map_width * 4);
    write<int32_t>(map_world, 36, g_smart_map_width); write<int32_t>(map_world, 40, g_smart_map_height);
    write_rect(map_world.data() + 44, g_smart_map_width, g_smart_map_height);
    g_smart_map_world = map_world.data();
  }

  std::array<std::array<std::byte, kParamSize>, 8> definitions{};
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  for (std::size_t i = 0; i < g_params.size() && i + 1 < definitions.size(); ++i)
    definitions[i + 1] = g_params[i].raw;
  write<int32_t>(definitions[1], 56, amount); write<int32_t>(definitions[2], 56, direction);
  write<int32_t>(definitions[3], 56, seed); write<int32_t>(definitions[4], 56, repeat);
  write<double>(definitions[5], 56, mix);
  if (case_id == "inverted_map") write<int32_t>(definitions[7], 56, 1);
  std::array<void*, 9> params{};
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  if (crash_null_output)
    entry(kFrameSetup, input.data(), command_output.data(), params.data(), nullptr, nullptr);

  const int32_t current_time = temporal_context ? 42 : 0;
  const int32_t time_step = temporal_context ? 2 : 1;
  const uint32_t time_scale = temporal_context ? 24 : 1;
  write<int32_t>(input, 224, current_time); write<int32_t>(input, 228, time_step);
  write<int32_t>(input, 232, temporal_context ? 240 : 1); write<uint32_t>(input, 240, time_scale);
  g_checkout_time = 0; g_checkout_time_step = 0; g_checkout_time_scale = 0;

  std::array<std::byte, 8> gpu_setup_input{}, gpu_setup_output{};
  std::array<std::byte, 16> gpu_setup_extra{};
  if (gpu_negotiation) {
    write<int32_t>(gpu_setup_input, 0, 4); write<uint32_t>(gpu_setup_input, 4, 0);
    write<void*>(gpu_setup_extra, 0, gpu_setup_input.data()); write<void*>(gpu_setup_extra, 8, gpu_setup_output.data());
    result.gpu_setup_error = entry(kGpuDeviceSetup, input.data(), command_output.data(), params.data(), nullptr, gpu_setup_extra.data());
  }

  std::array<std::byte, 64> pre_input{}; std::array<std::byte, 56> pre_output{};
  std::array<std::byte, 16> pre_callbacks{}; std::array<std::byte, 24> pre_extra{};
  const std::array<int32_t, 4> expected_request = partial_output_request
      ? std::array<int32_t, 4>{2, 3, 8, 11}
      : std::array<int32_t, 4>{0, 0, height, width};
  std::memcpy(pre_input.data(), expected_request.data(), sizeof(expected_request));
  write<void*>(pre_callbacks, 0, reinterpret_cast<void*>(&pre_checkout_layer));
  write<void*>(pre_extra, 0, pre_input.data()); write<void*>(pre_extra, 8, pre_output.data());
  write<void*>(pre_extra, 16, pre_callbacks.data());
  g_input_checkout_request.fill(-1); g_map_checkout_request.fill(-1);
  g_smart_width = width; g_smart_height = height; g_smart_rowbytes = rowbytes;
  g_smart_pixel_format = float32 ? "argb32f" : (deep16 ? "argb16" : "argb8");
  result.pre_error = entry(kSmartPreRender, input.data(), command_output.data(), params.data(), nullptr, pre_extra.data());
  auto valid_rect = [&](std::size_t offset) {
    const auto* p = pre_output.data() + offset;
    const int32_t top = read<int32_t>(pre_output, offset), left = read<int32_t>(pre_output, offset + 4);
    const int32_t bottom = read<int32_t>(pre_output, offset + 8), right = read<int32_t>(pre_output, offset + 12);
    (void)p; return top >= 0 && left >= 0 && bottom >= top && right >= left && bottom <= height && right <= width;
  };
  result.rects_valid = result.pre_error == 0 && valid_rect(0) && valid_rect(16);
  std::array<int32_t, 4> result_rect{}, max_result_rect{};
  std::memcpy(result_rect.data(), pre_output.data(), sizeof(result_rect));
  std::memcpy(max_result_rect.data(), pre_output.data() + 16, sizeof(max_result_rect));
  result.roi_contract_valid = !partial_output_request ||
      (g_input_checkout_request == expected_request && g_map_checkout_request == expected_request &&
       result_rect == expected_request && max_result_rect == expected_request);
  result.gpu_render_possible = (read<uint16_t>(pre_output, 34) & 0x2u) != 0;
  result.checkout_time = g_checkout_time; result.checkout_time_step = g_checkout_time_step;
  result.checkout_time_scale = g_checkout_time_scale;

  std::array<std::byte, 72> smart_input{}; std::array<std::byte, 24> callbacks{};
  std::array<std::byte, 16> smart_extra{};
  write<void*>(callbacks, 0, reinterpret_cast<void*>(&smart_checkout_pixels));
  write<void*>(callbacks, 8, reinterpret_cast<void*>(&smart_checkin_pixels));
  write<void*>(callbacks, 16, reinterpret_cast<void*>(&smart_checkout_output));
  write<void*>(smart_extra, 0, smart_input.data()); write<void*>(smart_extra, 8, callbacks.data());
  g_smart_input_world = missing_input ? nullptr : input_world.data();
  g_smart_output_world = output_world.data();
  const int32_t render_selector = gpu_negotiation && result.gpu_render_possible ? kSmartRenderGpu : kSmartRender;
  result.gpu_render_dispatched = render_selector == kSmartRenderGpu;
  result.render_error = result.pre_error == 0
      ? entry(render_selector, input.data(), command_output.data(), params.data(), nullptr, smart_extra.data()) : -1;
  if (gpu_negotiation) {
    std::array<std::byte, 16> setdown_input{}; std::array<std::byte, 8> setdown_extra{};
    write<void*>(setdown_input, 0, read<void*>(gpu_setup_output, 0));
    write<int32_t>(setdown_input, 8, 4); write<uint32_t>(setdown_input, 12, 0);
    write<void*>(setdown_extra, 0, setdown_input.data());
    result.gpu_setdown_error = entry(kGpuDeviceSetdown, input.data(), command_output.data(), params.data(), nullptr, setdown_extra.data());
  }
  g_smart_input_world = nullptr; g_smart_output_world = nullptr; g_smart_map_world = nullptr;
  std::vector<unsigned char> logical_input(width * height * pixel_bytes), logical_output(width * height * pixel_bytes);
  for (int32_t y = 0; y < height; ++y) {
    std::memcpy(logical_input.data() + y * width * pixel_bytes, source.data() + y * rowbytes, width * pixel_bytes);
    std::memcpy(logical_output.data() + y * width * pixel_bytes, destination + y * rowbytes, width * pixel_bytes);
  }
  result.input_hash = sha256_bytes(logical_input.data(), logical_input.size());
  result.output_hash = sha256_bytes(logical_output.data(), logical_output.size());
  result.guards_intact = std::all_of(guarded.begin(), guarded.begin() + guard, [](auto b) { return b == 0xA5; }) &&
      std::all_of(guarded.end() - guard, guarded.end(), [](auto b) { return b == 0xA5; });
  return result;
}
#endif

void report(const char* status, int32_t global_error, int32_t params_error,
            int32_t setdown_error, const std::array<std::byte, kOutSize>& output,
            const std::string& about_message, const std::array<int32_t, 5>& lifecycle_errors,
            bool lifecycle_data_null) {
  const char* message = reinterpret_cast<const char*>(output.data() + kOutMessage);
  const uint32_t out_flags = read<uint32_t>(output, kOutFlags);
  const uint32_t out_flags2 = read<uint32_t>(output, kOutFlags2);
  std::cout << "{\"schema_version\":1,\"stage\":\"L2\",\"status\":\"" << status
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"reported_num_params\":" << read<int32_t>(output, kOutNumParams)
            << ",\"out_flags\":" << out_flags
            << ",\"out_flags2\":" << out_flags2
            << ",\"update_params_ui_advertised\":" << ((out_flags & (1u << 26)) != 0 ? "true" : "false")
            << ",\"query_dynamic_flags_advertised\":" << ((out_flags2 & 1u) != 0 ? "true" : "false")
            << ",\"conditional_ui_selectors_dispatched\":false"
            << ",\"return_message\":\"" << escape(std::string(message, strnlen_s(message, 256)))
            << "\",\"about_message\":\"" << escape(about_message)
            << "\",\"sequence_setup_error\":" << lifecycle_errors[0]
            << ",\"sequence_resetup_error\":" << lifecycle_errors[1]
            << ",\"frame_setup_error\":" << lifecycle_errors[2]
            << ",\"frame_setdown_error\":" << lifecycle_errors[3]
            << ",\"sequence_setdown_error\":" << lifecycle_errors[4]
            << ",\"lifecycle_data_null\":" << (lifecycle_data_null ? "true" : "false")
            << ",\"parameters\":[";
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (i) std::cout << ',';
    const auto& param = g_params[i];
    std::cout << "{\"index\":" << param.index << ",\"type\":" << param.type
              << ",\"flags\":" << param.flags << ",\"name\":\""
              << escape(param.name) << "\"";
    if (param.has_numeric) {
      std::cout << ",\"valid_min\":" << param.valid_min
                << ",\"valid_max\":" << param.valid_max
                << ",\"slider_min\":" << param.slider_min
                << ",\"slider_max\":" << param.slider_max
                << ",\"default\":" << param.default_value;
    }
    if (param.precision >= 0) std::cout << ",\"precision\":" << param.precision;
    if (param.has_current) {
      std::cout << ",\"current\":" << param.current_value
                << ",\"current_default_mismatch\":"
                << (param.current_value != param.default_value ? "true" : "false");
    }
    if (!param.choices.empty()) std::cout << ",\"choices\":\"" << escape(param.choices) << "\"";
    if (!param.label.empty()) std::cout << ",\"label\":\"" << escape(param.label) << "\"";
    std::cout << '}';
  }
  std::cout << "],\"selectors_executed\":true,\"render_performed\":false}\n";
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
#ifdef AEXCOMPAT_RENDER_WORKER
  if (argc != 5) return 2;
  if (std::wstring(argv[1]) != L"--render") return 2;
#elif defined(AEXCOMPAT_SMART_WORKER)
  if (argc != 5 || std::wstring(argv[1]) != L"--smart") return 2;
#else
  if (argc != 4) return 2;
  if (std::wstring(argv[1]) != L"--l2") return 2;
#endif
  std::string expected;
  for (const wchar_t* p = argv[3]; *p; ++p) {
    if (*p > 0x7f) return 2;
    expected.push_back(static_cast<char>(*p));
  }
  std::string actual;
  if (!sha256(argv[2], actual) || actual != expected) return 10;
  SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS);
  HMODULE module = LoadLibraryExW(argv[2], nullptr,
      LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!module) return 11;
  auto entry = reinterpret_cast<EffectEntry>(GetProcAddress(module, "EffectMain"));
  if (!entry) entry = reinterpret_cast<EffectEntry>(GetProcAddress(module, "EntryPointFunc"));
  if (!entry) { FreeLibrary(module); return 12; }

  alignas(8) std::array<std::byte, kInSize> input{};
  alignas(8) std::array<std::byte, kOutSize> output{};
  alignas(8) std::array<std::byte, kUtilsSize> utils{};
  write(input, 0, &checkout_param);
  write(input, 8, &checkin_param);
  write(input, kInAddParam, static_cast<AddParamCallback>(&add_param));
  write(utils, kUtilsNewHandle, &new_handle);
  write(utils, kUtilsLockHandle, &lock_handle);
  write(utils, kUtilsUnlockHandle, &unlock_handle);
  write(utils, kUtilsDisposeHandle, &dispose_handle);
  write<void*>(input, kInUtils, utils.data());
  write<void*>(input, kInPicaBasic, &g_basic_suite);
  write<void*>(input, kInEffectRef, nullptr);
  write<uint32_t>(input, kInVersion, 0);
  write<uint32_t>(input, kInApplicationId, 0x46585443u);
  write<int32_t>(input, kInNumParams, 1);
#if !defined(AEXCOMPAT_RENDER_WORKER) && !defined(AEXCOMPAT_SMART_WORKER)
  std::array<std::byte, kOutSize> about_output{};
  int32_t about_error = -1;
  std::string about_message;
#endif
  std::cerr << "stage:global_setup_begin\n" << std::flush;
  const int32_t global_error = entry(kGlobalSetup, input.data(), output.data(), nullptr, nullptr, nullptr);
  std::cerr << "stage:global_setup_end error=" << global_error << "\n" << std::flush;
  write<void*>(input, kInGlobalData, read<void*>(output, kOutGlobalData));
#if !defined(AEXCOMPAT_RENDER_WORKER) && !defined(AEXCOMPAT_SMART_WORKER)
  about_error = global_error == 0 ? entry(kAbout, input.data(), about_output.data(), nullptr, nullptr, nullptr) : -1;
  const char* about_text = reinterpret_cast<const char*>(about_output.data() + kOutMessage);
  about_message.assign(about_text, strnlen_s(about_text, 256));
#endif
  std::cerr << "stage:params_setup_begin\n" << std::flush;
  const int32_t params_error = global_error == 0
      ? entry(kParamsSetup, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:params_setup_end error=" << params_error << "\n" << std::flush;
#if !defined(AEXCOMPAT_RENDER_WORKER) && !defined(AEXCOMPAT_SMART_WORKER)
  std::array<std::array<std::byte, kParamSize>, 8> lifecycle_definitions{};
  std::array<unsigned char, 4> lifecycle_pixel{255, 0, 0, 0};
  std::array<std::byte, 120> lifecycle_world{};
  write<void*>(lifecycle_world, 24, lifecycle_pixel.data());
  write<int32_t>(lifecycle_world, 32, 4); write<int32_t>(lifecycle_world, 36, 1);
  write<int32_t>(lifecycle_world, 40, 1); write_rect(lifecycle_world.data() + 44, 1, 1);
  std::memcpy(lifecycle_definitions[0].data() + 56, lifecycle_world.data(), lifecycle_world.size());
  for (std::size_t i = 0; i < g_params.size() && i + 1 < lifecycle_definitions.size(); ++i) {
    lifecycle_definitions[i + 1] = g_params[i].raw;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(lifecycle_definitions[i + 1], 56, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(lifecycle_definitions[i + 1], 56, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 10)
      write<double>(lifecycle_definitions[i + 1], 56, g_params[i].default_value);
  }
  std::array<void*, 9> lifecycle_params{};
  for (std::size_t i = 0; i < lifecycle_definitions.size(); ++i)
    lifecycle_params[i] = lifecycle_definitions[i].data();
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  std::cerr << "stage:sequence_setup_begin\n" << std::flush;
  lifecycle_errors[0] = params_error == 0 ? entry(kSequenceSetup, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:sequence_setup_end error=" << lifecycle_errors[0] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  std::cerr << "stage:sequence_resetup_begin\n" << std::flush;
  lifecycle_errors[1] = lifecycle_errors[0] == 0 ? entry(kSequenceResetup, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:sequence_resetup_end error=" << lifecycle_errors[1] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  std::cerr << "stage:frame_setup_begin\n" << std::flush;
  lifecycle_errors[2] = lifecycle_errors[1] == 0 ? entry(kFrameSetup, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setup_end error=" << lifecycle_errors[2] << "\n" << std::flush;
  write<void*>(input, kInFrameData, read<void*>(output, kOutFrameData));
  std::cerr << "stage:frame_setdown_begin\n" << std::flush;
  lifecycle_errors[3] = lifecycle_errors[2] == 0 ? entry(kFrameSetdown, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setdown_end error=" << lifecycle_errors[3] << "\n" << std::flush;
  std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
  lifecycle_errors[4] = lifecycle_errors[3] == 0 ? entry(kSequenceSetdown, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:sequence_setdown_end error=" << lifecycle_errors[4] << "\n" << std::flush;
  const bool lifecycle_data_null = read<void*>(output, kOutSequenceData) == nullptr && read<void*>(output, kOutFrameData) == nullptr;
#endif
#ifdef AEXCOMPAT_RENDER_WORKER
  std::string case_id;
  for (const wchar_t* p = argv[4]; *p; ++p) {
    if (*p > 0x7f) return 2;
    case_id.push_back(static_cast<char>(*p));
  }
  std::string input_hash, output_hash;
  bool guards_intact = false;
  int32_t render_width = 0, render_height = 0, render_rowbytes = 0;
  std::array<int32_t, 2> thread_errors{-1, -1};
  std::array<std::string, 2> thread_hashes{};
  std::array<bool, 2> thread_guards{false, false};
  const bool concurrent_render = case_id == "threaded_default";
  std::cerr << "stage:render_begin\n" << std::flush;
  int32_t render_error = -1;
  if (params_error == 0 && concurrent_render) {
    std::array<int32_t, 2> widths{}, heights{}, rowbytes{};
    std::array<std::string, 2> input_hashes{};
    auto run_thread = [&](std::size_t index) {
      auto thread_input = input;
      auto thread_output = output;
      thread_errors[index] = render_once(entry, thread_input, thread_output, "default",
          widths[index], heights[index], rowbytes[index], input_hashes[index],
          thread_hashes[index], thread_guards[index]);
    };
    std::thread first(run_thread, 0); std::thread second(run_thread, 1);
    first.join(); second.join();
    render_width = widths[0]; render_height = heights[0]; render_rowbytes = rowbytes[0];
    input_hash = input_hashes[0]; output_hash = thread_hashes[0];
    guards_intact = thread_guards[0] && thread_guards[1];
    render_error = thread_errors[0] == 0 && thread_errors[1] == 0 &&
        widths[0] == widths[1] && heights[0] == heights[1] && rowbytes[0] == rowbytes[1] &&
        input_hashes[0] == input_hashes[1] && thread_hashes[0] == thread_hashes[1] ? 0 : -1;
  } else if (params_error == 0) {
    render_error = render_once(entry, input, output, case_id, render_width, render_height,
                               render_rowbytes, input_hash, output_hash, guards_intact);
  }
  std::cerr << "stage:render_end error=" << render_error << "\n" << std::flush;
#elif defined(AEXCOMPAT_SMART_WORKER)
  std::string case_id;
  for (const wchar_t* p = argv[4]; *p; ++p) { if (*p > 0x7f) return 2; case_id.push_back(static_cast<char>(*p)); }
  std::cerr << "stage:smart_render_begin\n" << std::flush;
  const SmartResult smart = params_error == 0 ? smart_render_once(entry, input, output, case_id) : SmartResult{};
  std::cerr << "stage:smart_render_end pre_error=" << smart.pre_error
            << " render_error=" << smart.render_error << "\n" << std::flush;
#endif
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = global_error == 0
      ? entry(kGlobalSetdown, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  std::cerr << "stage:global_setdown_end error=" << setdown_error << "\n" << std::flush;
#ifdef AEXCOMPAT_RENDER_WORKER
  std::cout << "{\"schema_version\":1,\"stage\":\"classic_render\",\"status\":\""
            << (render_error == 0 && guards_intact ? "render_completed" : "render_failed")
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"render_error\":" << render_error
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"case_id\":\"" << case_id << "\",\"pixel_format\":\"" << g_smart_pixel_format << "\",\"width\":"
            << render_width << ",\"height\":" << render_height << ",\"rowbytes\":" << render_rowbytes
            << ",\"bytes_written_per_row\":" << render_width * 4
            << ",\"undefined_tail_bytes_per_row\":" << std::max(0, render_rowbytes - render_width * 4)
            << ",\"input_sha256\":\"" << input_hash << "\",\"output_sha256\":\""
            << output_hash << "\",\"guard_bytes_intact\":" << (guards_intact ? "true" : "false")
            << ",\"concurrent_render\":" << (concurrent_render ? "true" : "false")
            << ",\"thread_1_error\":" << thread_errors[0]
            << ",\"thread_2_error\":" << thread_errors[1]
            << ",\"thread_1_sha256\":\"" << thread_hashes[0] << "\""
            << ",\"thread_2_sha256\":\"" << thread_hashes[1] << "\""
            << ",\"thread_1_guards_intact\":" << (thread_guards[0] ? "true" : "false")
            << ",\"thread_2_guards_intact\":" << (thread_guards[1] ? "true" : "false")
            << ",\"render_performed\":true}\n";
#elif defined(AEXCOMPAT_SMART_WORKER)
  std::cout << "{\"schema_version\":1,\"stage\":\"smartfx_render\",\"status\":\""
            << (smart.pre_error == 0 && smart.render_error == 0 && smart.rects_valid && smart.guards_intact
                ? "render_completed" : "render_failed")
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"pre_render_error\":" << smart.pre_error
            << ",\"smart_render_error\":" << smart.render_error
            << ",\"gpu_device_setup_error\":" << smart.gpu_setup_error
            << ",\"gpu_device_setdown_error\":" << smart.gpu_setdown_error
            << ",\"gpu_render_possible\":" << (smart.gpu_render_possible ? "true" : "false")
            << ",\"gpu_render_dispatched\":" << (smart.gpu_render_dispatched ? "true" : "false")
            << ",\"checkout_time\":" << smart.checkout_time
            << ",\"checkout_time_step\":" << smart.checkout_time_step
            << ",\"checkout_time_scale\":" << smart.checkout_time_scale
            << ",\"roi_contract_valid\":" << (smart.roi_contract_valid ? "true" : "false")
            << ",\"input_checkout_request\":[" << g_input_checkout_request[1] << "," << g_input_checkout_request[0]
            << "," << g_input_checkout_request[3] << "," << g_input_checkout_request[2] << "]"
            << ",\"map_checkout_request\":[" << g_map_checkout_request[1] << "," << g_map_checkout_request[0]
            << "," << g_map_checkout_request[3] << "," << g_map_checkout_request[2] << "]"
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"case_id\":\"" << case_id << "\",\"pixel_format\":\"" << g_smart_pixel_format << "\",\"width\":"
            << g_smart_width << ",\"height\":" << g_smart_height << ",\"rowbytes\":"
            << g_smart_rowbytes
            << ",\"bytes_written_per_row\":" << g_smart_width * 4
            << ",\"undefined_tail_bytes_per_row\":" << std::max(0, g_smart_rowbytes - g_smart_width * 4)
            << ",\"input_sha256\":\"" << smart.input_hash << "\",\"output_sha256\":\""
            << smart.output_hash << "\",\"result_rects_valid\":" << (smart.rects_valid ? "true" : "false")
            << ",\"guard_bytes_intact\":" << (smart.guards_intact ? "true" : "false")
            << ",\"render_performed\":true}\n";
#else
  report(global_error == 0 && params_error == 0 ? "selectors_completed" : "selector_error",
         global_error, params_error, setdown_error, output, about_message, lifecycle_errors, lifecycle_data_null);
#endif
  FreeLibrary(module);
#ifdef AEXCOMPAT_RENDER_WORKER
  return global_error == 0 && params_error == 0 && render_error == 0 && guards_intact ? 0 : 21;
#elif defined(AEXCOMPAT_SMART_WORKER)
  return global_error == 0 && params_error == 0 && smart.pre_error == 0 && smart.render_error == 0 &&
      smart.rects_valid && smart.guards_intact ? 0 : 22;
#else
  return global_error == 0 && params_error == 0 && about_error == 0 && lifecycle_data_null &&
      std::all_of(lifecycle_errors.begin(), lifecycle_errors.end(), [](auto error) { return error == 0; }) ? 0 : 20;
#endif
}
