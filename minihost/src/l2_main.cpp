#include <windows.h>
#include <bcrypt.h>

#include <array>
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cwchar>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <list>
#include <map>
#include <memory>
#include <mutex>
#include <new>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
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
struct MaskFeather {
  int32_t segment{};
  double segment_s{};
  double radius{};
  float ui_corner_angle{};
  float tension{};
  uint8_t interp{};
  uint8_t type{};
};
static_assert(sizeof(MaskFeather) == 40);
static_assert(offsetof(MaskFeather, segment_s) == 8);
static_assert(offsetof(MaskFeather, radius) == 16);
static_assert(offsetof(MaskFeather, interp) == 32);
static_assert(offsetof(MaskFeather, type) == 33);
struct OutlineData {
  OpaqueHostObject outline{0x4f55544c};
  bool open{};
  std::vector<MaskVertex> vertices;
  std::vector<MaskFeather> feathers;
};
struct HostTime { int32_t value{}; uint32_t scale{1}; };
static_assert(sizeof(HostTime) == 8);
struct HostKeyframe : OutlineData {
  HostTime time{};
  int32_t flags{};
  int32_t in_interpolation{1};
  int32_t out_interpolation{1};
  int32_t label{};
};
struct HostMask : OutlineData {
  OpaqueHostObject mask{0x4d41534b};
  bool mask_live{};
  bool stream_live{};
  bool value_live{};
  bool deleted{};
  bool invert{};
  bool locked{};
  bool roto_bezier{};
  uint8_t motion_blur{};
  uint8_t feather_falloff{};
  int32_t mode{1};
  int32_t id{};
  int32_t outline_stream_id{};
  int32_t feather_stream_id{};
  int32_t opacity_stream_id{};
  int32_t expansion_stream_id{};
  int32_t dynamic_order{};
  double opacity{100.0};
  std::array<double, 2> feather{};
  double expansion{};
  std::u16string dynamic_name{u"Mask"};
  std::array<uint32_t, 5> dynamic_flags{};
  bool dynamic_modified{};
  std::array<std::u16string, 4> expressions;
  std::array<bool, 4> expression_enabled{};
  std::array<double, 4> color{1.0, 1.0, 0.0, 0.0};
  std::list<HostKeyframe> keyframes;
};
enum class DynamicNodeKind {
  MaskOutline, LayerRoot, MaskParade, MaskAtom, MaskFeather, MaskOpacity, MaskExpansion
};
struct HostStreamRef {
  OpaqueHostObject opaque{0x5354524d};
  HostMask* mask{};
  int32_t selector{};
  int32_t unique_id{};
  uint32_t live_values{};
  DynamicNodeKind kind{DynamicNodeKind::MaskOutline};
};
struct StreamValue {
  void* stream;
  union {
    void* value;
    double one_d;
    double two_d[2];
    std::byte raw_value[32];
  };
};
static_assert(sizeof(StreamValue) == 40);
struct CheckedStreamValue {
  HostStreamRef* stream{};
  OutlineData* outline{};
  HostKeyframe* source_keyframe{};
  std::unique_ptr<OutlineData> owned_outline;
};
OutlineData* sampled_outline(HostStreamRef* stream, const HostTime* time,
                             std::unique_ptr<OutlineData>& owned);
bool dynamic_leaf(DynamicNodeKind kind);
int32_t __cdecl get_mask_outline_vertex_info(void* outline, int32_t index, MaskVertex* vertex);
int32_t __cdecl set_mask_outline_vertex_info(void* outline, int32_t index,
                                              const MaskVertex* vertex);
OpaqueHostObject g_effect{0x45464658};
OpaqueHostObject g_layer{0x4c415952};
std::vector<HostMask> g_mask_scene;
std::list<HostStreamRef> g_stream_refs;
std::unordered_map<StreamValue*, CheckedStreamValue> g_stream_values;
struct MaskLifetimeCounts {
  uint32_t masks_acquired{};
  uint32_t masks_disposed{};
  uint32_t streams_acquired{};
  uint32_t streams_disposed{};
  uint32_t values_acquired{};
  uint32_t values_disposed{};
};
MaskLifetimeCounts g_mask_lifetime;
uint32_t g_invalid_outline_operations{};
uint32_t g_outline_mutations{};
uint32_t g_mask_mutations{};
uint32_t g_invalid_mask_operations{};
uint32_t g_invalid_stream_operations{};
uint32_t g_stream_metadata_queries{};
uint32_t g_stream_duplicates{};
uint32_t g_keyframe_mutations{};
uint32_t g_invalid_keyframe_operations{};
uint32_t g_dynamic_stream_queries{};
uint32_t g_dynamic_stream_mutations{};
uint32_t g_invalid_dynamic_stream_operations{};
uint32_t g_layer_dynamic_flags{};
uint32_t g_mask_parade_dynamic_flags{};
int32_t g_next_mask_id{1};
int32_t g_next_stream_id{1};
constexpr std::size_t kMaxHostMasks = 8;
constexpr std::size_t kMaxOutlineVertices = 64;
constexpr std::size_t kMaxOutlineFeathers = 64;
constexpr std::size_t kMaxKeyframesPerStream = 64;
constexpr std::size_t kMaxCheckedStreamValues = 256;

struct AddKeyframesTransaction {
  OpaqueHostObject opaque{0x41444b46};
  HostStreamRef* stream{};
  std::vector<HostKeyframe> staged;
};
std::list<AddKeyframesTransaction> g_add_keyframe_transactions;

std::size_t distinct_vertex_count(const OutlineData& mask) {
  return mask.vertices.size() - static_cast<std::size_t>(!mask.open && !mask.vertices.empty());
}

void sync_closed_vertex(OutlineData& mask) {
  if (!mask.open && !mask.vertices.empty()) mask.vertices.back() = mask.vertices.front();
}

bool mask_lifetimes_balanced() {
  return g_mask_lifetime.masks_acquired == g_mask_lifetime.masks_disposed &&
      g_mask_lifetime.streams_acquired == g_mask_lifetime.streams_disposed &&
      g_mask_lifetime.values_acquired == g_mask_lifetime.values_disposed &&
      g_stream_refs.empty() && g_stream_values.empty() && g_add_keyframe_transactions.empty() &&
      std::none_of(g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) {
        return mask.mask_live || mask.stream_live || mask.value_live;
      });
}

bool configure_mask_scene(const std::string& scene_id) {
  if (!g_stream_refs.empty() || !g_stream_values.empty() ||
      !g_add_keyframe_transactions.empty()) return false;
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  g_mask_lifetime = {};
  g_mask_scene_id = scene_id;
  const auto rectangle = [](double left, double top, double right, double bottom) {
    HostMask mask;
    mask.id = g_next_mask_id++;
    mask.outline_stream_id = g_next_stream_id++;
    mask.feather_stream_id = g_next_stream_id++;
    mask.opacity_stream_id = g_next_stream_id++;
    mask.expansion_stream_id = g_next_stream_id++;
    mask.vertices = {{left, top, 0, 0, 0, 0}, {right, top, 0, 0, 0, 0},
                     {right, bottom, 0, 0, 0, 0}, {left, bottom, 0, 0, 0, 0},
                     {left, top, 0, 0, 0, 0}};
    return mask;
  };
  if (scene_id == "rectangle") g_mask_scene.push_back(rectangle(4, 3, 12, 9));
  else if (scene_id == "translated_rectangle") g_mask_scene.push_back(rectangle(2, 2, 10, 8));
  else if (scene_id == "two_rectangles") {
    g_mask_scene.push_back(rectangle(1, 1, 7, 6));
    g_mask_scene.push_back(rectangle(9, 5, 15, 11));
  } else if (scene_id != "empty") return false;
  int32_t order = 0;
  for (auto& mask : g_mask_scene) mask.dynamic_order = order++;
  return true;
}

std::vector<HostMask*> ordered_active_masks() {
  std::vector<HostMask*> masks;
  for (auto& mask : g_mask_scene) if (!mask.deleted) masks.push_back(&mask);
  std::sort(masks.begin(), masks.end(), [](const HostMask* left, const HostMask* right) {
    return left->dynamic_order < right->dynamic_order;
  });
  return masks;
}

HostMask* find_mask(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.mask; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}
HostStreamRef* find_stream(void* handle) {
  const auto found = std::find_if(g_stream_refs.begin(), g_stream_refs.end(),
      [handle](auto& stream) { return handle == &stream.opaque; });
  return found == g_stream_refs.end() ? nullptr : &*found;
}
OutlineData* find_outline(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.outline; });
  if (found != g_mask_scene.end()) return &*found;
  for (auto& mask : g_mask_scene) {
    const auto key = std::find_if(mask.keyframes.begin(), mask.keyframes.end(),
        [handle](auto& item) { return handle == &item.outline; });
    if (key != mask.keyframes.end()) return &*key;
  }
  for (auto& item : g_stream_values) {
    if (item.second.outline && handle == &item.second.outline->outline)
      return item.second.outline;
  }
  return nullptr;
}

std::size_t mask_open_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted && mask.open; }));
}

std::size_t active_mask_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted; }));
}

std::size_t mask_tangent_vertex_count() {
  std::size_t count = 0;
  for (const auto& mask : g_mask_scene) {
    if (mask.deleted) continue;
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

struct AegpMemoryRecord {
  std::vector<std::byte> bytes;
  int32_t plugin_id{};
  uint32_t lock_count{};
};
std::unordered_map<void*, std::unique_ptr<AegpMemoryRecord>> g_aegp_memory;
std::mutex g_aegp_memory_mutex;
uint64_t g_aegp_memory_bytes{};
uint32_t g_aegp_memory_created{};
uint32_t g_aegp_memory_freed{};
uint32_t g_invalid_aegp_memory_operations{};
bool g_aegp_memory_reporting{};
constexpr std::size_t kMaxAegpMemoryHandles = 256;
constexpr uint64_t kMaxAegpMemoryBytes = 16 * 1024 * 1024;

int32_t __cdecl new_aegp_mem_handle(int32_t plugin_id, const char* what, uint32_t size,
                                    int32_t flags, void** handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  if (plugin_id != 1 || !what || std::strlen(what) > 127 || !handle || (flags & ~3) != 0 ||
      g_aegp_memory.size() >= kMaxAegpMemoryHandles ||
      size > kMaxAegpMemoryBytes || g_aegp_memory_bytes > kMaxAegpMemoryBytes - size) {
    if (handle) *handle = nullptr; ++g_invalid_aegp_memory_operations; return 4;
  }
  auto record = std::make_unique<AegpMemoryRecord>();
  record->plugin_id = plugin_id; record->bytes.resize(size);
  if ((flags & 1) == 0 && size) std::memset(record->bytes.data(), 0xcd, size);
  void* key = record.get(); g_aegp_memory.emplace(key, std::move(record));
  g_aegp_memory_bytes += size; ++g_aegp_memory_created; *handle = key; return 0;
}
int32_t __cdecl free_aegp_mem_handle(void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || found->second->lock_count != 0) {
    ++g_invalid_aegp_memory_operations; return 4;
  }
  g_aegp_memory_bytes -= found->second->bytes.size(); g_aegp_memory.erase(found);
  ++g_aegp_memory_freed; return 0;
}
int32_t __cdecl lock_aegp_mem_handle(void* handle, void** data) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || !data) { ++g_invalid_aegp_memory_operations; return 4; }
  ++found->second->lock_count;
  *data = found->second->bytes.empty() ? nullptr : found->second->bytes.data(); return 0;
}
int32_t __cdecl unlock_aegp_mem_handle(void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || found->second->lock_count == 0) {
    ++g_invalid_aegp_memory_operations; return 4;
  }
  --found->second->lock_count; return 0;
}
int32_t __cdecl get_aegp_mem_handle_size(void* handle, uint32_t* size) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || !size) return 4;
  *size = static_cast<uint32_t>(found->second->bytes.size()); return 0;
}
int32_t __cdecl resize_aegp_mem_handle(const char* what, uint32_t size, void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (!what || std::strlen(what) > 127 || found == g_aegp_memory.end() ||
      found->second->lock_count != 0 || size > kMaxAegpMemoryBytes ||
      g_aegp_memory_bytes - found->second->bytes.size() > kMaxAegpMemoryBytes - size) {
    ++g_invalid_aegp_memory_operations; return 4;
  }
  const std::size_t old_size = found->second->bytes.size(); found->second->bytes.resize(size);
  g_aegp_memory_bytes = g_aegp_memory_bytes - old_size + size; return 0;
}
int32_t __cdecl set_aegp_mem_reporting(uint8_t enabled) {
  g_aegp_memory_reporting = enabled != 0; return 0;
}
int32_t __cdecl get_aegp_mem_stats(int32_t plugin_id, int32_t* count, int32_t* size) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  if (plugin_id != 1 || !count || !size) return 4;
  uint64_t total{}; int32_t handles{};
  for (const auto& item : g_aegp_memory) if (item.second->plugin_id == plugin_id) {
    ++handles; total += item.second->bytes.size();
  }
  if (total > INT32_MAX) return 4;
  *count = handles; *size = static_cast<int32_t>(total); return 0;
}
bool aegp_memory_balanced() {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  return g_aegp_memory.empty() && g_aegp_memory_bytes == 0 &&
      g_aegp_memory_created == g_aegp_memory_freed;
}
int32_t make_utf16_handle(const std::u16string& text, const char* label, void** handle) {
  const uint64_t bytes = (text.size() + 1) * sizeof(char16_t);
  if (bytes > UINT32_MAX || new_aegp_mem_handle(1, label, static_cast<uint32_t>(bytes), 1, handle))
    return 4;
  void* data = nullptr;
  if (lock_aegp_mem_handle(*handle, &data) != 0) { free_aegp_mem_handle(*handle); *handle = nullptr; return 4; }
  std::memcpy(data, text.c_str(), static_cast<std::size_t>(bytes));
  return unlock_aegp_mem_handle(*handle);
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
  *count = static_cast<int32_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted; }));
  return 0;
}

