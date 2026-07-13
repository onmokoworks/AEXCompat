#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cwchar>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <map>
#include <mutex>
#include <new>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_set>
#include <utility>
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
  bool has_color{};
  std::array<unsigned char, 4> default_color{};
  std::array<unsigned char, 4> current_color{};
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
bool g_mask_model_enabled = false;
enum class MaskFault { None, CountError, CountCrash };
MaskFault g_mask_fault = MaskFault::None;
std::string g_mask_scene_id = "none";

struct OpaqueHostObject { uint32_t tag; };
struct MaskVertex {
  double x, y;
  double tangent_in_x, tangent_in_y;
  double tangent_out_x, tangent_out_y;
};
struct HostMask {
  OpaqueHostObject mask{0x4d41534b};
  OpaqueHostObject stream{0x5354524d};
  OpaqueHostObject outline{0x4f55544c};
  bool open{};
  bool mask_live{};
  bool stream_live{};
  bool value_live{};
  std::vector<MaskVertex> vertices;
};
OpaqueHostObject g_effect{0x45464658};
OpaqueHostObject g_layer{0x4c415952};
std::vector<HostMask> g_mask_scene;
struct MaskLifetimeCounts {
  uint32_t masks_acquired{};
  uint32_t masks_disposed{};
  uint32_t streams_acquired{};
  uint32_t streams_disposed{};
  uint32_t values_acquired{};
  uint32_t values_disposed{};
};
MaskLifetimeCounts g_mask_lifetime;

bool mask_lifetimes_balanced() {
  return g_mask_lifetime.masks_acquired == g_mask_lifetime.masks_disposed &&
      g_mask_lifetime.streams_acquired == g_mask_lifetime.streams_disposed &&
      g_mask_lifetime.values_acquired == g_mask_lifetime.values_disposed &&
      std::none_of(g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) {
        return mask.mask_live || mask.stream_live || mask.value_live;
      });
}

bool configure_mask_scene(const std::string& scene_id) {
  g_mask_scene.clear();
  g_mask_lifetime = {};
  g_mask_scene_id = scene_id;
  const auto rectangle = [](double left, double top, double right, double bottom) {
    HostMask mask;
    mask.vertices = {{left, top, 0, 0, 0, 0}, {right, top, 0, 0, 0, 0},
                     {right, bottom, 0, 0, 0, 0}, {left, bottom, 0, 0, 0, 0},
                     {left, top, 0, 0, 0, 0}};
    return mask;
  };
  if (scene_id == "rectangle") g_mask_scene.push_back(rectangle(4, 3, 12, 9));
  else if (scene_id == "translated_rectangle") g_mask_scene.push_back(rectangle(2, 2, 10, 8));
  else if (scene_id == "two_rectangles") {
    g_mask_scene.reserve(2);
    g_mask_scene.push_back(rectangle(1, 1, 7, 6));
    g_mask_scene.push_back(rectangle(9, 5, 15, 11));
  } else if (scene_id != "empty") return false;
  return true;
}

HostMask* find_mask(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.mask; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}
HostMask* find_stream(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.stream; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}
HostMask* find_outline(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.outline; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}

std::size_t mask_open_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return mask.open; }));
}

std::size_t mask_tangent_vertex_count() {
  std::size_t count = 0;
  for (const auto& mask : g_mask_scene) {
    const auto end = !mask.open && !mask.vertices.empty()
        ? mask.vertices.end() - 1 : mask.vertices.end();
    count += static_cast<std::size_t>(std::count_if(mask.vertices.begin(), end,
        [](const auto& vertex) {
          return vertex.tangent_in_x != 0 || vertex.tangent_in_y != 0 ||
                 vertex.tangent_out_x != 0 || vertex.tangent_out_y != 0;
        }));
  }
  return count;
}

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
struct HandleRecord {
  void* data{};
  std::size_t size{};
  uint32_t lock_count{};
};
std::unordered_set<HandleRecord*> g_handles;
std::mutex g_handle_mutex;
uint32_t g_handles_created{};
uint32_t g_handles_disposed{};
uint32_t g_handle_locks{};
uint32_t g_handle_unlocks{};
uint32_t g_invalid_handle_operations{};
uint64_t g_handle_bytes{};
constexpr uint64_t kMaxHandleBytes = 64 * 1024 * 1024;
constexpr std::size_t kMaxHandleCount = 1024;

bool handle_lifetimes_balanced() {
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  return g_handles.empty() && g_handles_created == g_handles_disposed &&
      g_handle_locks == g_handle_unlocks;
}

void** __cdecl new_handle(uint64_t size) {
  std::cerr << "callback:new_handle size=" << size << "\n" << std::flush;
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  if (size > kMaxHandleBytes || g_handles.size() >= kMaxHandleCount ||
      g_handle_bytes > kMaxHandleBytes - size) {
    ++g_invalid_handle_operations;
    return nullptr;
  }
  auto* record = new (std::nothrow) HandleRecord;
  if (!record) return nullptr;
  record->data = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!record->data && size != 0) { delete record; return nullptr; }
  if (record->data) std::memset(record->data, 0, static_cast<std::size_t>(size));
  record->size = static_cast<std::size_t>(size);
  g_handles.insert(record);
  ++g_handles_created;
  g_handle_bytes += size;
  return &record->data;
}

void* __cdecl lock_handle(void** handle) {
  std::cerr << "callback:lock_handle\n" << std::flush;
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  if (!record || !g_handles.count(record)) {
    ++g_invalid_handle_operations;
    return nullptr;
  }
  ++record->lock_count;
  ++g_handle_locks;
  return record->data;
}

void __cdecl unlock_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  if (!record || !g_handles.count(record) || record->lock_count == 0) {
    ++g_invalid_handle_operations;
    return;
  }
  --record->lock_count;
  ++g_handle_unlocks;
}

void __cdecl dispose_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  {
    std::lock_guard<std::mutex> lock(g_handle_mutex);
    if (!record || !g_handles.count(record) || record->lock_count != 0) {
      ++g_invalid_handle_operations;
      return;
    }
    g_handles.erase(record);
    ++g_handles_disposed;
    g_handle_bytes -= record->size;
  }
  ::operator delete(record->data);
  delete record;
}

uint64_t __cdecl handle_size(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  if (!record || !g_handles.count(record)) {
    ++g_invalid_handle_operations;
    return 0;
  }
  return record->size;
}

int32_t __cdecl resize_handle(uint64_t size, void*** handle) {
  std::lock_guard<std::mutex> lock(g_handle_mutex);
  if (!handle || !*handle || size > kMaxHandleBytes) {
    ++g_invalid_handle_operations;
    return 4;
  }
  auto* record = reinterpret_cast<HandleRecord*>(*handle);
  if (!g_handles.count(record) || record->lock_count != 0 ||
      g_handle_bytes - record->size > kMaxHandleBytes - size) {
    ++g_invalid_handle_operations;
    return 4;
  }
  void* replacement = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!replacement && size) return 1;
  if (replacement) {
    std::memset(replacement, 0, static_cast<std::size_t>(size));
    std::memcpy(replacement, record->data, (std::min)(record->size, static_cast<std::size_t>(size)));
  }
  ::operator delete(record->data);
  record->data = replacement;
  g_handle_bytes = g_handle_bytes - record->size + size;
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

bool verify_handle_resize_while_locked_rejected() {
  const uint32_t invalid_before = g_invalid_handle_operations;
  void** handle = new_handle(16);
  if (!handle || !lock_handle(handle)) return false;
  const int32_t resize_error = resize_handle(32, &handle);
  unlock_handle(handle);
  dispose_handle(handle);
  return resize_error != 0 && g_invalid_handle_operations == invalid_before + 1 &&
      handle_lifetimes_balanced();
}

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id) {
  if (!plugin_id) return 4;
  *plugin_id = 1;
  return 0;
}

int32_t __cdecl get_effect_layer(void* effect, void** layer) {
  if (effect != &g_effect || !layer) return 4;
  *layer = &g_layer;
  return 0;
}

int32_t __cdecl get_layer_num_masks(void* layer, int32_t* count) {
  if (layer != &g_layer || !count) return 4;
  if (g_mask_fault == MaskFault::CountCrash) {
    RaiseException(EXCEPTION_ACCESS_VIOLATION, 0, 0, nullptr);
  }
  if (g_mask_fault == MaskFault::CountError) return 4;
  *count = static_cast<int32_t>(g_mask_scene.size());
  return 0;
}

int32_t __cdecl get_layer_mask_by_index(void* layer, int32_t index, void** mask) {
  if (layer != &g_layer || index < 0 ||
      static_cast<std::size_t>(index) >= g_mask_scene.size() || !mask) return 4;
  auto& record = g_mask_scene[static_cast<std::size_t>(index)];
  if (record.mask_live) return 4;
  record.mask_live = true;
  ++g_mask_lifetime.masks_acquired;
  *mask = &record.mask;
  return 0;
}