int32_t __cdecl get_layer_mask_by_index(void* layer, int32_t index, void** mask) {
  if (layer != &g_layer || index < 0 || !mask) return 4;
  const auto masks = ordered_active_masks();
  if (static_cast<std::size_t>(index) >= masks.size()) return 4;
  auto& record = *masks[static_cast<std::size_t>(index)];
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

bool usable_mask(const HostMask* mask) { return mask && mask->mask_live && !mask->deleted; }

int32_t __cdecl get_mask_invert(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->invert; return 0;
}
int32_t __cdecl set_mask_invert(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->invert = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_mode(void* handle, int32_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->mode; return 0;
}
int32_t __cdecl set_mask_mode(void* handle, int32_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value < 0 || value > 7) { ++g_invalid_mask_operations; return 4; }
  mask->mode = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_motion_blur(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->motion_blur; return 0;
}
int32_t __cdecl set_mask_motion_blur(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value > 2) { ++g_invalid_mask_operations; return 4; }
  mask->motion_blur = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_feather_falloff(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->feather_falloff; return 0;
}
int32_t __cdecl set_mask_feather_falloff(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || value > 1) { ++g_invalid_mask_operations; return 4; }
  mask->feather_falloff = value; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_id(void* handle, int32_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->id; return 0;
}
int32_t __cdecl create_new_mask(void* layer, void** handle, int32_t* index) {
  if (layer != &g_layer || !handle || g_mask_scene.size() >= kMaxHostMasks) {
    ++g_invalid_mask_operations; return 4;
  }
  HostMask mask; mask.id = g_next_mask_id++; mask.outline_stream_id = g_next_stream_id++;
  mask.feather_stream_id = g_next_stream_id++; mask.opacity_stream_id = g_next_stream_id++;
  mask.expansion_stream_id = g_next_stream_id++;
  mask.dynamic_order = static_cast<int32_t>(active_mask_count());
  mask.mask_live = true;
  g_mask_scene.push_back(mask);
  ++g_mask_lifetime.masks_acquired; ++g_mask_mutations;
  *handle = &g_mask_scene.back().mask;
  if (index) *index = static_cast<int32_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& item) { return !item.deleted; }) - 1);
  return 0;
}
int32_t __cdecl delete_mask_from_layer(void* handle) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || mask->stream_live || mask->value_live) {
    ++g_invalid_mask_operations; return 4;
  }
  const int32_t deleted_order = mask->dynamic_order;
  mask->deleted = true;
  for (auto& candidate : g_mask_scene)
    if (!candidate.deleted && candidate.dynamic_order > deleted_order) --candidate.dynamic_order;
  ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_color(void* handle, double* color) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !color) return 4;
  std::copy(mask->color.begin(), mask->color.end(), color); return 0;
}
int32_t __cdecl set_mask_color(void* handle, const double* color) {
  HostMask* mask = find_mask(handle);
  if (!usable_mask(mask) || !color || !std::all_of(color, color + 4,
      [](double value) { return std::isfinite(value) && value >= 0 && value <= 1; })) {
    ++g_invalid_mask_operations; return 4;
  }
  std::copy(color, color + 4, mask->color.begin()); ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_lock(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->locked; return 0;
}
int32_t __cdecl set_mask_lock(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->locked = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl get_mask_roto_bezier(void* handle, uint8_t* value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask) || !value) return 4;
  *value = mask->roto_bezier; return 0;
}
int32_t __cdecl set_mask_roto_bezier(void* handle, uint8_t value) {
  HostMask* mask = find_mask(handle); if (!usable_mask(mask)) return 4;
  mask->roto_bezier = value != 0; ++g_mask_mutations; return 0;
}
int32_t __cdecl duplicate_mask(void* original_handle, void** duplicate_handle) {
  HostMask* original = find_mask(original_handle);
  if (!usable_mask(original) || !duplicate_handle || g_mask_scene.size() >= kMaxHostMasks) {
    ++g_invalid_mask_operations; return 4;
  }
  HostMask copy = *original;
  copy.mask_live = true; copy.stream_live = false; copy.value_live = false;
  copy.deleted = false; copy.id = g_next_mask_id++;
  copy.outline_stream_id = g_next_stream_id++;
  copy.feather_stream_id = g_next_stream_id++;
  copy.opacity_stream_id = g_next_stream_id++;
  copy.expansion_stream_id = g_next_stream_id++;
  copy.dynamic_order = static_cast<int32_t>(active_mask_count());
  g_mask_scene.push_back(std::move(copy));
  ++g_mask_lifetime.masks_acquired; ++g_mask_mutations;
  *duplicate_handle = &g_mask_scene.back().mask;
  return 0;
}

int32_t stream_identity(const HostMask* mask, DynamicNodeKind kind) {
  if (!mask) return kind == DynamicNodeKind::LayerRoot ? 0x70000001 : 0x70000002;
  switch (kind) {
    case DynamicNodeKind::MaskOutline: return mask->outline_stream_id;
    case DynamicNodeKind::MaskFeather: return mask->feather_stream_id;
    case DynamicNodeKind::MaskOpacity: return mask->opacity_stream_id;
    case DynamicNodeKind::MaskExpansion: return mask->expansion_stream_id;
    case DynamicNodeKind::MaskAtom: return 0x10000000 + mask->id;
    default: return 0;
  }
}
int32_t create_stream_ref(HostMask* mask, DynamicNodeKind kind, int32_t selector, void** stream) {
  if (!stream || g_stream_refs.size() >= 64) return 4;
  g_stream_refs.push_back({{}, mask, selector, stream_identity(mask, kind), 0, kind});
  if (mask) mask->stream_live = true;
  ++g_mask_lifetime.streams_acquired;
  *stream = &g_stream_refs.back().opaque;
  return 0;
}
int32_t __cdecl get_new_mask_stream(int32_t plugin_id, void* mask, int32_t selector, void** stream) {
  HostMask* record = find_mask(mask);
  DynamicNodeKind kind{};
  if (selector == 400) kind = DynamicNodeKind::MaskOutline;
  else if (selector == 401) kind = DynamicNodeKind::MaskOpacity;
  else if (selector == 402) kind = DynamicNodeKind::MaskFeather;
  else if (selector == 403) kind = DynamicNodeKind::MaskExpansion;
  else kind = DynamicNodeKind::LayerRoot;
  if (plugin_id != 1 || !usable_mask(record) || selector < 400 || selector > 403 || !stream) {
    ++g_invalid_stream_operations;
    if (stream) *stream = nullptr;
    return 4;
  }
  const int32_t error = create_stream_ref(record, kind, selector, stream);
  if (error) ++g_invalid_stream_operations;
  return error;
}

int32_t __cdecl dispose_stream(void* stream) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->live_values != 0 ||
      std::any_of(g_add_keyframe_transactions.begin(), g_add_keyframe_transactions.end(),
          [record](const auto& transaction) { return transaction.stream == record; })) {
    ++g_invalid_stream_operations; return 4;
  }
  HostMask* mask = record->mask;
  g_stream_refs.erase(std::find_if(g_stream_refs.begin(), g_stream_refs.end(),
      [record](auto& candidate) { return &candidate == record; }));
  if (mask) mask->stream_live = std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
      [mask](const auto& candidate) { return candidate.mask == mask; });
  ++g_mask_lifetime.streams_disposed;
  return 0;
}

int32_t __cdecl get_new_stream_value(int32_t plugin_id, void* stream, int32_t,
                                     const HostTime* time, int32_t, StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !value ||
      g_stream_values.size() >= kMaxCheckedStreamValues ||
      g_stream_values.find(value) != g_stream_values.end()) {
    ++g_invalid_stream_operations; return 4;
  }
  std::unique_ptr<OutlineData> owned;
  OutlineData* outline = nullptr;
  if (record->kind == DynamicNodeKind::MaskOutline)
    outline = sampled_outline(record, time, owned);
  else if (!dynamic_leaf(record->kind)) { ++g_invalid_stream_operations; return 4; }
  ++record->live_values;
  record->mask->value_live = true;
  g_stream_values.emplace(value, CheckedStreamValue{record, outline, nullptr, std::move(owned)});
  ++g_mask_lifetime.values_acquired;
  value->stream = &record->opaque;
  std::memset(value->raw_value, 0, sizeof(value->raw_value));
  if (outline) value->value = &outline->outline;
  else if (record->kind == DynamicNodeKind::MaskOpacity) value->one_d = record->mask->opacity;
  else if (record->kind == DynamicNodeKind::MaskExpansion) value->one_d = record->mask->expansion;
  else { value->two_d[0] = record->mask->feather[0]; value->two_d[1] = record->mask->feather[1]; }
  return 0;
}

int32_t __cdecl dispose_stream_value(StreamValue* value) {
  if (!value) return 4;
  const auto owned = g_stream_values.find(value);
  HostStreamRef* stream_record = find_stream(value->stream);
  OutlineData* outline_record = owned != g_stream_values.end() && owned->second.outline
      ? find_outline(value->value) : nullptr;
  if (owned == g_stream_values.end() || !stream_record || owned->second.stream != stream_record ||
      (owned->second.outline && owned->second.outline != outline_record) ||
      stream_record->live_values == 0) {
    ++g_invalid_stream_operations; return 4;
  }
  --stream_record->live_values;
  g_stream_values.erase(owned);
  stream_record->mask->value_live = std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
      [mask = stream_record->mask](const auto& candidate) {
        return candidate.mask == mask && candidate.live_values != 0;
      });
  ++g_mask_lifetime.values_disposed;
  value->stream = nullptr;
  value->value = nullptr;
  return 0;
}

int32_t __cdecl is_stream_legal(void* layer, int32_t, uint8_t* legal) {
  if (layer != &g_layer || !legal) return 4;
  *legal = 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl can_vary_over_time(void* stream, uint8_t* can_vary) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !can_vary || !dynamic_leaf(record->kind)) return 4;
  *can_vary = 1;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_valid_interpolations(void* stream, int32_t* interpolations) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !interpolations || !dynamic_leaf(record->kind)) return 4;
  *interpolations = 0xffff;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl unsupported_new_layer_stream(int32_t, void*, int32_t, void** stream) {
  if (stream) *stream = nullptr;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t __cdecl unsupported_effect_stream_count(void*, int32_t* count) {
  if (count) *count = 0;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t __cdecl unsupported_new_effect_stream(int32_t, void*, int32_t, void** stream) {
  if (stream) *stream = nullptr;
  ++g_invalid_stream_operations;
  return 4;
}
std::u16string stream_display_name(const HostStreamRef& stream) {
  switch (stream.kind) {
    case DynamicNodeKind::LayerRoot: return u"Layer";
    case DynamicNodeKind::MaskParade: return u"Masks";
    case DynamicNodeKind::MaskAtom: return stream.mask ? stream.mask->dynamic_name : u"Mask";
    case DynamicNodeKind::MaskOutline: return u"Mask Path";
    case DynamicNodeKind::MaskFeather: return u"Mask Feather";
    case DynamicNodeKind::MaskOpacity: return u"Mask Opacity";
    case DynamicNodeKind::MaskExpansion: return u"Mask Expansion";
  }
  return {};
}
int32_t __cdecl unsupported_stream_name(int32_t plugin_id, void* stream, uint8_t, void** name) {
  if (name) *name = nullptr;
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !name) return 4;
  return make_utf16_handle(stream_display_name(*record), "stream name", name);
}
int32_t __cdecl get_stream_units_text(void* stream, uint8_t, char* units) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !units || !dynamic_leaf(record->kind)) return 4;
  const char* text = record->kind == DynamicNodeKind::MaskOpacity ? "%" :
      record->kind == DynamicNodeKind::MaskOutline ? "" : "pixels";
  strcpy_s(units, 32, text);
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_stream_properties(void* stream, int32_t* flags, double* minimum,
                                      double* maximum) {
  if (!find_stream(stream) || !flags) return 4;
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind)) return 4;
  *flags = record->kind == DynamicNodeKind::MaskOpacity ? 3 : 0;
  if (minimum) *minimum = 0;
  if (maximum) *maximum = record->kind == DynamicNodeKind::MaskOpacity ? 100 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl is_stream_timevarying(void* stream, uint8_t* timevarying) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !timevarying || !dynamic_leaf(record->kind)) return 4;
  *timevarying = record->kind == DynamicNodeKind::MaskOutline &&
      !record->mask->keyframes.empty() ? 1 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl get_stream_type(void* stream, int32_t* type) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !type) return 4;
  *type = record->kind == DynamicNodeKind::MaskOutline ? 11 :
      record->kind == DynamicNodeKind::MaskFeather ? 4 :
      (record->kind == DynamicNodeKind::MaskOpacity ||
       record->kind == DynamicNodeKind::MaskExpansion) ? 5 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl reject_set_stream_value(int32_t plugin_id, void* stream, StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !value || value->stream != &record->opaque ||
      !dynamic_leaf(record->kind) || record->kind == DynamicNodeKind::MaskOutline) {
    ++g_invalid_stream_operations; return 4;
  }
  if (record->kind == DynamicNodeKind::MaskOpacity) {
    if (!std::isfinite(value->one_d) || value->one_d < 0 || value->one_d > 100) {
      ++g_invalid_stream_operations; return 4;
    }
    record->mask->opacity = value->one_d;
  } else if (record->kind == DynamicNodeKind::MaskExpansion) {
    if (!std::isfinite(value->one_d)) { ++g_invalid_stream_operations; return 4; }
    record->mask->expansion = value->one_d;
  } else {
    if (!std::isfinite(value->two_d[0]) || !std::isfinite(value->two_d[1])) {
      ++g_invalid_stream_operations; return 4;
    }
    record->mask->feather = {value->two_d[0], value->two_d[1]};
  }
  record->mask->dynamic_modified = true; ++g_dynamic_stream_mutations; return 0;
}
int32_t __cdecl unsupported_layer_stream_value(void*, int32_t, int32_t, const void*,
                                               uint8_t, void* value, int32_t* type) {
  if (value) std::memset(value, 0, sizeof(void*));
  if (type) *type = 0;
  ++g_invalid_stream_operations;
  return 4;
}
int32_t expression_index(DynamicNodeKind kind) {
  return kind == DynamicNodeKind::MaskOutline ? 0 : kind == DynamicNodeKind::MaskFeather ? 1 :
      kind == DynamicNodeKind::MaskOpacity ? 2 : kind == DynamicNodeKind::MaskExpansion ? 3 : -1;
}
int32_t __cdecl get_expression_state(int32_t plugin_id, void* stream, uint8_t* enabled) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !enabled) return 4;
  *enabled = record->mask->expression_enabled[static_cast<std::size_t>(index)] ? 1 : 0;
  ++g_stream_metadata_queries;
  return 0;
}
int32_t __cdecl reject_expression_state(int32_t plugin_id, void* stream, uint8_t enabled) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 ||
      (enabled && record->mask->expressions[static_cast<std::size_t>(index)].empty())) {
    ++g_invalid_stream_operations; return 4;
  }
  record->mask->expression_enabled[static_cast<std::size_t>(index)] = enabled != 0;
  record->mask->dynamic_modified = true; return 0;
}
int32_t __cdecl unsupported_get_expression(int32_t plugin_id, void* stream, void** expression) {
  if (expression) *expression = nullptr;
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !expression) return 4;
  return make_utf16_handle(record->mask->expressions[static_cast<std::size_t>(index)],
                           "stream expression", expression);
}
int32_t __cdecl unsupported_set_expression(int32_t plugin_id, void* stream, const uint16_t* expression) {
  HostStreamRef* record = find_stream(stream); const int32_t index = record ? expression_index(record->kind) : -1;
  if (plugin_id != 1 || !record || !record->mask || index < 0 || !expression) return 4;
  std::size_t length = 0; while (length <= 4096 && expression[length]) ++length;
  if (length > 4096) { ++g_invalid_stream_operations; return 4; }
  auto& stored = record->mask->expressions[static_cast<std::size_t>(index)];
  stored.assign(reinterpret_cast<const char16_t*>(expression), length);
  record->mask->expression_enabled[static_cast<std::size_t>(index)] = !stored.empty();
  record->mask->dynamic_modified = true; return 0;
}
int32_t __cdecl duplicate_stream_ref(int32_t plugin_id, void* stream, void** duplicate) {
  HostStreamRef* original = find_stream(stream);
  if (plugin_id != 1 || !original || !duplicate || g_stream_refs.size() >= 64) {
    if (duplicate) *duplicate = nullptr;
    ++g_invalid_stream_operations;
    return 4;
  }
  g_stream_refs.push_back({{}, original->mask, original->selector, original->unique_id, 0,
                           original->kind});
  ++g_mask_lifetime.streams_acquired;
  ++g_stream_duplicates;
  *duplicate = &g_stream_refs.back().opaque;
  return 0;
}
int32_t __cdecl get_unique_stream_id(void* stream, int32_t* id) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !id) return 4;
  *id = record->unique_id;
  ++g_stream_metadata_queries;
  return 0;
}