int32_t __cdecl dispose_mask(void* mask) {
  HostMask* record = find_mask(mask);
  if (!record || !record->mask_live) return 4;
  record->mask_live = false;
  ++g_mask_lifetime.masks_disposed;
  return 0;
}

int32_t __cdecl get_new_mask_stream(int32_t plugin_id, void* mask, int32_t, void** stream) {
  HostMask* record = find_mask(mask);
  if (plugin_id != 1 || !record || !record->mask_live || record->stream_live || !stream) return 4;
  record->stream_live = true;
  ++g_mask_lifetime.streams_acquired;
  *stream = &record->stream;
  return 0;
}

int32_t __cdecl dispose_stream(void* stream) {
  HostMask* record = find_stream(stream);
  if (!record || !record->stream_live || record->value_live) return 4;
  record->stream_live = false;
  ++g_mask_lifetime.streams_disposed;
  return 0;
}

struct StreamValue {
  void* stream;
  void* value;
};

int32_t __cdecl get_new_stream_value(int32_t plugin_id, void* stream, int32_t,
                                     const void*, int32_t, StreamValue* value) {
  HostMask* record = find_stream(stream);
  if (plugin_id != 1 || !record || !record->stream_live || record->value_live || !value) return 4;
  record->value_live = true;
  ++g_mask_lifetime.values_acquired;
  value->stream = &record->stream;
  value->value = &record->outline;
  return 0;
}

int32_t __cdecl dispose_stream_value(StreamValue* value) {
  if (!value) return 4;
  HostMask* stream_record = find_stream(value->stream);
  HostMask* outline_record = find_outline(value->value);
  if (!stream_record || stream_record != outline_record || !stream_record->value_live) return 4;
  stream_record->value_live = false;
  ++g_mask_lifetime.values_disposed;
  value->stream = nullptr;
  value->value = nullptr;
  return 0;
}

int32_t __cdecl is_mask_outline_open(void* outline, int32_t* open) {
  HostMask* record = find_outline(outline);
  if (!record || !open) return 4;
  *open = record->open ? 1 : 0;
  return 0;
}

int32_t __cdecl get_mask_outline_num_segments(void* outline, int32_t* count) {
  HostMask* record = find_outline(outline);
  if (!record || !count || record->vertices.empty()) return 4;
  *count = static_cast<int32_t>(record->vertices.size() - 1);
  return 0;
}

int32_t __cdecl get_mask_outline_vertex_info(void* outline, int32_t index,
                                             MaskVertex* vertex) {
  HostMask* record = find_outline(outline);
  if (!record || !vertex || index < 0 ||
      static_cast<std::size_t>(index) >= record->vertices.size()) return 4;
  *vertex = record->vertices[static_cast<std::size_t>(index)];
  return 0;
}

bool verify_mask_double_dispose_rejected() {
  void* mask = nullptr;
  return get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      dispose_mask(mask) == 0 && dispose_mask(mask) == 4 &&
      mask_lifetimes_balanced();
}

bool verify_stream_dispose_with_live_value_rejected() {
  void* mask = nullptr;
  void* stream = nullptr;
  StreamValue value{};
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 0, &stream) != 0 ||
      get_new_stream_value(1, stream, 0, nullptr, 0, &value) != 0)
    return false;
  const int32_t premature_error = dispose_stream(stream);
  return premature_error == 4 && dispose_stream_value(&value) == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0 &&
      mask_lifetimes_balanced();
}

struct UtilitySuite {
  // Function pointer positions mirror the reviewed Adobe suite versions.
  void* unsupported[9]{};
  decltype(&register_with_aegp) register_with_aegp;
};
struct PfInterfaceSuite {
  decltype(&get_effect_layer) get_effect_layer;
  void* unsupported[4]{};
};
struct MaskSuite {
  decltype(&get_layer_num_masks) get_layer_num_masks;
  decltype(&get_layer_mask_by_index) get_layer_mask_by_index;
  decltype(&dispose_mask) dispose_mask;
  void* unsupported[17]{};
};
struct StreamSuite {
  void* unsupported_before_new_mask[6]{};
  decltype(&get_new_mask_stream) get_new_mask_stream;
  decltype(&dispose_stream) dispose_stream;
  void* unsupported_before_value[5]{};
  decltype(&get_new_stream_value) get_new_stream_value;
  decltype(&dispose_stream_value) dispose_stream_value;
};
struct MaskOutlineSuite {
  decltype(&is_mask_outline_open) is_open;
  void* set_open{};
  decltype(&get_mask_outline_num_segments) get_num_segments;
  decltype(&get_mask_outline_vertex_info) get_vertex_info;
  void* unsupported[6]{};
};

UtilitySuite g_utility_suite{{}, &register_with_aegp};
PfInterfaceSuite g_pf_interface_suite{&get_effect_layer};
MaskSuite g_mask_suite{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask};
StreamSuite g_stream_suite{{}, &get_new_mask_stream, &dispose_stream, {},
                           &get_new_stream_value, &dispose_stream_value};
MaskOutlineSuite g_mask_outline_suite{&is_mask_outline_open, nullptr,
                                      &get_mask_outline_num_segments,
                                      &get_mask_outline_vertex_info};

int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format) {
  if (!world || !pixel_format) return 4;
  const auto* bytes = static_cast<const std::byte*>(world);
  int32_t rowbytes{};
  int32_t width{};
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  if (width <= 0 || rowbytes == (std::numeric_limits<int32_t>::min)()) return 4;
  const int32_t bytes_per_pixel = std::abs(rowbytes) / width;
  if (bytes_per_pixel >= 16)
    *pixel_format = 842229089;  // PF_PixelFormat_ARGB128
  else if (bytes_per_pixel >= 8)
    *pixel_format = 909206881;  // PF_PixelFormat_ARGB64
  else
    *pixel_format = 1650946657;  // PF_PixelFormat_ARGB32
  return 0;
}

struct WorldSuite {
  void* new_world{};
  void* dispose_world{};
  decltype(&get_pixel_format) get_pixel_format;
};
WorldSuite g_world_suite{nullptr, nullptr, &get_pixel_format};

std::mutex g_suite_lease_mutex;
std::map<std::pair<std::string, int32_t>, uint32_t> g_suite_leases;
uint32_t g_suite_acquires{};
uint32_t g_suite_releases{};

void record_suite_acquire(const char* name, int32_t version) {
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  ++g_suite_leases[{name, version}];
  ++g_suite_acquires;
}

bool suite_leases_balanced() {
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  return g_suite_acquires == g_suite_releases &&
      std::all_of(g_suite_leases.begin(), g_suite_leases.end(),
                  [](const auto& lease) { return lease.second == 0; });
}

std::size_t live_suite_lease_count() {
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  return static_cast<std::size_t>(std::count_if(
      g_suite_leases.begin(), g_suite_leases.end(),
      [](const auto& lease) { return lease.second != 0; }));
}

uint32_t live_suite_reference_count() {
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  uint32_t count = 0;
  for (const auto& lease : g_suite_leases) count += lease.second;
  return count;
}

std::string live_suite_lease_summary() {
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  std::ostringstream summary;
  for (const auto& [key, count] : g_suite_leases) {
    if (count == 0) continue;
    if (summary.tellp() > 0) summary << ';';
    summary << key.first << '@' << key.second << '=' << count;
  }
  return summary.str();
}

int32_t __cdecl acquire_suite(const char* name, int32_t version, const void** suite) {
  if (!suite) return 4;
  *suite = nullptr;
  if (name && std::strcmp(name, "PF Handle Suite") == 0 && version == 2) {
    *suite = &g_handle_suite;
    record_suite_acquire(name, version);
    return 0;
  }
  if (name && std::strcmp(name, "PF World Suite") == 0 && version == 2) {
    *suite = &g_world_suite;
    record_suite_acquire(name, version);
    return 0;
  }
  if (!g_mask_model_enabled || !name) return 1;
  if (std::strcmp(name, "AEGP Utility Suite") == 0 && version == 13)
    *suite = &g_utility_suite;
  else if (std::strcmp(name, "AEGP PF Interface Suite") == 0 && version == 1)
    *suite = &g_pf_interface_suite;
  else if (std::strcmp(name, "AEGP Layer Mask Suite") == 0 && version == 7)
    *suite = &g_mask_suite;
  else if (std::strcmp(name, "AEGP Stream Suite") == 0 && version == 11)
    *suite = &g_stream_suite;
  else if (std::strcmp(name, "AEGP Mask Outline Suite") == 0 && version == 5)
    *suite = &g_mask_outline_suite;
  else
    return 1;
  record_suite_acquire(name, version);
  return 0;
}

int32_t __cdecl release_suite(const char* name, int32_t version) {
  if (!name) return 1;
  std::lock_guard<std::mutex> lock(g_suite_lease_mutex);
  const auto found = g_suite_leases.find({name, version});
  if (found == g_suite_leases.end() || found->second == 0) return 1;
  --found->second;
  ++g_suite_releases;
  return 0;
}