bool valid_time(const HostTime* time) { return time && time->scale != 0; }
bool time_less(const HostTime& left, const HostTime& right) {
  return static_cast<int64_t>(left.value) * right.scale <
      static_cast<int64_t>(right.value) * left.scale;
}
bool time_equal(const HostTime& left, const HostTime& right) {
  return static_cast<int64_t>(left.value) * right.scale ==
      static_cast<int64_t>(right.value) * left.scale;
}
OutlineData* sampled_outline(HostStreamRef* stream, const HostTime* time,
                             std::unique_ptr<OutlineData>& owned) {
  if (!stream) return nullptr;
  auto& keys = stream->mask->keyframes;
  if (keys.empty() || !time) return stream->mask;
  if (!valid_time(time)) return nullptr;
  auto upper = std::find_if(keys.begin(), keys.end(),
      [time](const auto& key) { return time_less(*time, key.time); });
  if (upper == keys.begin()) return &*upper;
  if (upper == keys.end()) return &keys.back();
  auto lower = std::prev(upper);
  if (time_equal(lower->time, *time) || lower->out_interpolation == 3) return &*lower;
  if (lower->open != upper->open || lower->vertices.size() != upper->vertices.size() ||
      lower->feathers.size() != upper->feathers.size()) return &*lower;
  const double lower_seconds = static_cast<double>(lower->time.value) / lower->time.scale;
  const double upper_seconds = static_cast<double>(upper->time.value) / upper->time.scale;
  const double requested_seconds = static_cast<double>(time->value) / time->scale;
  if (!(upper_seconds > lower_seconds)) return &*lower;
  const double amount = (requested_seconds - lower_seconds) / (upper_seconds - lower_seconds);
  owned = std::make_unique<OutlineData>(static_cast<const OutlineData&>(*lower));
  const auto blend = [amount](double left, double right) { return left + (right - left) * amount; };
  for (std::size_t index = 0; index < owned->vertices.size(); ++index) {
    const MaskVertex& left = lower->vertices[index]; const MaskVertex& right = upper->vertices[index];
    owned->vertices[index] = {blend(left.x, right.x), blend(left.y, right.y),
        blend(left.tangent_in_x, right.tangent_in_x),
        blend(left.tangent_in_y, right.tangent_in_y),
        blend(left.tangent_out_x, right.tangent_out_x),
        blend(left.tangent_out_y, right.tangent_out_y)};
  }
  for (std::size_t index = 0; index < owned->feathers.size(); ++index) {
    const MaskFeather& left = lower->feathers[index]; const MaskFeather& right = upper->feathers[index];
    if (left.segment != right.segment || left.interp != right.interp || left.type != right.type)
      return &*lower;
    owned->feathers[index].segment_s = blend(left.segment_s, right.segment_s);
    owned->feathers[index].radius = blend(left.radius, right.radius);
    owned->feathers[index].ui_corner_angle = static_cast<float>(
        blend(left.ui_corner_angle, right.ui_corner_angle));
    owned->feathers[index].tension = static_cast<float>(blend(left.tension, right.tension));
  }
  return owned.get();
}
HostKeyframe* keyframe_at(HostStreamRef* stream, int32_t index) {
  if (!stream || stream->kind != DynamicNodeKind::MaskOutline || index < 0 ||
      static_cast<std::size_t>(index) >= stream->mask->keyframes.size())
    return nullptr;
  auto item = stream->mask->keyframes.begin();
  std::advance(item, index);
  return &*item;
}
AddKeyframesTransaction* find_add_transaction(void* handle) {
  const auto found = std::find_if(g_add_keyframe_transactions.begin(),
      g_add_keyframe_transactions.end(), [handle](auto& item) { return handle == &item.opaque; });
  return found == g_add_keyframe_transactions.end() ? nullptr : &*found;
}
HostKeyframe snapshot_keyframe(const HostMask& mask, const HostTime& time) {
  HostKeyframe key;
  static_cast<OutlineData&>(key) = static_cast<const OutlineData&>(mask);
  key.time = time;
  return key;
}
int32_t __cdecl get_stream_num_keyframes(void* stream, int32_t* count) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !count) return 4;
  *count = record->kind == DynamicNodeKind::MaskOutline
      ? static_cast<int32_t>(record->mask->keyframes.size()) : 0;
  return 0;
}
int32_t __cdecl get_keyframe_time(void* stream, int32_t index, int16_t time_mode,
                                  HostTime* time) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || !time || time_mode < 0 || time_mode > 1) return 4;
  *time = key->time;
  return 0;
}
int32_t __cdecl insert_keyframe(void* stream, int16_t time_mode, const HostTime* time,
                                int32_t* index) {
  HostStreamRef* record = find_stream(stream);
  if (!record || time_mode < 0 || time_mode > 1 || !valid_time(time) || !index ||
      record->mask->keyframes.size() >= kMaxKeyframesPerStream) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto position = record->mask->keyframes.begin();
  int32_t found_index = 0;
  while (position != record->mask->keyframes.end() && time_less(position->time, *time)) {
    ++position; ++found_index;
  }
  if (position != record->mask->keyframes.end() && time_equal(position->time, *time)) {
    *index = found_index; return 0;
  }
  record->mask->keyframes.insert(position, snapshot_keyframe(*record->mask, *time));
  *index = found_index;
  ++g_keyframe_mutations;
  return 0;
}
int32_t __cdecl delete_keyframe(void* stream, int32_t index) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (!key || std::any_of(g_stream_values.begin(), g_stream_values.end(),
      [key](const auto& item) { return item.second.source_keyframe == key; })) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto position = record->mask->keyframes.begin(); std::advance(position, index);
  record->mask->keyframes.erase(position);
  ++g_keyframe_mutations;
  return 0;
}
int32_t __cdecl get_new_keyframe_value(int32_t plugin_id, void* stream, int32_t index,
                                       StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  if (plugin_id != 1 || !record || !key || !value ||
      g_stream_values.size() >= kMaxCheckedStreamValues ||
      g_stream_values.find(value) != g_stream_values.end()) {
    ++g_invalid_keyframe_operations; return 4;
  }
  auto owned = std::make_unique<OutlineData>(static_cast<const OutlineData&>(*key));
  OutlineData* outline = owned.get();
  ++record->live_values; record->mask->value_live = true;
  g_stream_values.emplace(value, CheckedStreamValue{record, outline, key, std::move(owned)});
  ++g_mask_lifetime.values_acquired;
  value->stream = &record->opaque; value->value = &outline->outline;
  return 0;
}
int32_t __cdecl set_keyframe_value(void* stream, int32_t index, const StreamValue* value) {
  HostStreamRef* record = find_stream(stream);
  HostKeyframe* key = keyframe_at(record, index);
  OutlineData* source = value ? find_outline(value->value) : nullptr;
  if (!key || !source || source == key) { ++g_invalid_keyframe_operations; return 4; }
  static_cast<OutlineData&>(*key) = *source;
  ++g_keyframe_mutations;
  return 0;
}
int32_t __cdecl get_stream_value_dimensionality(void* stream, int16_t* dimensions) {
  if (!find_stream(stream) || !dimensions) return 4;
  *dimensions = 0; return 0;
}
int32_t __cdecl get_stream_temporal_dimensionality(void* stream, int16_t* dimensions) {
  if (!find_stream(stream) || !dimensions) return 4;
  *dimensions = 0; return 0;
}
int32_t __cdecl reject_spatial_tangents(int32_t, void* stream, int32_t,
                                        StreamValue* in_value, StreamValue* out_value) {
  if (in_value) *in_value = {}; if (out_value) *out_value = {};
  if (!find_stream(stream)) return 4;
  ++g_invalid_keyframe_operations; return 4;
}
int32_t __cdecl reject_set_spatial_tangents(void* stream, int32_t,
                                            const StreamValue*, const StreamValue*) {
  if (!find_stream(stream)) return 4;
  ++g_invalid_keyframe_operations; return 4;
}
struct KeyframeEase { double speed; double influence; };
int32_t __cdecl reject_get_temporal_ease(void* stream, int32_t, int32_t,
                                         KeyframeEase* in_ease, KeyframeEase* out_ease) {
  if (in_ease) *in_ease = {}; if (out_ease) *out_ease = {};
  if (!find_stream(stream)) return 4;
  ++g_invalid_keyframe_operations; return 4;
}
int32_t __cdecl reject_set_temporal_ease(void* stream, int32_t, int32_t,
                                         const KeyframeEase*, const KeyframeEase*) {
  if (!find_stream(stream)) return 4;
  ++g_invalid_keyframe_operations; return 4;
}
int32_t __cdecl get_keyframe_flags(void* stream, int32_t index, int32_t* flags) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || !flags) return 4;
  *flags = key->flags; return 0;
}
int32_t __cdecl set_keyframe_flag(void* stream, int32_t index, int32_t flag, uint8_t enabled) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || flag == 0 || (flag & ~0x1f) != 0 || (flag & (flag - 1)) != 0) {
    ++g_invalid_keyframe_operations; return 4;
  }
  if (enabled) key->flags |= flag; else key->flags &= ~flag;
  ++g_keyframe_mutations; return 0;
}
int32_t __cdecl get_keyframe_interpolation(void* stream, int32_t index,
                                           int32_t* in_interp, int32_t* out_interp) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || (!in_interp && !out_interp)) return 4;
  if (in_interp) *in_interp = key->in_interpolation;
  if (out_interp) *out_interp = key->out_interpolation;
  return 0;
}
int32_t __cdecl set_keyframe_interpolation(void* stream, int32_t index,
                                           int32_t in_interp, int32_t out_interp) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || in_interp < 0 || in_interp > 3 || out_interp < 0 || out_interp > 3) {
    ++g_invalid_keyframe_operations; return 4;
  }
  key->in_interpolation = in_interp; key->out_interpolation = out_interp;
  ++g_keyframe_mutations; return 0;
}
int32_t __cdecl start_add_keyframes(void* stream, void** transaction) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !transaction || g_add_keyframe_transactions.size() >= 8) {
    ++g_invalid_keyframe_operations; return 4;
  }
  g_add_keyframe_transactions.push_back({{}, record, {}});
  *transaction = &g_add_keyframe_transactions.back().opaque;
  return 0;
}
int32_t __cdecl add_keyframes(void* handle, int16_t time_mode, const HostTime* time,
                              int32_t* index) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  if (!transaction || time_mode < 0 || time_mode > 1 || !valid_time(time) || !index ||
      transaction->stream->mask->keyframes.size() + transaction->staged.size() >=
          kMaxKeyframesPerStream) {
    ++g_invalid_keyframe_operations; return 4;
  }
  const auto duplicate = std::find_if(transaction->staged.begin(), transaction->staged.end(),
      [time](const auto& key) { return time_equal(key.time, *time); });
  if (duplicate != transaction->staged.end()) {
    *index = static_cast<int32_t>(duplicate - transaction->staged.begin()); return 0;
  }
  transaction->staged.push_back(snapshot_keyframe(*transaction->stream->mask, *time));
  *index = static_cast<int32_t>(transaction->staged.size() - 1);
  return 0;
}
int32_t __cdecl set_add_keyframe(void* handle, int32_t index, const StreamValue* value) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  OutlineData* source = value ? find_outline(value->value) : nullptr;
  if (!transaction || index < 0 || static_cast<std::size_t>(index) >= transaction->staged.size() ||
      !source) { ++g_invalid_keyframe_operations; return 4; }
  static_cast<OutlineData&>(transaction->staged[index]) = *source;
  return 0;
}
int32_t __cdecl end_add_keyframes(uint8_t add, void* handle) {
  AddKeyframesTransaction* transaction = find_add_transaction(handle);
  if (!transaction) { ++g_invalid_keyframe_operations; return 4; }
  if (add) {
    for (auto& staged : transaction->staged) {
      auto position = transaction->stream->mask->keyframes.begin();
      while (position != transaction->stream->mask->keyframes.end() &&
             time_less(position->time, staged.time)) ++position;
      if (position == transaction->stream->mask->keyframes.end() ||
          !time_equal(position->time, staged.time)) {
        transaction->stream->mask->keyframes.insert(position, std::move(staged));
        ++g_keyframe_mutations;
      }
    }
  }
  g_add_keyframe_transactions.erase(std::find_if(g_add_keyframe_transactions.begin(),
      g_add_keyframe_transactions.end(), [transaction](auto& item) { return &item == transaction; }));
  return 0;
}
int32_t __cdecl get_keyframe_label(void* stream, int32_t index, int32_t* label) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || !label) return 4;
  *label = key->label; return 0;
}
int32_t __cdecl set_keyframe_label(void* stream, int32_t index, int32_t label) {
  HostKeyframe* key = keyframe_at(find_stream(stream), index);
  if (!key || label < 0 || label > 16) { ++g_invalid_keyframe_operations; return 4; }
  key->label = label; ++g_keyframe_mutations; return 0;
}