bool verify_suite_release_without_acquire_rejected() {
  const uint32_t acquires_before = g_suite_acquires;
  const uint32_t releases_before = g_suite_releases;
  const uint32_t live_before = live_suite_reference_count();
  return release_suite("AEGP Layer Mask Suite", 999) != 0 &&
      g_suite_acquires == acquires_before && g_suite_releases == releases_before &&
      live_suite_reference_count() == live_before;
}

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
  } else if (record.type == 5) {
    record.has_color = true;
    std::memcpy(record.current_color.data(), bytes.data() + u, record.current_color.size());
    std::memcpy(record.default_color.data(), bytes.data() + u + 4, record.default_color.size());
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

#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
enum class RequestedKind { Integer, Float, Color };
struct RequestedAssignment {
  std::wstring id;
  int32_t index{};
  RequestedKind kind{};
  double value{};
  std::array<unsigned char, 4> color{};
};
using RequestedAssignments = std::vector<RequestedAssignment>;

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
  const std::wstring encoded(text);
  if (encoded.size() < 3 || encoded.size() > 8192 || encoded.compare(0, 3, L"v2|") != 0)
    return false;
  std::vector<HostMask> masks;
  std::size_t total_vertices = 0;
  const std::wstring payload = encoded.substr(3);
  if (payload.empty()) {
    g_mask_scene.clear();
    g_mask_lifetime = {};
    g_mask_scene_id = "request_v4";
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
  g_mask_lifetime = {};
  g_mask_scene_id = "request_v4";
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
  const bool version3 = encoded.compare(0, 3, L"v3|") == 0;
  if (encoded.size() < 4 || encoded.size() > 4096 ||
      (!version3 && encoded.compare(0, 3, L"v2|") != 0)) return false;
  const std::wstring payload = encoded.substr(3);
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
    if (!valid_parameter_id(id) || value.size() > 64 ||
        !parse_i32_arg(index_text.c_str(), 1, static_cast<int32_t>(kMaxParams), index) ||
        !seen_ids.insert(id).second || !seen_indices.insert(index).second) return false;
    RequestedKind kind{};
    if (kind_text == L"i32") kind = RequestedKind::Integer;
    else if (kind_text == L"f64") kind = RequestedKind::Float;
    else if (version3 && kind_text == L"argb8") kind = RequestedKind::Color;
    else return false;
    double parsed{};
    std::array<unsigned char, 4> color{};
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
    output.push_back({id, index, kind, parsed, color});
    if (output.size() > kMaxParams) return false;
    if (separator == std::wstring::npos) break;
    offset = separator + 1;
    if (offset == payload.size()) return false;
  }
  return !output.empty();
}

bool validate_requested_assignments(const RequestedAssignments& requested) {
  for (const auto& assignment : requested) {
    if (assignment.index < 1 || static_cast<std::size_t>(assignment.index) > g_params.size()) return false;
    const auto& descriptor = g_params[static_cast<std::size_t>(assignment.index - 1)];
    const bool integer_compatible = descriptor.type == 1 || descriptor.type == 4 || descriptor.type == 7;
    const bool float_compatible = descriptor.type == 10;
    const bool color_compatible = descriptor.type == 5;
    if ((assignment.kind == RequestedKind::Integer && !integer_compatible) ||
        (assignment.kind == RequestedKind::Float && !float_compatible) ||
        (assignment.kind == RequestedKind::Color && !color_compatible) ||
        (assignment.kind != RequestedKind::Color && (!descriptor.has_numeric ||
         assignment.value < descriptor.valid_min || assignment.value > descriptor.valid_max))) return false;
  }
  return true;
}

void initialize_parameter_definitions(
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    definitions[i + 1] = g_params[i].raw;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(definitions[i + 1], 56, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 10)
      write<double>(definitions[i + 1], 56, g_params[i].default_value);
  }
}

bool apply_requested_assignments(
    std::vector<std::array<std::byte, kParamSize>>& definitions,
    const RequestedAssignments& requested) {
  if (!validate_requested_assignments(requested)) return false;
  for (const auto& assignment : requested) {
    const auto slot = static_cast<std::size_t>(assignment.index);
    const auto type = g_params[slot - 1].type;
    if (type == 1 || type == 4 || type == 7)
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(assignment.value));
    else if (type == 10)
      write<double>(definitions[slot], 56, assignment.value);
    else if (type == 5)
      std::memcpy(definitions[slot].data() + 56, assignment.color.data(), assignment.color.size());
    else
      return false;
  }
  return true;
}

double requested_value(const RequestedAssignments& requested, const wchar_t* id) {
  const auto found = std::find_if(requested.begin(), requested.end(), [id](const auto& assignment) {
    return assignment.id == id;
  });
  return found == requested.end() || found->kind == RequestedKind::Color ? 0.0 : found->value;
}

std::string requested_parameters_json(const RequestedAssignments& requested) {
  std::ostringstream output;
  output << "[";
  for (std::size_t i = 0; i < requested.size(); ++i) {
    if (i != 0) output << ",";
    const auto& assignment = requested[i];
    std::string id;
    id.reserve(assignment.id.size());
    for (const wchar_t character : assignment.id) id.push_back(static_cast<char>(character));
    output << "{\"id\":\"" << id << "\",\"slot\":" << assignment.index
           << ",\"kind\":\""
           << (assignment.kind == RequestedKind::Integer ? "integer" :
               assignment.kind == RequestedKind::Float ? "float" : "color")
           << "\",\"value\":";
    if (assignment.kind == RequestedKind::Integer)
      output << static_cast<int32_t>(assignment.value);
    else if (assignment.kind == RequestedKind::Float)
      output << std::setprecision(17) << assignment.value;
    else
      output << "{\"alpha\":" << static_cast<unsigned>(assignment.color[0])
             << ",\"red\":" << static_cast<unsigned>(assignment.color[1])
             << ",\"green\":" << static_cast<unsigned>(assignment.color[2])
             << ",\"blue\":" << static_cast<unsigned>(assignment.color[3]) << "}";
    output << "}";
  }
  output << "]";
  return output.str();
}
#endif

#ifdef AEXCOMPAT_RENDER_WORKER
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& command_output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes,
                    std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr) {
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
  else if (case_id != "default" && case_id != "connected_map" && case_id != "request" && !partial_extent_hint) return -2;
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

  std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (requested) {
    if (!apply_requested_assignments(definitions, *requested)) return -3;
  } else {
    if (definitions.size() <= 7) return -3;
    write<int32_t>(definitions[1], 56, amount);
    write<int32_t>(definitions[2], 56, direction);
    write<int32_t>(definitions[3], 56, seed);
    write<int32_t>(definitions[4], 56, repeat);
    write<double>(definitions[5], 56, mix);
    if (case_id == "inverted_map") write<int32_t>(definitions[7], 56, 1);
  }
  std::vector<void*> params(definitions.size());
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
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr) {
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
  else if (case_id != "default" && case_id != "request" && !deep16 && !float32 && !missing_input && !crash_null_output && !temporal_context && !partial_output_request && !connected_map) return result;
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

  std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (requested) {
    if (!apply_requested_assignments(definitions, *requested)) return result;
  } else {
    if (definitions.size() <= 7) return result;
    write<int32_t>(definitions[1], 56, amount); write<int32_t>(definitions[2], 56, direction);
    write<int32_t>(definitions[3], 56, seed); write<int32_t>(definitions[4], 56, repeat);
    write<double>(definitions[5], 56, mix);
    if (case_id == "inverted_map") write<int32_t>(definitions[7], 56, 1);
  }
  std::vector<void*> params(definitions.size());
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
  // PF_PreRenderOutput::pre_render_data -> PF_SmartRenderInput::pre_render_data.
  write<void*>(smart_input, 48, read<void*>(pre_output, 40));
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
  if (auto delete_pre_render_data = read<void(__cdecl*)(void*)>(pre_output, 48))
    delete_pre_render_data(read<void*>(pre_output, 40));
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
    if (param.has_color) {
      const auto color_json = [](const auto& color) {
        std::ostringstream value;
        value << "{\"alpha\":" << static_cast<unsigned>(color[0])
              << ",\"red\":" << static_cast<unsigned>(color[1])
              << ",\"green\":" << static_cast<unsigned>(color[2])
              << ",\"blue\":" << static_cast<unsigned>(color[3]) << '}';
        return value.str();
      };
      std::cout << ",\"default_color\":" << color_json(param.default_color)
                << ",\"current_color\":" << color_json(param.current_color);
    }
    if (!param.choices.empty()) std::cout << ",\"choices\":\"" << escape(param.choices) << "\"";
    if (!param.label.empty()) std::cout << ",\"label\":\"" << escape(param.label) << "\"";
    std::cout << '}';
  }
  std::cout << "],\"selectors_executed\":true,\"render_performed\":false}\n";
}
}  // namespace