bool verify_keyframe_ownership_rejection() {
  const uint32_t invalid_before = g_invalid_keyframe_operations;
  const uint32_t mutations_before = g_keyframe_mutations;
  void* mask = nullptr; void* stream = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0) return false;
  const HostTime later{20, 1}, earlier{10, 1}, batched{30, 1}, cancelled{40, 1};
  int32_t later_index = -1, earlier_index = -1, duplicate_index = -1, count = -1;
  HostTime observed{};
  bool passed = insert_keyframe(stream, 0, &later, &later_index) == 0 && later_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &earlier_index) == 0 && earlier_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &duplicate_index) == 0 && duplicate_index == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      get_keyframe_time(stream, 0, 0, &observed) == 0 && time_equal(observed, earlier) &&
      set_keyframe_flag(stream, 0, 1, 1) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 3) == 0 &&
      set_keyframe_label(stream, 0, 7) == 0;
  int32_t flags{}, in_interp{}, out_interp{}, label{};
  passed = passed && get_keyframe_flags(stream, 0, &flags) == 0 && flags == 1 &&
      get_keyframe_interpolation(stream, 0, &in_interp, &out_interp) == 0 &&
      in_interp == 2 && out_interp == 3 &&
      get_keyframe_label(stream, 0, &label) == 0 && label == 7;
  StreamValue later_value{}, hold_value{}, midpoint_value{};
  MaskVertex later_vertex{}, hold_vertex{}, midpoint_vertex{};
  const HostTime midpoint{15, 1};
  passed = passed && get_new_keyframe_value(1, stream, 1, &later_value) == 0 &&
      get_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0;
  later_vertex.x += 10;
  passed = passed && set_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0 &&
      set_keyframe_value(stream, 1, &later_value) == 0 &&
      dispose_stream_value(&later_value) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &hold_value) == 0 &&
      get_mask_outline_vertex_info(hold_value.value, 0, &hold_vertex) == 0 &&
      hold_vertex.x == later_vertex.x - 10 && dispose_stream_value(&hold_value) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 1) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &midpoint_value) == 0 &&
      get_mask_outline_vertex_info(midpoint_value.value, 0, &midpoint_vertex) == 0 &&
      midpoint_vertex.x == later_vertex.x - 5 && dispose_stream_value(&midpoint_value) == 0;
  StreamValue source{}, checked{};
  passed = passed && get_new_stream_value(1, stream, 0, nullptr, 0, &source) == 0 &&
      set_keyframe_value(stream, 0, &source) == 0 &&
      get_new_keyframe_value(1, stream, 0, &checked) == 0 &&
      checked.value != source.value && delete_keyframe(stream, 0) != 0 &&
      reject_spatial_tangents(1, stream, 0, nullptr, nullptr) != 0 &&
      dispose_stream_value(&checked) == 0 && dispose_stream_value(&source) == 0;
  void* transaction = nullptr; int32_t staged_index = -1;
  passed = passed && start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &cancelled, &staged_index) == 0 && staged_index == 0 &&
      end_add_keyframes(0, transaction) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &batched, &staged_index) == 0 &&
      end_add_keyframes(1, transaction) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 3 &&
      delete_keyframe(stream, 2) == 0 && delete_keyframe(stream, 1) == 0 &&
      delete_keyframe(stream, 0) == 0 && dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  return passed && g_invalid_keyframe_operations == invalid_before + 2 &&
      g_keyframe_mutations == mutations_before + 12 && mask_lifetimes_balanced();
}

bool dynamic_leaf(DynamicNodeKind kind) {
  return kind == DynamicNodeKind::MaskOutline || kind == DynamicNodeKind::MaskFeather ||
      kind == DynamicNodeKind::MaskOpacity || kind == DynamicNodeKind::MaskExpansion;
}
const char* dynamic_match_name(DynamicNodeKind kind) {
  switch (kind) {
    case DynamicNodeKind::LayerRoot: return "ADBE Abstract Layer";
    case DynamicNodeKind::MaskParade: return "ADBE Mask Parade";
    case DynamicNodeKind::MaskAtom: return "ADBE Mask Atom";
    case DynamicNodeKind::MaskOutline: return "ADBE Mask Shape";
    case DynamicNodeKind::MaskFeather: return "ADBE Mask Feather";
    case DynamicNodeKind::MaskOpacity: return "ADBE Mask Opacity";
    case DynamicNodeKind::MaskExpansion: return "ADBE Mask Offset";
  }
  return "";
}
int32_t dynamic_depth(DynamicNodeKind kind) {
  if (kind == DynamicNodeKind::LayerRoot) return 0;
  if (kind == DynamicNodeKind::MaskParade) return 1;
  if (kind == DynamicNodeKind::MaskAtom) return 2;
  return 3;
}
uint32_t* dynamic_flags(HostStreamRef* stream) {
  if (!stream) return nullptr;
  if (stream->kind == DynamicNodeKind::LayerRoot) return &g_layer_dynamic_flags;
  if (stream->kind == DynamicNodeKind::MaskParade) return &g_mask_parade_dynamic_flags;
  if (!stream->mask) return nullptr;
  const std::size_t index = stream->kind == DynamicNodeKind::MaskOutline ? 0 :
      stream->kind == DynamicNodeKind::MaskFeather ? 1 :
      stream->kind == DynamicNodeKind::MaskOpacity ? 2 :
      stream->kind == DynamicNodeKind::MaskExpansion ? 3 : 4;
  return &stream->mask->dynamic_flags[index];
}
int32_t __cdecl get_new_dynamic_stream_for_layer(int32_t plugin_id, void* layer, void** stream) {
  if (plugin_id != 1 || layer != &g_layer || !stream) return 4;
  return create_stream_ref(nullptr, DynamicNodeKind::LayerRoot, -1, stream);
}
int32_t __cdecl get_new_dynamic_stream_for_mask(int32_t plugin_id, void* mask, void** stream) {
  HostMask* record = find_mask(mask);
  if (plugin_id != 1 || !usable_mask(record) || !stream) return 4;
  return create_stream_ref(record, DynamicNodeKind::MaskAtom, -1, stream);
}
int32_t __cdecl get_dynamic_stream_depth(void* stream, int32_t* depth) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !depth) return 4;
  *depth = dynamic_depth(record->kind); ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_grouping_type(void* stream, int32_t* grouping) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !grouping) return 4;
  *grouping = dynamic_leaf(record->kind) ? 0 :
      record->kind == DynamicNodeKind::MaskParade ? 2 : 1;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_num_streams_in_group(void* stream, int32_t* count) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !count || dynamic_leaf(record->kind)) return 4;
  *count = record->kind == DynamicNodeKind::LayerRoot ? 1 :
      record->kind == DynamicNodeKind::MaskParade ? static_cast<int32_t>(active_mask_count()) : 4;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_flags(void* stream, uint32_t* flags) {
  uint32_t* stored = dynamic_flags(find_stream(stream));
  if (!stored || !flags) return 4;
  *flags = *stored; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl set_dynamic_stream_flag(void* stream, uint32_t flag, uint8_t undoable,
                                        uint8_t set) {
  HostStreamRef* record = find_stream(stream); uint32_t* stored = dynamic_flags(record);
  if (!stored || (flag != 1 && flag != 2) || (!undoable && flag != 2)) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  if (set) *stored |= flag; else *stored &= ~flag;
  if (record->mask) record->mask->dynamic_modified = true;
  ++g_dynamic_stream_mutations; return 0;
}
int32_t dynamic_child(HostStreamRef* parent, int32_t index, HostMask*& mask,
                      DynamicNodeKind& kind) {
  if (!parent || index < 0 || dynamic_leaf(parent->kind)) return 4;
  mask = parent->mask;
  if (parent->kind == DynamicNodeKind::LayerRoot) {
    if (index != 0) return 4; kind = DynamicNodeKind::MaskParade; mask = nullptr; return 0;
  }
  if (parent->kind == DynamicNodeKind::MaskParade) {
    const auto masks = ordered_active_masks();
    if (static_cast<std::size_t>(index) >= masks.size()) return 4;
    mask = masks[static_cast<std::size_t>(index)]; kind = DynamicNodeKind::MaskAtom; return 0;
  }
  static constexpr DynamicNodeKind children[4]{DynamicNodeKind::MaskOutline,
      DynamicNodeKind::MaskFeather, DynamicNodeKind::MaskOpacity,
      DynamicNodeKind::MaskExpansion};
  if (index >= 4) return 4; kind = children[index]; return 0;
}
int32_t __cdecl get_new_dynamic_stream_by_index(int32_t plugin_id, void* parent_stream,
                                                int32_t index, void** stream) {
  HostStreamRef* parent = find_stream(parent_stream); HostMask* mask{}; DynamicNodeKind kind{};
  if (plugin_id != 1 || !stream || dynamic_child(parent, index, mask, kind) != 0) {
    if (stream) *stream = nullptr; return 4;
  }
  ++g_dynamic_stream_queries;
  return create_stream_ref(mask, kind, kind == DynamicNodeKind::MaskOutline ? 400 :
      kind == DynamicNodeKind::MaskOpacity ? 401 : kind == DynamicNodeKind::MaskFeather ? 402 :
      kind == DynamicNodeKind::MaskExpansion ? 403 : -1, stream);
}
int32_t __cdecl get_new_dynamic_stream_by_match_name(int32_t plugin_id, void* parent_stream,
                                                     const char* match_name, void** stream) {
  HostStreamRef* parent = find_stream(parent_stream);
  if (plugin_id != 1 || !parent || !match_name || !stream ||
      std::strlen(match_name) >= 40 || dynamic_leaf(parent->kind) ||
      parent->kind == DynamicNodeKind::MaskParade) {
    if (stream) *stream = nullptr; return 4;
  }
  const int32_t count = parent->kind == DynamicNodeKind::LayerRoot ? 1 : 4;
  for (int32_t index = 0; index < count; ++index) {
    HostMask* mask{}; DynamicNodeKind kind{};
    if (dynamic_child(parent, index, mask, kind) == 0 &&
        std::strcmp(match_name, dynamic_match_name(kind)) == 0)
      return get_new_dynamic_stream_by_index(plugin_id, parent_stream, index, stream);
  }
  *stream = nullptr; return 4;
}
int32_t __cdecl delete_dynamic_stream(void* stream) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask ||
      record->mask->deleted || std::any_of(g_stream_refs.begin(), g_stream_refs.end(),
          [record](const auto& other) { return &other != record && other.mask == record->mask; })) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  const int32_t deleted_order = record->mask->dynamic_order;
  record->mask->deleted = true; record->mask->dynamic_modified = true;
  for (auto& candidate : g_mask_scene)
    if (!candidate.deleted && candidate.dynamic_order > deleted_order) --candidate.dynamic_order;
  ++g_dynamic_stream_mutations; return 0;
}
int32_t __cdecl reorder_dynamic_stream(void* stream, int32_t new_index) {
  HostStreamRef* record = find_stream(stream); const auto masks = ordered_active_masks();
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || new_index < 0 ||
      static_cast<std::size_t>(new_index) >= masks.size()) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  const int32_t old_index = record->mask->dynamic_order;
  for (auto* mask : masks) {
    if (old_index < new_index && mask->dynamic_order > old_index && mask->dynamic_order <= new_index)
      --mask->dynamic_order;
    else if (old_index > new_index && mask->dynamic_order >= new_index && mask->dynamic_order < old_index)
      ++mask->dynamic_order;
  }
  record->mask->dynamic_order = new_index; record->mask->dynamic_modified = true;
  ++g_dynamic_stream_mutations; return 0;
}
int32_t __cdecl duplicate_dynamic_stream(int32_t plugin_id, void* stream, int32_t* new_index) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || record->kind != DynamicNodeKind::MaskAtom || !record->mask ||
      g_mask_scene.size() >= kMaxHostMasks) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  HostMask copy = *record->mask;
  copy.mask_live = false; copy.stream_live = false; copy.value_live = false; copy.deleted = false;
  copy.id = g_next_mask_id++; copy.outline_stream_id = g_next_stream_id++;
  copy.feather_stream_id = g_next_stream_id++; copy.opacity_stream_id = g_next_stream_id++;
  copy.expansion_stream_id = g_next_stream_id++; copy.dynamic_order = static_cast<int32_t>(active_mask_count());
  copy.dynamic_modified = true; g_mask_scene.push_back(std::move(copy));
  if (new_index) *new_index = g_mask_scene.back().dynamic_order;
  ++g_dynamic_stream_mutations; return 0;
}
int32_t __cdecl set_dynamic_stream_name(void* stream, const uint16_t* name) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || !name) {
    ++g_invalid_dynamic_stream_operations; return 4;
  }
  std::size_t length = 0; while (length <= 127 && name[length]) ++length;
  if (length > 127) { ++g_invalid_dynamic_stream_operations; return 4; }
  record->mask->dynamic_name.assign(reinterpret_cast<const char16_t*>(name), length);
  record->mask->dynamic_modified = true; ++g_dynamic_stream_mutations; return 0;
}
int32_t __cdecl can_add_dynamic_stream(void* stream, const char* match_name, uint8_t* can_add) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !match_name || !can_add || std::strlen(match_name) >= 40) return 4;
  *can_add = record->kind == DynamicNodeKind::MaskParade &&
      std::strcmp(match_name, "ADBE Mask Atom") == 0 && g_mask_scene.size() < kMaxHostMasks;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl add_dynamic_stream(int32_t plugin_id, void* stream, const char* match_name,
                                   void** added) {
  HostStreamRef* group = find_stream(stream); uint8_t can_add{};
  if (plugin_id != 1 || !added || can_add_dynamic_stream(stream, match_name, &can_add) != 0 ||
      !can_add || !group) { if (added) *added = nullptr; ++g_invalid_dynamic_stream_operations; return 4; }
  HostMask mask; mask.id = g_next_mask_id++; mask.outline_stream_id = g_next_stream_id++;
  mask.feather_stream_id = g_next_stream_id++; mask.opacity_stream_id = g_next_stream_id++;
  mask.expansion_stream_id = g_next_stream_id++; mask.dynamic_order = static_cast<int32_t>(active_mask_count());
  mask.dynamic_modified = true; g_mask_scene.push_back(std::move(mask));
  ++g_dynamic_stream_mutations;
  return create_stream_ref(&g_mask_scene.back(), DynamicNodeKind::MaskAtom, -1, added);
}
int32_t __cdecl get_dynamic_match_name(void* stream, char* match_name) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !match_name) return 4;
  strcpy_s(match_name, 40, dynamic_match_name(record->kind));
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_new_parent_dynamic_stream(int32_t plugin_id, void* stream, void** parent) {
  HostStreamRef* record = find_stream(stream);
  if (plugin_id != 1 || !record || !parent || record->kind == DynamicNodeKind::LayerRoot) {
    if (parent) *parent = nullptr; return 4;
  }
  DynamicNodeKind kind = record->kind == DynamicNodeKind::MaskParade ? DynamicNodeKind::LayerRoot :
      record->kind == DynamicNodeKind::MaskAtom ? DynamicNodeKind::MaskParade : DynamicNodeKind::MaskAtom;
  HostMask* mask = kind == DynamicNodeKind::MaskAtom ? record->mask : nullptr;
  ++g_dynamic_stream_queries; return create_stream_ref(mask, kind, -1, parent);
}
int32_t __cdecl get_dynamic_stream_modified(void* stream, uint8_t* modified) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !modified) return 4;
  *modified = record->mask && record->mask->dynamic_modified ? 1 : 0;
  ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl get_dynamic_stream_index(void* stream, int32_t* index) {
  HostStreamRef* record = find_stream(stream);
  if (!record || record->kind != DynamicNodeKind::MaskAtom || !record->mask || !index) return 4;
  *index = record->mask->dynamic_order; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl is_separation_leader(void* stream, uint8_t* leader) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !leader) return 4;
  *leader = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl are_dimensions_separated(void* stream, uint8_t* separated) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !separated) return 4;
  *separated = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl reject_set_dimensions_separated(void* stream, uint8_t) {
  if (!find_stream(stream)) return 4; ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl reject_get_separation_follower(void* stream, int16_t, void** follower) {
  if (follower) *follower = nullptr; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl is_separation_follower(void* stream, uint8_t* follower) {
  HostStreamRef* record = find_stream(stream);
  if (!record || !dynamic_leaf(record->kind) || !follower) return 4;
  *follower = 0; ++g_dynamic_stream_queries; return 0;
}
int32_t __cdecl reject_get_separation_leader(void* stream, void** leader) {
  if (leader) *leader = nullptr; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}
int32_t __cdecl reject_get_separation_dimension(void* stream, int16_t* dimension) {
  if (dimension) *dimension = 0; if (!find_stream(stream)) return 4;
  ++g_invalid_dynamic_stream_operations; return 4;
}

bool verify_dynamic_stream_tree_rejection() {
  const auto original_scene = g_mask_scene;
  const uint32_t mutations_before = g_dynamic_stream_mutations;
  const uint32_t invalid_before = g_invalid_dynamic_stream_operations;
  void* mask = nullptr; void* mask_root = nullptr; void* layer_root = nullptr;
  void* parade = nullptr; void* atom = nullptr; void* outline = nullptr;
  void* opacity = nullptr; void* parent = nullptr; void* added = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_dynamic_stream_for_mask(1, mask, &mask_root) != 0 ||
      get_new_dynamic_stream_for_layer(1, &g_layer, &layer_root) != 0 ||
      get_new_dynamic_stream_by_match_name(1, layer_root, "ADBE Mask Parade", &parade) != 0 ||
      get_new_dynamic_stream_by_index(1, parade, 0, &atom) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Shape", &outline) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Opacity", &opacity) != 0)
    return false;
  int32_t depth{}, grouping{}, count{}, index{}; char match_name[40]{};
  uint32_t flags{}; uint8_t boolean{};
  bool passed = get_dynamic_stream_depth(layer_root, &depth) == 0 && depth == 0 &&
      get_dynamic_stream_grouping_type(parade, &grouping) == 0 && grouping == 2 &&
      get_num_streams_in_group(atom, &count) == 0 && count == 4 &&
      get_dynamic_match_name(outline, match_name) == 0 &&
      std::strcmp(match_name, "ADBE Mask Shape") == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0 &&
      get_new_parent_dynamic_stream(1, outline, &parent) == 0 &&
      get_dynamic_stream_grouping_type(parent, &grouping) == 0 && grouping == 1 &&
      set_dynamic_stream_flag(outline, 2, 0, 1) == 0 &&
      get_dynamic_stream_flags(outline, &flags) == 0 && flags == 2 &&
      set_dynamic_stream_flag(outline, 1, 0, 1) != 0 &&
      is_separation_leader(opacity, &boolean) == 0 && boolean == 0;
  StreamValue opacity_value{};
  passed = passed && get_new_stream_value(1, opacity, 0, nullptr, 0, &opacity_value) == 0 &&
      opacity_value.one_d == 100.0;
  opacity_value.one_d = 75.0;
  passed = passed && reject_set_stream_value(1, opacity, &opacity_value) == 0 &&
      dispose_stream_value(&opacity_value) == 0 &&
      can_add_dynamic_stream(parade, "ADBE Mask Atom", &boolean) == 0 && boolean == 1 &&
      add_dynamic_stream(1, parade, "ADBE Mask Atom", &added) == 0;
  int32_t duplicate_index = -1;
  passed = passed && duplicate_dynamic_stream(1, atom, &duplicate_index) == 0 &&
      duplicate_index == 2 && reorder_dynamic_stream(atom, 2) == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 2;
  const uint16_t renamed[]{'R','e','n','a','m','e','d',0};
  passed = passed && set_dynamic_stream_name(atom, renamed) == 0 &&
      get_dynamic_stream_modified(atom, &boolean) == 0 && boolean == 1;
  void* duplicate = nullptr;
  passed = passed && get_new_dynamic_stream_by_index(1, parade, 1, &duplicate) == 0 &&
      delete_dynamic_stream(duplicate) == 0 && dispose_stream(duplicate) == 0 &&
      delete_dynamic_stream(added) == 0 && dispose_stream(added) == 0 &&
      get_num_streams_in_group(parade, &count) == 0 && count == 1 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0;
  passed = passed && dispose_stream(parent) == 0 && dispose_stream(opacity) == 0 &&
      dispose_stream(outline) == 0 && dispose_stream(atom) == 0 &&
      dispose_stream(parade) == 0 && dispose_stream(layer_root) == 0 &&
      dispose_stream(mask_root) == 0 && dispose_mask(mask) == 0;
  const bool balanced = mask_lifetimes_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  return passed && balanced && g_dynamic_stream_mutations == mutations_before + 8 &&
      g_invalid_dynamic_stream_operations == invalid_before + 1;
}