int wmain(int argc, wchar_t** argv) {
  SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
#ifdef AEXCOMPAT_RENDER_WORKER
  const bool request_mode = argc == 5 && std::wstring(argv[1]) == L"--render-request";
  if (!request_mode && (argc != 5 || std::wstring(argv[1]) != L"--render")) return 2;
  RequestedAssignments requested_parameters;
  if (request_mode && !parse_parameter_payload(argv[4], requested_parameters)) return 3;
#elif defined(AEXCOMPAT_SMART_WORKER)
  const bool mask_request_mode = argc == 5 && std::wstring(argv[1]) == L"--smart-mask-request";
  const bool mask_scene_request_mode = argc == 6 &&
      std::wstring(argv[1]) == L"--smart-mask-scene-request";
  const bool mask_context_request_mode = argc == 6 &&
      std::wstring(argv[1]) == L"--smart-mask-context-request";
  const bool mask_count_error_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-mask-count-error-request";
  const bool mask_count_crash_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-mask-count-crash-request";
  const bool mask_double_dispose_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-mask-double-dispose-request";
  const bool stream_live_value_dispose_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-stream-live-value-dispose-request";
  const bool suite_release_without_acquire_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-suite-release-without-acquire-request";
  const bool handle_resize_while_locked_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-handle-resize-while-locked-request";
  const bool request_mode = mask_request_mode || mask_scene_request_mode || mask_context_request_mode ||
      mask_count_error_mode || mask_count_crash_mode || mask_double_dispose_mode ||
      stream_live_value_dispose_mode || suite_release_without_acquire_mode ||
      handle_resize_while_locked_mode ||
      (argc == 5 && std::wstring(argv[1]) == L"--smart-request");
  if (!request_mode && (argc != 5 || std::wstring(argv[1]) != L"--smart")) return 2;
  g_mask_model_enabled = mask_request_mode || mask_scene_request_mode || mask_context_request_mode ||
      mask_count_error_mode || mask_count_crash_mode || mask_double_dispose_mode ||
      stream_live_value_dispose_mode || suite_release_without_acquire_mode ||
      handle_resize_while_locked_mode;
  g_mask_fault = mask_count_error_mode ? MaskFault::CountError :
      mask_count_crash_mode ? MaskFault::CountCrash : MaskFault::None;
  RequestedAssignments requested_parameters;
  if (request_mode && !parse_parameter_payload(argv[4], requested_parameters)) return 3;
  if (g_mask_model_enabled) {
    std::string scene_id = "rectangle";
    if (mask_context_request_mode) {
      if (!parse_mask_context_payload(argv[5])) return 3;
    } else if (mask_scene_request_mode) {
      scene_id.clear();
      for (const wchar_t* p = argv[5]; *p; ++p) {
        if (*p > 0x7f) return 3;
        scene_id.push_back(static_cast<char>(*p));
      }
    }
    if (!mask_context_request_mode && !configure_mask_scene(scene_id)) return 3;
  }
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
  write<void*>(input, kInEffectRef, g_mask_model_enabled ? &g_effect : nullptr);
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
#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
  if (request_mode && (params_error != 0 || !validate_requested_assignments(requested_parameters))) {
    if (global_error == 0)
      entry(kGlobalSetdown, input.data(), output.data(), nullptr, nullptr, nullptr);
    FreeLibrary(module);
    return 3;
  }
#endif
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
  std::string case_id = request_mode ? "request" : "";
  if (!request_mode) {
    for (const wchar_t* p = argv[4]; *p; ++p) {
      if (*p > 0x7f) return 2;
      case_id.push_back(static_cast<char>(*p));
    }
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
          thread_hashes[index], thread_guards[index], nullptr);
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
                               render_rowbytes, input_hash, output_hash, guards_intact,
                               request_mode ? &requested_parameters : nullptr);
  }
  std::cerr << "stage:render_end error=" << render_error << "\n" << std::flush;
#elif defined(AEXCOMPAT_SMART_WORKER)
  std::string case_id = request_mode ? "request" : "";
  if (!request_mode)
    for (const wchar_t* p = argv[4]; *p; ++p) { if (*p > 0x7f) return 2; case_id.push_back(static_cast<char>(*p)); }
  std::cerr << "stage:smart_render_begin\n" << std::flush;
  const SmartResult smart = params_error == 0
      ? smart_render_once(entry, input, output, case_id,
                          request_mode ? &requested_parameters : nullptr)
      : SmartResult{};
  const bool lifetime_fault_observed = mask_double_dispose_mode
      ? verify_mask_double_dispose_rejected()
      : stream_live_value_dispose_mode
          ? verify_stream_dispose_with_live_value_rejected()
          : false;
  bool suite_fault_observed = false;
  bool handle_fault_observed = false;
  std::cerr << "stage:smart_render_end pre_error=" << smart.pre_error
            << " render_error=" << smart.render_error << "\n" << std::flush;