bool verify_aegp_memory_and_strings_rejection() {
  const auto original_scene = g_mask_scene;
  const uint32_t invalid_before = g_invalid_aegp_memory_operations;
  const uint32_t created_before = g_aegp_memory_created;
  const uint32_t freed_before = g_aegp_memory_freed;
  void* memory = nullptr; void* data = nullptr; uint32_t size{}; int32_t count{}, total{};
  bool passed = new_aegp_mem_handle(1, "fault probe", 32, 1, &memory) == 0 &&
      lock_aegp_mem_handle(memory, &data) == 0 && data &&
      std::all_of(static_cast<std::byte*>(data), static_cast<std::byte*>(data) + 32,
          [](std::byte value) { return value == std::byte{}; }) &&
      lock_aegp_mem_handle(memory, &data) == 0 &&
      resize_aegp_mem_handle("locked", 64, memory) != 0 &&
      unlock_aegp_mem_handle(memory) == 0 && unlock_aegp_mem_handle(memory) == 0 &&
      resize_aegp_mem_handle("resized", 64, memory) == 0 &&
      get_aegp_mem_handle_size(memory, &size) == 0 && size == 64 &&
      get_aegp_mem_stats(1, &count, &total) == 0 && count == 1 && total == 64 &&
      free_aegp_mem_handle(memory) == 0;
  void* mask = nullptr; void* stream = nullptr; void* name = nullptr; void* expression = nullptr;
  passed = passed && get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      get_new_mask_stream(1, mask, 400, &stream) == 0 &&
      unsupported_stream_name(1, stream, 1, &name) == 0 &&
      lock_aegp_mem_handle(name, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"Mask Path" &&
      unlock_aegp_mem_handle(name) == 0 && free_aegp_mem_handle(name) == 0;
  const uint16_t source[]{'t','i','m','e','*','2',0}; uint8_t enabled{};
  passed = passed && unsupported_set_expression(1, stream, source) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 1 &&
      unsupported_get_expression(1, stream, &expression) == 0 &&
      lock_aegp_mem_handle(expression, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"time*2" &&
      unlock_aegp_mem_handle(expression) == 0 && free_aegp_mem_handle(expression) == 0 &&
      reject_expression_state(1, stream, 0) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  const bool balanced = mask_lifetimes_balanced() && aegp_memory_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  return passed && balanced && g_invalid_aegp_memory_operations == invalid_before + 1 &&
      g_aegp_memory_created == created_before + 3 && g_aegp_memory_freed == freed_before + 3;
}

int32_t __cdecl is_mask_outline_open(void* outline, uint8_t* open) {
  OutlineData* record = find_outline(outline);
  if (!record || !open) return 4;
  *open = record->open ? 1 : 0;
  return 0;
}

int32_t __cdecl set_mask_outline_open(void* outline, uint8_t open) {
  OutlineData* record = find_outline(outline);
  if (!record) { ++g_invalid_outline_operations; return 4; }
  const bool requested = open != 0;
  if (record->open == requested) return 0;
  if (requested) {
    if (!record->vertices.empty()) record->vertices.pop_back();
  } else if (!record->vertices.empty()) {
    record->vertices.push_back(record->vertices.front());
  }
  record->open = requested;
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl get_mask_outline_num_segments(void* outline, int32_t* count) {
  OutlineData* record = find_outline(outline);
  if (!record || !count) return 4;
  const std::size_t vertices = distinct_vertex_count(*record);
  *count = static_cast<int32_t>(vertices == 0 ? 0 :
      vertices - static_cast<std::size_t>(record->open));
  return 0;
}

int32_t __cdecl get_mask_outline_vertex_info(void* outline, int32_t index,
                                             MaskVertex* vertex) {
  OutlineData* record = find_outline(outline);
  if (!record || !vertex || index < 0 ||
      static_cast<std::size_t>(index) >= record->vertices.size()) return 4;
  *vertex = record->vertices[static_cast<std::size_t>(index)];
  return 0;
}

bool finite_vertex(const MaskVertex& vertex) {
  return std::isfinite(vertex.x) && std::isfinite(vertex.y) &&
      std::isfinite(vertex.tangent_in_x) && std::isfinite(vertex.tangent_in_y) &&
      std::isfinite(vertex.tangent_out_x) && std::isfinite(vertex.tangent_out_y);
}

int32_t __cdecl set_mask_outline_vertex_info(void* outline, int32_t index,
                                             const MaskVertex* vertex) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || !vertex || !finite_vertex(*vertex) || index < 0 ||
      static_cast<std::size_t>(index) > count ||
      (record->open && static_cast<std::size_t>(index) == count)) {
    ++g_invalid_outline_operations;
    return 4;
  }
  const std::size_t target = static_cast<std::size_t>(index) == count ? 0 :
      static_cast<std::size_t>(index);
  record->vertices[target] = *vertex;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl create_mask_outline_vertex(void* outline, int32_t position) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || count >= kMaxOutlineVertices) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (position == 10922) position = static_cast<int32_t>(count);
  if (position < 0 || static_cast<std::size_t>(position) > count) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (!record->open && count == 0) {
    record->vertices = {MaskVertex{}, MaskVertex{}};
  } else {
    record->vertices.insert(record->vertices.begin() + position, MaskVertex{});
  }
  for (auto& feather : record->feathers)
    if (feather.segment >= position) ++feather.segment;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl delete_mask_outline_vertex(void* outline, int32_t index) {
  OutlineData* record = find_outline(outline);
  const std::size_t count = record ? distinct_vertex_count(*record) : 0;
  if (!record || index < 0 || static_cast<std::size_t>(index) >= count) {
    ++g_invalid_outline_operations;
    return 4;
  }
  if (!record->open && count == 1) record->vertices.clear();
  else record->vertices.erase(record->vertices.begin() + index);
  record->feathers.erase(std::remove_if(record->feathers.begin(), record->feathers.end(),
      [index](const auto& feather) { return feather.segment == index; }), record->feathers.end());
  for (auto& feather : record->feathers)
    if (feather.segment > index) --feather.segment;
  sync_closed_vertex(*record);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl get_mask_outline_num_feathers(void* outline, int32_t* count) {
  OutlineData* record = find_outline(outline);
  if (!record || !count) return 4;
  *count = static_cast<int32_t>(record->feathers.size());
  return 0;
}

bool valid_feather(const OutlineData& mask, const MaskFeather& feather) {
  const std::size_t vertices = distinct_vertex_count(mask);
  const std::size_t segments = vertices == 0 ? 0 : vertices - static_cast<std::size_t>(mask.open);
  return feather.segment >= 0 && static_cast<std::size_t>(feather.segment) < segments &&
      std::isfinite(feather.segment_s) && feather.segment_s >= 0 && feather.segment_s <= 1 &&
      std::isfinite(feather.radius) && std::isfinite(feather.ui_corner_angle) &&
      feather.ui_corner_angle >= 0 && feather.ui_corner_angle <= 1 &&
      std::isfinite(feather.tension) && feather.tension >= 0 && feather.tension <= 1 &&
      feather.interp <= 1 && feather.type <= 1 && (feather.type == 1 || feather.radius >= 0);
}

int32_t __cdecl get_mask_outline_feather_info(void* outline, int32_t index,
                                              MaskFeather* feather) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || index < 0 ||
      static_cast<std::size_t>(index) >= record->feathers.size()) return 4;
  *feather = record->feathers[static_cast<std::size_t>(index)];
  return 0;
}

int32_t __cdecl set_mask_outline_feather_info(void* outline, int32_t index,
                                              const MaskFeather* feather) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || !valid_feather(*record, *feather) || index < 0 ||
      static_cast<std::size_t>(index) >= record->feathers.size()) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers[static_cast<std::size_t>(index)] = *feather;
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl create_mask_outline_feather(void* outline, const MaskFeather* feather,
                                            int32_t* position) {
  OutlineData* record = find_outline(outline);
  if (!record || !feather || !position || !valid_feather(*record, *feather) ||
      record->feathers.size() >= kMaxOutlineFeathers) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers.push_back(*feather);
  *position = static_cast<int32_t>(record->feathers.size() - 1);
  ++g_outline_mutations;
  return 0;
}

int32_t __cdecl delete_mask_outline_feather(void* outline, int32_t index) {
  OutlineData* record = find_outline(outline);
  if (!record || index < 0 || static_cast<std::size_t>(index) >= record->feathers.size()) {
    ++g_invalid_outline_operations;
    return 4;
  }
  record->feathers.erase(record->feathers.begin() + index);
  ++g_outline_mutations;
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
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_stream_value(1, stream, 0, nullptr, 0, &value) != 0)
    return false;
  const int32_t premature_error = dispose_stream(stream);
  return premature_error == 4 && dispose_stream_value(&value) == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0 &&
      mask_lifetimes_balanced();
}

bool verify_stream_metadata_and_ownership_rejection() {
  const uint32_t invalid_before = g_invalid_stream_operations;
  const uint32_t metadata_before = g_stream_metadata_queries;
  const uint32_t duplicates_before = g_stream_duplicates;
  void* mask = nullptr;
  void* stream = nullptr;
  void* duplicate = nullptr;
  void* rejected = reinterpret_cast<void*>(1);
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_mask_stream(1, mask, 999, &rejected) == 0 || rejected != nullptr ||
      duplicate_stream_ref(1, stream, &duplicate) != 0)
    return false;
  uint8_t boolean{};
  int32_t interpolations{}, flags{}, type{}, id{}, duplicate_id{};
  double minimum = -1, maximum = -1;
  char units[32]{'x'};
  StreamValue first{}, second{};
  bool passed = can_vary_over_time(stream, &boolean) == 0 && boolean == 1 &&
      get_valid_interpolations(stream, &interpolations) == 0 && interpolations == 0xffff &&
      get_stream_units_text(stream, 0, units) == 0 && units[0] == '\0' &&
      get_stream_properties(stream, &flags, &minimum, &maximum) == 0 && flags == 0 &&
      minimum == 0 && maximum == 0 && is_stream_timevarying(stream, &boolean) == 0 &&
      boolean == 0 && get_stream_type(stream, &type) == 0 && type == 11 &&
      get_unique_stream_id(stream, &id) == 0 &&
      get_unique_stream_id(duplicate, &duplicate_id) == 0 && duplicate_id == id &&
      get_expression_state(1, stream, &boolean) == 0 && boolean == 0 &&
      get_new_stream_value(1, stream, 0, nullptr, 0, &first) == 0 &&
      get_new_stream_value(1, duplicate, 0, nullptr, 0, &second) == 0 &&
      first.value == second.value && reject_set_stream_value(1, stream, &first) != 0 &&
      dispose_stream_value(&second) == 0 && dispose_stream_value(&first) == 0 &&
      dispose_stream(duplicate) == 0 && dispose_stream(stream) == 0 &&
      dispose_mask(mask) == 0;
  return passed && g_invalid_stream_operations == invalid_before + 2 &&
      g_stream_metadata_queries == metadata_before + 9 &&
      g_stream_duplicates == duplicates_before + 1 && mask_lifetimes_balanced();
}

bool verify_outline_mutation_rejection() {
  if (g_mask_scene.empty()) return false;
  HostMask& mask = g_mask_scene.front();
  const HostMask original = mask;
  const uint32_t invalid_before = g_invalid_outline_operations;
  const uint32_t mutations_before = g_outline_mutations;
  void* outline = &mask.outline;
  int32_t segments = -1;
  MaskVertex replacement{11, 2, -1, 0, 1, 0};
  MaskVertex observed{};
  MaskFeather feather{1, 0.25, 3.0, 0.5f, 0.75f, 0, 0};
  int32_t feather_index = -1;
  int32_t feather_count = -1;
  bool passed = set_mask_outline_open(outline, 1) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 3 &&
      set_mask_outline_open(outline, 0) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 4 &&
      set_mask_outline_vertex_info(outline, 1, &replacement) == 0 &&
      get_mask_outline_vertex_info(outline, 1, &observed) == 0 && observed.x == 11 &&
      create_mask_outline_vertex(outline, 2) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 5 &&
      delete_mask_outline_vertex(outline, 2) == 0 &&
      create_mask_outline_feather(outline, &feather, &feather_index) == 0 &&
      feather_index == 0 && get_mask_outline_num_feathers(outline, &feather_count) == 0 &&
      feather_count == 1;
  feather.radius = 2.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &feather) == 0 &&
      get_mask_outline_feather_info(outline, 0, &feather) == 0 && feather.radius == 2.0;
  MaskFeather invalid = feather;
  invalid.radius = -1.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &invalid) != 0 &&
      delete_mask_outline_feather(outline, 0) == 0;
  mask = original;
  return passed && g_invalid_outline_operations == invalid_before + 1 &&
      g_outline_mutations == mutations_before + 8;
}