#endif
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = global_error == 0
      ? entry(kGlobalSetdown, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
  #ifdef AEXCOMPAT_SMART_WORKER
  if (suite_release_without_acquire_mode)
    suite_fault_observed = verify_suite_release_without_acquire_rejected();
  if (handle_resize_while_locked_mode)
    handle_fault_observed = verify_handle_resize_while_locked_rejected();
  #endif
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
            << ",\"request_mode\":" << (request_mode ? "true" : "false")
            << ",\"requested_parameters\":" << requested_parameters_json(requested_parameters)
            << ",\"requested_amount\":" << static_cast<int32_t>(requested_value(requested_parameters, L"amount"))
            << ",\"requested_direction\":" << static_cast<int32_t>(requested_value(requested_parameters, L"direction"))
            << ",\"requested_seed\":" << static_cast<int32_t>(requested_value(requested_parameters, L"seed"))
            << ",\"requested_mix\":" << std::setprecision(17) << requested_value(requested_parameters, L"mix")
            << ",\"requested_invert_map\":" << static_cast<int32_t>(requested_value(requested_parameters, L"invert_map"))
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
            << ",\"request_mode\":" << (request_mode ? "true" : "false")
            << ",\"mask_scene_id\":\"" << g_mask_scene_id << "\""
            << ",\"mask_count\":" << g_mask_scene.size()
            << ",\"mask_open_count\":" << mask_open_count()
            << ",\"mask_tangent_vertex_count\":" << mask_tangent_vertex_count()
            << ",\"mask_lifetimes_balanced\":" << (mask_lifetimes_balanced() ? "true" : "false")
            << ",\"mask_handles_acquired\":" << g_mask_lifetime.masks_acquired
            << ",\"mask_handles_disposed\":" << g_mask_lifetime.masks_disposed
            << ",\"stream_handles_acquired\":" << g_mask_lifetime.streams_acquired
            << ",\"stream_handles_disposed\":" << g_mask_lifetime.streams_disposed
            << ",\"stream_values_acquired\":" << g_mask_lifetime.values_acquired
            << ",\"stream_values_disposed\":" << g_mask_lifetime.values_disposed
            << ",\"lifetime_fault_observed\":" << (lifetime_fault_observed ? "true" : "false")
            << ",\"suite_leases_balanced\":" << (suite_leases_balanced() ? "true" : "false")
            << ",\"suite_acquires\":" << g_suite_acquires
            << ",\"suite_releases\":" << g_suite_releases
            << ",\"live_suite_lease_count\":" << live_suite_lease_count()
            << ",\"live_suite_reference_count\":" << live_suite_reference_count()
            << ",\"live_suite_leases\":\"" << live_suite_lease_summary() << "\""
            << ",\"suite_fault_observed\":" << (suite_fault_observed ? "true" : "false")
            << ",\"handle_lifetimes_balanced\":" << (handle_lifetimes_balanced() ? "true" : "false")
            << ",\"handles_created\":" << g_handles_created
            << ",\"handles_disposed\":" << g_handles_disposed
            << ",\"handle_locks\":" << g_handle_locks
            << ",\"handle_unlocks\":" << g_handle_unlocks
            << ",\"live_handle_count\":" << g_handles.size()
            << ",\"live_handle_bytes\":" << g_handle_bytes
            << ",\"invalid_handle_operations\":" << g_invalid_handle_operations
            << ",\"handle_fault_observed\":" << (handle_fault_observed ? "true" : "false")
            << ",\"requested_parameters\":" << requested_parameters_json(requested_parameters)
            << ",\"requested_amount\":" << static_cast<int32_t>(requested_value(requested_parameters, L"amount"))
            << ",\"requested_direction\":" << static_cast<int32_t>(requested_value(requested_parameters, L"direction"))
            << ",\"requested_seed\":" << static_cast<int32_t>(requested_value(requested_parameters, L"seed"))
            << ",\"requested_mix\":" << std::setprecision(17) << requested_value(requested_parameters, L"mix")
            << ",\"requested_invert_map\":" << static_cast<int32_t>(requested_value(requested_parameters, L"invert_map"))
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