bool verify_mask_attribute_and_ownership_rejection() {
  const auto original_scene = g_mask_scene;
  const uint32_t invalid_before = g_invalid_mask_operations;
  const uint32_t mutations_before = g_mask_mutations;
  void* original = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &original) != 0) return false;
  const double color[4]{1.0, 0.2, 0.4, 0.6};
  double observed_color[4]{};
  uint8_t byte_value{};
  int32_t long_value{};
  bool passed = set_mask_invert(original, 1) == 0 &&
      get_mask_invert(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 3) == 0 && get_mask_mode(original, &long_value) == 0 &&
      long_value == 3 && set_mask_motion_blur(original, 2) == 0 &&
      get_mask_motion_blur(original, &byte_value) == 0 && byte_value == 2 &&
      set_mask_feather_falloff(original, 1) == 0 &&
      get_mask_feather_falloff(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_color(original, color) == 0 && get_mask_color(original, observed_color) == 0 &&
      std::equal(std::begin(color), std::end(color), std::begin(observed_color)) &&
      set_mask_lock(original, 1) == 0 && get_mask_lock(original, &byte_value) == 0 &&
      byte_value == 1 && set_mask_roto_bezier(original, 1) == 0 &&
      get_mask_roto_bezier(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 99) != 0;
  int32_t original_id{}, duplicate_id{}, count{};
  void* duplicate = nullptr;
  passed = passed && get_mask_id(original, &original_id) == 0 &&
      duplicate_mask(original, &duplicate) == 0 &&
      get_mask_id(duplicate, &duplicate_id) == 0 && duplicate_id != original_id &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 2 &&
      delete_mask_from_layer(duplicate) == 0 && dispose_mask(duplicate) == 0 &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 1;
  void* created = nullptr;
  int32_t created_index = -1;
  passed = passed && create_new_mask(&g_layer, &created, &created_index) == 0 &&
      created_index == 1 && delete_mask_from_layer(created) == 0 &&
      dispose_mask(created) == 0 && dispose_mask(original) == 0;
  const bool balanced = mask_lifetimes_balanced();
  g_mask_scene = original_scene;
  g_mask_scene.reserve(kMaxHostMasks);
  return passed && g_invalid_mask_operations == invalid_before + 1 &&
      g_mask_mutations == mutations_before + 11 && balanced;
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
  decltype(&get_mask_invert) get_invert;
  decltype(&set_mask_invert) set_invert;
  decltype(&get_mask_mode) get_mode;
  decltype(&set_mask_mode) set_mode;
  decltype(&get_mask_motion_blur) get_motion_blur;
  decltype(&set_mask_motion_blur) set_motion_blur;
  decltype(&get_mask_feather_falloff) get_feather_falloff;
  decltype(&set_mask_feather_falloff) set_feather_falloff;
  decltype(&get_mask_id) get_id;
  decltype(&create_new_mask) create_new;
  decltype(&delete_mask_from_layer) delete_from_layer;
  decltype(&get_mask_color) get_color;
  decltype(&set_mask_color) set_color;
  decltype(&get_mask_lock) get_lock;
  decltype(&set_mask_lock) set_lock;
  decltype(&get_mask_roto_bezier) get_roto_bezier;
  decltype(&set_mask_roto_bezier) set_roto_bezier;
  decltype(&duplicate_mask) duplicate;
};
struct StreamSuite {
  decltype(&is_stream_legal) is_stream_legal;
  decltype(&can_vary_over_time) can_vary_over_time;
  decltype(&get_valid_interpolations) get_valid_interpolations;
  decltype(&unsupported_new_layer_stream) get_new_layer_stream;
  decltype(&unsupported_effect_stream_count) get_effect_num_param_streams;
  decltype(&unsupported_new_effect_stream) get_new_effect_stream_by_index;
  decltype(&get_new_mask_stream) get_new_mask_stream;
  decltype(&dispose_stream) dispose_stream;
  decltype(&unsupported_stream_name) get_stream_name;
  decltype(&get_stream_units_text) get_stream_units_text;
  decltype(&get_stream_properties) get_stream_properties;
  decltype(&is_stream_timevarying) is_stream_timevarying;
  decltype(&get_stream_type) get_stream_type;
  decltype(&get_new_stream_value) get_new_stream_value;
  decltype(&dispose_stream_value) dispose_stream_value;
  decltype(&reject_set_stream_value) set_stream_value;
  decltype(&unsupported_layer_stream_value) get_layer_stream_value;
  decltype(&get_expression_state) get_expression_state;
  decltype(&reject_expression_state) set_expression_state;
  decltype(&unsupported_get_expression) get_expression;
  decltype(&unsupported_set_expression) set_expression;
  decltype(&duplicate_stream_ref) duplicate_stream_ref;
  decltype(&get_unique_stream_id) get_unique_stream_id;
};
static_assert(sizeof(StreamSuite) == 23 * sizeof(void*));
struct KeyframeSuite {
  decltype(&get_stream_num_keyframes) get_stream_num_keyframes;
  decltype(&get_keyframe_time) get_keyframe_time;
  decltype(&insert_keyframe) insert_keyframe;
  decltype(&delete_keyframe) delete_keyframe;
  decltype(&get_new_keyframe_value) get_new_keyframe_value;
  decltype(&set_keyframe_value) set_keyframe_value;
  decltype(&get_stream_value_dimensionality) get_stream_value_dimensionality;
  decltype(&get_stream_temporal_dimensionality) get_stream_temporal_dimensionality;
  decltype(&reject_spatial_tangents) get_new_keyframe_spatial_tangents;
  decltype(&reject_set_spatial_tangents) set_keyframe_spatial_tangents;
  decltype(&reject_get_temporal_ease) get_keyframe_temporal_ease;
  decltype(&reject_set_temporal_ease) set_keyframe_temporal_ease;
  decltype(&get_keyframe_flags) get_keyframe_flags;
  decltype(&set_keyframe_flag) set_keyframe_flag;
  decltype(&get_keyframe_interpolation) get_keyframe_interpolation;
  decltype(&set_keyframe_interpolation) set_keyframe_interpolation;
  decltype(&start_add_keyframes) start_add_keyframes;
  decltype(&add_keyframes) add_keyframes;
  decltype(&set_add_keyframe) set_add_keyframe;
  decltype(&end_add_keyframes) end_add_keyframes;
  decltype(&get_keyframe_label) get_keyframe_label_color_index;
  decltype(&set_keyframe_label) set_keyframe_label_color_index;
};
static_assert(sizeof(KeyframeSuite) == 22 * sizeof(void*));
struct DynamicStreamSuite {
  decltype(&get_new_dynamic_stream_for_layer) get_new_stream_ref_for_layer;
  decltype(&get_new_dynamic_stream_for_mask) get_new_stream_ref_for_mask;
  decltype(&get_dynamic_stream_depth) get_stream_depth;
  decltype(&get_dynamic_stream_grouping_type) get_stream_grouping_type;
  decltype(&get_num_streams_in_group) get_num_streams_in_group;
  decltype(&get_dynamic_stream_flags) get_dynamic_stream_flags;
  decltype(&set_dynamic_stream_flag) set_dynamic_stream_flag;
  decltype(&get_new_dynamic_stream_by_index) get_new_stream_ref_by_index;
  decltype(&get_new_dynamic_stream_by_match_name) get_new_stream_ref_by_match_name;
  decltype(&delete_dynamic_stream) delete_stream;
  decltype(&reorder_dynamic_stream) reorder_stream;
  decltype(&duplicate_dynamic_stream) duplicate_stream;
  decltype(&set_dynamic_stream_name) set_stream_name;
  decltype(&can_add_dynamic_stream) can_add_stream;
  decltype(&add_dynamic_stream) add_stream;
  decltype(&get_dynamic_match_name) get_match_name;
  decltype(&get_new_parent_dynamic_stream) get_new_parent_stream_ref;
  decltype(&get_dynamic_stream_modified) get_stream_is_modified;
  decltype(&get_dynamic_stream_index) get_stream_index_in_parent;
  decltype(&is_separation_leader) is_separation_leader;
  decltype(&are_dimensions_separated) are_dimensions_separated;
  decltype(&reject_set_dimensions_separated) set_dimensions_separated;
  decltype(&reject_get_separation_follower) get_separation_follower;
  decltype(&is_separation_follower) is_separation_follower;
  decltype(&reject_get_separation_leader) get_separation_leader;
  decltype(&reject_get_separation_dimension) get_separation_dimension;
};
static_assert(sizeof(DynamicStreamSuite) == 26 * sizeof(void*));
struct AegpMemorySuite {
  decltype(&new_aegp_mem_handle) new_mem_handle;
  decltype(&free_aegp_mem_handle) free_mem_handle;
  decltype(&lock_aegp_mem_handle) lock_mem_handle;
  decltype(&unlock_aegp_mem_handle) unlock_mem_handle;
  decltype(&get_aegp_mem_handle_size) get_mem_handle_size;
  decltype(&resize_aegp_mem_handle) resize_mem_handle;
  decltype(&set_aegp_mem_reporting) set_mem_reporting_on;
  decltype(&get_aegp_mem_stats) get_mem_stats;
};
static_assert(sizeof(AegpMemorySuite) == 8 * sizeof(void*));
struct MaskOutlineSuite {
  decltype(&is_mask_outline_open) is_open;
  decltype(&set_mask_outline_open) set_open;
  decltype(&get_mask_outline_num_segments) get_num_segments;
  decltype(&get_mask_outline_vertex_info) get_vertex_info;
  decltype(&set_mask_outline_vertex_info) set_vertex_info;
  decltype(&create_mask_outline_vertex) create_vertex;
  decltype(&delete_mask_outline_vertex) delete_vertex;
  decltype(&get_mask_outline_num_feathers) get_num_feathers;
  decltype(&get_mask_outline_feather_info) get_feather_info;
  decltype(&set_mask_outline_feather_info) set_feather_info;
  decltype(&create_mask_outline_feather) create_feather;
  decltype(&delete_mask_outline_feather) delete_feather;
};

UtilitySuite g_utility_suite{{}, &register_with_aegp};
PfInterfaceSuite g_pf_interface_suite{&get_effect_layer};
MaskSuite g_mask_suite{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask,
    &get_mask_invert, &set_mask_invert, &get_mask_mode, &set_mask_mode,
    &get_mask_motion_blur, &set_mask_motion_blur,
    &get_mask_feather_falloff, &set_mask_feather_falloff, &get_mask_id,
    &create_new_mask, &delete_mask_from_layer, &get_mask_color, &set_mask_color,
    &get_mask_lock, &set_mask_lock, &get_mask_roto_bezier, &set_mask_roto_bezier,
    &duplicate_mask};
StreamSuite g_stream_suite{&is_stream_legal, &can_vary_over_time,
    &get_valid_interpolations, &unsupported_new_layer_stream,
    &unsupported_effect_stream_count, &unsupported_new_effect_stream,
    &get_new_mask_stream, &dispose_stream, &unsupported_stream_name,
    &get_stream_units_text, &get_stream_properties, &is_stream_timevarying,
    &get_stream_type, &get_new_stream_value, &dispose_stream_value,
    &reject_set_stream_value, &unsupported_layer_stream_value,
    &get_expression_state, &reject_expression_state, &unsupported_get_expression,
    &unsupported_set_expression, &duplicate_stream_ref, &get_unique_stream_id};
KeyframeSuite g_keyframe_suite{&get_stream_num_keyframes, &get_keyframe_time,
    &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
    &set_keyframe_value, &get_stream_value_dimensionality,
    &get_stream_temporal_dimensionality, &reject_spatial_tangents,
    &reject_set_spatial_tangents, &reject_get_temporal_ease,
    &reject_set_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
    &get_keyframe_interpolation, &set_keyframe_interpolation,
    &start_add_keyframes, &add_keyframes, &set_add_keyframe,
    &end_add_keyframes, &get_keyframe_label, &set_keyframe_label};
DynamicStreamSuite g_dynamic_stream_suite{&get_new_dynamic_stream_for_layer,
    &get_new_dynamic_stream_for_mask, &get_dynamic_stream_depth,
    &get_dynamic_stream_grouping_type, &get_num_streams_in_group,
    &get_dynamic_stream_flags, &set_dynamic_stream_flag,
    &get_new_dynamic_stream_by_index, &get_new_dynamic_stream_by_match_name,
    &delete_dynamic_stream, &reorder_dynamic_stream, &duplicate_dynamic_stream,
    &set_dynamic_stream_name, &can_add_dynamic_stream, &add_dynamic_stream,
    &get_dynamic_match_name, &get_new_parent_dynamic_stream,
    &get_dynamic_stream_modified, &get_dynamic_stream_index,
    &is_separation_leader, &are_dimensions_separated,
    &reject_set_dimensions_separated, &reject_get_separation_follower,
    &is_separation_follower, &reject_get_separation_leader,
    &reject_get_separation_dimension};
AegpMemorySuite g_aegp_memory_suite{&new_aegp_mem_handle, &free_aegp_mem_handle,
    &lock_aegp_mem_handle, &unlock_aegp_mem_handle, &get_aegp_mem_handle_size,
    &resize_aegp_mem_handle, &set_aegp_mem_reporting, &get_aegp_mem_stats};
MaskOutlineSuite g_mask_outline_suite{&is_mask_outline_open, &set_mask_outline_open,
                                      &get_mask_outline_num_segments,
                                      &get_mask_outline_vertex_info,
                                      &set_mask_outline_vertex_info,
                                      &create_mask_outline_vertex,
                                      &delete_mask_outline_vertex,
                                      &get_mask_outline_num_feathers,
                                      &get_mask_outline_feather_info,
                                      &set_mask_outline_feather_info,
                                      &create_mask_outline_feather,
                                      &delete_mask_outline_feather};

constexpr int32_t kPixelFormatArgb32 = 1650946657;
constexpr int32_t kPixelFormatArgb64 = 909206881;
constexpr int32_t kPixelFormatArgb128 = 842229089;
constexpr std::size_t kEffectWorldSize = 120;
constexpr uint64_t kMaxWorldBytes = 256ULL * 1024 * 1024;
constexpr std::size_t kMaxWorldCount = 64;

struct LocalRect { int32_t left, top, right, bottom; };
struct LocalRationalScale { int32_t num; uint32_t den; };
struct LocalEffectWorld {
  void* reserved0;
  void* reserved1;
  int32_t world_flags;
  void* data;
  int32_t rowbytes;
  int32_t width;
  int32_t height;
  LocalRect extent_hint;
  void* platform_ref;
  int32_t reserved_long1;
  void* reserved_long4;
  LocalRationalScale pix_aspect_ratio;
  void* reserved_long2;
  int32_t origin_x;
  int32_t origin_y;
  int32_t reserved_long3;
  int32_t dephault;
};
static_assert(sizeof(LocalEffectWorld) == kEffectWorldSize);
static_assert(offsetof(LocalEffectWorld, world_flags) == 16);
static_assert(offsetof(LocalEffectWorld, data) == 24);
static_assert(offsetof(LocalEffectWorld, rowbytes) == 32);
static_assert(offsetof(LocalEffectWorld, extent_hint) == 44);
static_assert(offsetof(LocalEffectWorld, pix_aspect_ratio) == 88);

struct OwnedWorld {
  void* pixels{};
  uint64_t size{};
  int32_t pixel_format{};
};
std::unordered_map<void*, OwnedWorld> g_owned_worlds;
std::mutex g_world_mutex;
uint32_t g_worlds_created{};
uint32_t g_worlds_disposed{};
uint32_t g_invalid_world_operations{};
uint64_t g_world_bytes{};

bool world_lifetimes_balanced() {
  std::lock_guard<std::mutex> lock(g_world_mutex);
  return g_owned_worlds.empty() && g_worlds_created == g_worlds_disposed &&
      g_world_bytes == 0;
}

int32_t __cdecl new_world(void*, int32_t width, int32_t height, int32_t clear_pixels,
                          int32_t pixel_format, void* world) {
  std::lock_guard<std::mutex> lock(g_world_mutex);
  int32_t bytes_per_pixel = 0;
  if (pixel_format == kPixelFormatArgb32) bytes_per_pixel = 4;
  else if (pixel_format == kPixelFormatArgb64) bytes_per_pixel = 8;
  else if (pixel_format == kPixelFormatArgb128) bytes_per_pixel = 16;
  if (!world || width <= 0 || height <= 0 || bytes_per_pixel == 0 ||
      g_owned_worlds.count(world) || g_owned_worlds.size() >= kMaxWorldCount) {
    ++g_invalid_world_operations;
    return 4;
  }
  const uint64_t rowbytes64 = static_cast<uint64_t>(width) * bytes_per_pixel;
  const uint64_t size = rowbytes64 * static_cast<uint64_t>(height);
  if (rowbytes64 > static_cast<uint64_t>((std::numeric_limits<int32_t>::max)()) ||
      size > kMaxWorldBytes || g_world_bytes > kMaxWorldBytes - size) {
    ++g_invalid_world_operations;
    return 4;
  }
  void* pixels = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!pixels) return 1;
  // Never expose stale allocator contents when AE leaves scratch pixels unspecified.
  std::memset(pixels, clear_pixels ? 0 : 0xcd, static_cast<std::size_t>(size));
  std::memset(world, 0, kEffectWorldSize);
  auto* bytes = static_cast<std::byte*>(world);
  const int32_t flags = 2 | (pixel_format == kPixelFormatArgb32 ? 0 : 1);
  const int32_t rowbytes = static_cast<int32_t>(rowbytes64);
  const std::array<int32_t, 4> extent{0, 0, width, height};
  const int32_t aspect_num = 1;
  const uint32_t aspect_den = 1;
  std::memcpy(bytes + 16, &flags, sizeof(flags));
  std::memcpy(bytes + 24, &pixels, sizeof(pixels));
  std::memcpy(bytes + 32, &rowbytes, sizeof(rowbytes));
  std::memcpy(bytes + 36, &width, sizeof(width));
  std::memcpy(bytes + 40, &height, sizeof(height));
  std::memcpy(bytes + 44, extent.data(), sizeof(extent));
  std::memcpy(bytes + 88, &aspect_num, sizeof(aspect_num));
  std::memcpy(bytes + 92, &aspect_den, sizeof(aspect_den));
  g_owned_worlds.emplace(world, OwnedWorld{pixels, size, pixel_format});
  ++g_worlds_created;
  g_world_bytes += size;
  return 0;
}

int32_t __cdecl dispose_world(void*, void* world) {
  std::lock_guard<std::mutex> lock(g_world_mutex);
  const auto found = g_owned_worlds.find(world);
  if (!world || found == g_owned_worlds.end()) {
    ++g_invalid_world_operations;
    return 4;
  }
  ::operator delete(found->second.pixels);
  g_world_bytes -= found->second.size;
  g_owned_worlds.erase(found);
  ++g_worlds_disposed;
  std::memset(world, 0, kEffectWorldSize);
  return 0;
}

int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format) {
  if (!world || !pixel_format) return 4;
  {
    std::lock_guard<std::mutex> lock(g_world_mutex);
    const auto found = g_owned_worlds.find(const_cast<void*>(world));
    if (found != g_owned_worlds.end()) {
      *pixel_format = found->second.pixel_format;
      return 0;
    }
  }
  const auto* bytes = static_cast<const std::byte*>(world);
  int32_t rowbytes{};
  int32_t width{};
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  if (width <= 0 || rowbytes == (std::numeric_limits<int32_t>::min)()) return 4;
  const int32_t bytes_per_pixel = std::abs(rowbytes) / width;
  if (bytes_per_pixel >= 16)
    *pixel_format = kPixelFormatArgb128;
  else if (bytes_per_pixel >= 8)
    *pixel_format = kPixelFormatArgb64;
  else
    *pixel_format = kPixelFormatArgb32;
  return 0;
}

struct WorldSuite {
  decltype(&new_world) new_world;
  decltype(&dispose_world) dispose_world;
  decltype(&get_pixel_format) get_pixel_format;
};
WorldSuite g_world_suite{&new_world, &dispose_world, &get_pixel_format};

std::mutex g_pixel_format_mutex;
std::vector<int32_t> g_supported_pixel_formats;
uint32_t g_pixel_format_add_calls{};
uint32_t g_pixel_format_clear_calls{};
uint32_t g_invalid_pixel_format_operations{};
std::atomic_bool g_global_setup_active{false};

bool supported_cpu_pixel_format(int32_t pixel_format) {
  return pixel_format == kPixelFormatArgb32 || pixel_format == kPixelFormatArgb64 ||
      pixel_format == kPixelFormatArgb128;
}

int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active || !supported_cpu_pixel_format(pixel_format)) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  ++g_pixel_format_add_calls;
  if (std::find(g_supported_pixel_formats.begin(), g_supported_pixel_formats.end(),
                pixel_format) == g_supported_pixel_formats.end()) {
    g_supported_pixel_formats.push_back(pixel_format);
  }
  return 0;
}

int32_t __cdecl clear_supported_pixel_formats(void*) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  g_supported_pixel_formats.clear();
  ++g_pixel_format_clear_calls;
  return 0;
}

struct PixelFormatSuite {
  decltype(&add_supported_pixel_format) add_supported_pixel_format;
  decltype(&clear_supported_pixel_formats) clear_supported_pixel_formats;
};
PixelFormatSuite g_pixel_format_suite{&add_supported_pixel_format,
                                      &clear_supported_pixel_formats};

bool verify_pixel_format_registry_rejection() {
  const uint32_t invalid_before = g_invalid_pixel_format_operations;
  const uint32_t add_before = g_pixel_format_add_calls;
  const uint32_t clear_before = g_pixel_format_clear_calls;
  const bool phase_rejected = clear_supported_pixel_formats(&g_effect) != 0;
  g_global_setup_active = true;
  if (clear_supported_pixel_formats(&g_effect) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb64) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0) {
    g_global_setup_active = false;
    return false;
  }
  bool order_valid = false;
  {
    std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
    order_valid = g_supported_pixel_formats ==
        std::vector<int32_t>{kPixelFormatArgb128, kPixelFormatArgb64};
  }
  const bool rejected = add_supported_pixel_format(&g_effect, 1717854562) != 0;
  const bool cleared = clear_supported_pixel_formats(&g_effect) == 0;
  g_global_setup_active = false;
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  return phase_rejected && order_valid && rejected && cleared &&
      g_supported_pixel_formats.empty() &&
      g_invalid_pixel_format_operations == invalid_before + 2 &&
      g_pixel_format_add_calls == add_before + 3 &&
      g_pixel_format_clear_calls == clear_before + 2;
}

bool verify_world_double_dispose_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  const uint32_t invalid_before = g_invalid_world_operations;
  if (new_world(&g_effect, 7, 5, 1, kPixelFormatArgb128, world.data()) != 0) return false;
  void* pixels{};
  int32_t flags{}, rowbytes{}, width{}, height{}, format{};
  std::memcpy(&flags, world.data() + 16, sizeof(flags));
  std::memcpy(&pixels, world.data() + 24, sizeof(pixels));
  std::memcpy(&rowbytes, world.data() + 32, sizeof(rowbytes));
  std::memcpy(&width, world.data() + 36, sizeof(width));
  std::memcpy(&height, world.data() + 40, sizeof(height));
  const bool layout_valid = pixels && flags == 3 && rowbytes == 112 && width == 7 && height == 5 &&
      get_pixel_format(world.data(), &format) == 0 && format == kPixelFormatArgb128 &&
      std::all_of(static_cast<const unsigned char*>(pixels),
                  static_cast<const unsigned char*>(pixels) + 560,
                  [](unsigned char value) { return value == 0; });
  const int32_t first = dispose_world(&g_effect, world.data());
  const int32_t second = dispose_world(&g_effect, world.data());
  return layout_valid && first == 0 && second != 0 &&
      g_invalid_world_operations == invalid_before + 1 && world_lifetimes_balanced();
}

bool verify_world_allocation_limit_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  world.fill(std::byte{0x5a});
  const auto before = world;
  const uint32_t invalid_before = g_invalid_world_operations;
  const int32_t error = new_world(&g_effect, 32768, 32768, 1,
                                  kPixelFormatArgb128, world.data());
  return error != 0 && world == before &&
      g_invalid_world_operations == invalid_before + 1 && world_lifetimes_balanced();
}

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
  if (name && std::strcmp(name, "PF Pixel Format Suite") == 0 && version == 2) {
    *suite = &g_pixel_format_suite;
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
  else if (std::strcmp(name, "AEGP Keyframe Suite") == 0 && version == 5)
    *suite = &g_keyframe_suite;
  else if (std::strcmp(name, "AEGP Dynamic Stream Suite") == 0 && version == 5)
    *suite = &g_dynamic_stream_suite;
  else if (std::strcmp(name, "AEGP Memory Suite") == 0 && version == 1)
    *suite = &g_aegp_memory_suite;
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
  const bool stream_metadata_ownership_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-stream-metadata-ownership-request";
  const bool keyframe_ownership_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-keyframe-ownership-request";
  const bool dynamic_stream_tree_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-dynamic-stream-tree-request";
  const bool aegp_memory_strings_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-aegp-memory-strings-request";
  const bool suite_release_without_acquire_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-suite-release-without-acquire-request";
  const bool handle_resize_while_locked_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-handle-resize-while-locked-request";
  const bool world_double_dispose_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-world-double-dispose-request";
  const bool world_allocation_limit_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-world-allocation-limit-request";
  const bool pixel_format_registry_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-pixel-format-registry-request";
  const bool outline_mutation_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-outline-mutation-request";
  const bool mask_attribute_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--smart-mask-attribute-request";
  const bool request_mode = mask_request_mode || mask_scene_request_mode || mask_context_request_mode ||
      mask_count_error_mode || mask_count_crash_mode || mask_double_dispose_mode ||
      stream_live_value_dispose_mode || stream_metadata_ownership_mode || keyframe_ownership_mode ||
      dynamic_stream_tree_mode ||
      aegp_memory_strings_mode ||
      suite_release_without_acquire_mode ||
      handle_resize_while_locked_mode || world_double_dispose_mode ||
      world_allocation_limit_mode ||
      pixel_format_registry_mode ||
      outline_mutation_mode ||
      mask_attribute_mode ||
      (argc == 5 && std::wstring(argv[1]) == L"--smart-request");
  if (!request_mode && (argc != 5 || std::wstring(argv[1]) != L"--smart")) return 2;
  g_mask_model_enabled = mask_request_mode || mask_scene_request_mode || mask_context_request_mode ||
      mask_count_error_mode || mask_count_crash_mode || mask_double_dispose_mode ||
      stream_live_value_dispose_mode || stream_metadata_ownership_mode || keyframe_ownership_mode ||
      dynamic_stream_tree_mode ||
      aegp_memory_strings_mode ||
      suite_release_without_acquire_mode ||
      handle_resize_while_locked_mode || world_double_dispose_mode ||
      world_allocation_limit_mode;
  g_mask_model_enabled = g_mask_model_enabled || pixel_format_registry_mode ||
      outline_mutation_mode;
  g_mask_model_enabled = g_mask_model_enabled || mask_attribute_mode;
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
  g_global_setup_active = true;
  const int32_t global_error = entry(kGlobalSetup, input.data(), output.data(), nullptr, nullptr, nullptr);
  g_global_setup_active = false;
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
  bool world_fault_observed = false;
  bool pixel_format_fault_observed = false;
  bool outline_fault_observed = false;
  bool mask_attribute_fault_observed = false;
  bool stream_metadata_fault_observed = false;
  bool keyframe_fault_observed = false;
  bool dynamic_stream_fault_observed = false;
  bool aegp_memory_fault_observed = false;
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
  if (world_double_dispose_mode)
    world_fault_observed = verify_world_double_dispose_rejected();
  if (world_allocation_limit_mode)
    world_fault_observed = verify_world_allocation_limit_rejected();
  if (pixel_format_registry_mode)
    pixel_format_fault_observed = verify_pixel_format_registry_rejection();
  if (outline_mutation_mode)
    outline_fault_observed = verify_outline_mutation_rejection();
  if (mask_attribute_mode)
    mask_attribute_fault_observed = verify_mask_attribute_and_ownership_rejection();
  if (stream_metadata_ownership_mode)
    stream_metadata_fault_observed = verify_stream_metadata_and_ownership_rejection();
  if (keyframe_ownership_mode)
    keyframe_fault_observed = verify_keyframe_ownership_rejection();
  if (dynamic_stream_tree_mode)
    dynamic_stream_fault_observed = verify_dynamic_stream_tree_rejection();
  if (aegp_memory_strings_mode)
    aegp_memory_fault_observed = verify_aegp_memory_and_strings_rejection();
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
            << ",\"mask_count\":" << active_mask_count()
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
            << ",\"world_fault_observed\":" << (world_fault_observed ? "true" : "false")
            << ",\"world_lifetimes_balanced\":" << (world_lifetimes_balanced() ? "true" : "false")
            << ",\"worlds_created\":" << g_worlds_created
            << ",\"worlds_disposed\":" << g_worlds_disposed
            << ",\"live_world_count\":" << g_owned_worlds.size()
            << ",\"live_world_bytes\":" << g_world_bytes
            << ",\"invalid_world_operations\":" << g_invalid_world_operations
            << ",\"pixel_format_fault_observed\":" << (pixel_format_fault_observed ? "true" : "false")
            << ",\"pixel_format_add_calls\":" << g_pixel_format_add_calls
            << ",\"pixel_format_clear_calls\":" << g_pixel_format_clear_calls
            << ",\"supported_pixel_format_count\":" << g_supported_pixel_formats.size()
            << ",\"invalid_pixel_format_operations\":" << g_invalid_pixel_format_operations
            << ",\"outline_fault_observed\":" << (outline_fault_observed ? "true" : "false")
            << ",\"outline_mutations\":" << g_outline_mutations
            << ",\"invalid_outline_operations\":" << g_invalid_outline_operations
            << ",\"mask_attribute_fault_observed\":" << (mask_attribute_fault_observed ? "true" : "false")
            << ",\"mask_mutations\":" << g_mask_mutations
            << ",\"invalid_mask_operations\":" << g_invalid_mask_operations
            << ",\"stream_metadata_fault_observed\":" << (stream_metadata_fault_observed ? "true" : "false")
            << ",\"stream_metadata_queries\":" << g_stream_metadata_queries
            << ",\"stream_duplicates\":" << g_stream_duplicates
            << ",\"invalid_stream_operations\":" << g_invalid_stream_operations
            << ",\"keyframe_fault_observed\":" << (keyframe_fault_observed ? "true" : "false")
            << ",\"keyframe_mutations\":" << g_keyframe_mutations
            << ",\"invalid_keyframe_operations\":" << g_invalid_keyframe_operations
            << ",\"dynamic_stream_fault_observed\":" << (dynamic_stream_fault_observed ? "true" : "false")
            << ",\"dynamic_stream_queries\":" << g_dynamic_stream_queries
            << ",\"dynamic_stream_mutations\":" << g_dynamic_stream_mutations
            << ",\"invalid_dynamic_stream_operations\":" << g_invalid_dynamic_stream_operations
            << ",\"aegp_memory_fault_observed\":" << (aegp_memory_fault_observed ? "true" : "false")
            << ",\"aegp_memory_created\":" << g_aegp_memory_created
            << ",\"aegp_memory_freed\":" << g_aegp_memory_freed
            << ",\"live_aegp_memory_handles\":" << g_aegp_memory.size()
            << ",\"live_aegp_memory_bytes\":" << g_aegp_memory_bytes
            << ",\"invalid_aegp_memory_operations\":" << g_invalid_aegp_memory_operations
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
