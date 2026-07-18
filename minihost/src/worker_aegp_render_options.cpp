#include "worker_aegp_render_options.hpp"

#include <atomic>
#include <limits>
#include <mutex>
#include <new>
#include <unordered_map>

namespace aexcompat::render_options {
namespace {
constexpr std::size_t kMaxItemOptions = 32;
constexpr std::size_t kMaxLayerOptions = 256;
std::mutex g_item_mutex;
std::mutex g_layer_mutex;
std::unordered_map<uintptr_t, ItemValue> g_items;
std::unordered_map<uintptr_t, LayerValue> g_layers;
std::atomic<uint64_t> g_item_generation{1};
std::atomic<uint64_t> g_layer_generation{1};
std::atomic<uint32_t> g_item_created{};
std::atomic<uint32_t> g_item_disposed{};
std::atomic<uint32_t> g_item_invalid{};
std::atomic<uint32_t> g_layer_created{};
std::atomic<uint32_t> g_layer_disposed{};
std::atomic<uint32_t> g_layer_invalid{};
ItemValidator g_item_validator{};
LayerInitializer g_layer_initializer{};

uintptr_t key(void* handle) { return reinterpret_cast<uintptr_t>(handle); }
void* next_handle(std::atomic<uint64_t>& generation, unsigned shift, uintptr_t tag) {
  const uint64_t value = generation.fetch_add(1);
  if (!value || value > (std::numeric_limits<uintptr_t>::max)() / (uintptr_t{1} << shift))
    return nullptr;
  return reinterpret_cast<void*>((static_cast<uintptr_t>(value) << shift) | tag);
}
int32_t insert_item(const ItemValue& value, void** output) {
  if (!output) return 4;
  *output = nullptr;
  void* handle = next_handle(g_item_generation, 1, 1);
  if (!handle) return 4;
  std::lock_guard<std::mutex> lock(g_item_mutex);
  if (g_items.size() >= kMaxItemOptions) return 4;
  try { if (!g_items.emplace(key(handle), value).second) return 4; }
  catch (const std::bad_alloc&) { return 4; }
  ++g_item_created; *output = handle; return 0;
}
int32_t insert_layer(const LayerValue& value, void** output) {
  if (!output) return 4;
  *output = nullptr;
  void* handle = next_handle(g_layer_generation, 2, 2);
  if (!handle) return 4;
  std::lock_guard<std::mutex> lock(g_layer_mutex);
  if (g_layers.size() >= kMaxLayerOptions) return 4;
  try { if (!g_layers.emplace(key(handle), value).second) return 4; }
  catch (const std::bad_alloc&) { return 4; }
  ++g_layer_created; *output = handle; return 0;
}
template <typename F> int32_t mutate_item(void* handle, F&& mutation) {
  std::lock_guard<std::mutex> lock(g_item_mutex);
  const auto found = g_items.find(key(handle));
  if (!handle || found == g_items.end()) { ++g_item_invalid; return 4; }
  mutation(found->second); return 0;
}
template <typename F> int32_t mutate_layer(void* handle, F&& mutation) {
  std::lock_guard<std::mutex> lock(g_layer_mutex);
  const auto found = g_layers.find(key(handle));
  if (!handle || found == g_layers.end()) { ++g_layer_invalid; return 4; }
  mutation(found->second); return 0;
}
}  // namespace

void configure_validators(ItemValidator item, LayerInitializer layer) noexcept {
  g_item_validator = item; g_layer_initializer = layer;
}
bool snapshot_item(void* handle, ItemValue& value) noexcept {
  std::lock_guard<std::mutex> lock(g_item_mutex);
  const auto found = g_items.find(key(handle));
  if (!handle || found == g_items.end()) return false;
  value = found->second; return true;
}
bool snapshot_layer(void* handle, LayerValue& value) noexcept {
  std::lock_guard<std::mutex> lock(g_layer_mutex);
  const auto found = g_layers.find(key(handle));
  if (!handle || found == g_layers.end()) return false;
  value = found->second; return true;
}
int32_t insert_layer_value(const LayerValue& value, void** output) {
  return insert_layer(value, output);
}
std::size_t item_live_count() noexcept { std::lock_guard<std::mutex> l(g_item_mutex); return g_items.size(); }
std::size_t layer_live_count() noexcept { std::lock_guard<std::mutex> l(g_layer_mutex); return g_layers.size(); }
uint32_t item_created_count() noexcept { return g_item_created.load(); }
uint32_t item_disposed_count() noexcept { return g_item_disposed.load(); }
uint32_t item_invalid_count() noexcept { return g_item_invalid.load(); }
uint32_t layer_created_count() noexcept { return g_layer_created.load(); }
uint32_t layer_disposed_count() noexcept { return g_layer_disposed.load(); }
uint32_t layer_invalid_count() noexcept { return g_layer_invalid.load(); }

int32_t __cdecl render_options_new_from_item(int32_t id, void* item, void** out) {
  if (out) *out = nullptr; if (!out || !g_item_validator || !g_item_validator(id, item)) { ++g_item_invalid; return 4; }
  ItemValue value{}; value.owner_plugin_id = id; value.item = item; return insert_item(value, out);
}
int32_t __cdecl render_options_duplicate(int32_t id, void* h, void** out) { if(out)*out=nullptr; ItemValue v{}; if(id!=1||!out||!snapshot_item(h,v)){++g_item_invalid;return 4;}v.owner_plugin_id=id;return insert_item(v,out); }
int32_t __cdecl render_options_dispose(void* h) { std::lock_guard<std::mutex> l(g_item_mutex);auto i=g_items.find(key(h));if(!h||i==g_items.end()){++g_item_invalid;return 4;}g_items.erase(i);++g_item_disposed;return 0; }
#define ITEM_TIME(Name, Field, Valid) \
 int32_t __cdecl render_options_set_##Name(void*h,AegpTime v){if(!(Valid)){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.Field=v;});} \
 int32_t __cdecl render_options_get_##Name(void*h,AegpTime*v){ItemValue o{};if(!v||!snapshot_item(h,o)){++g_item_invalid;return 4;}*v=o.Field;return 0;}
ITEM_TIME(time,time,v.scale!=0) ITEM_TIME(time_step,time_step,v.scale!=0&&v.value>0)
#undef ITEM_TIME
#define ITEM_I32(Name, Field, Valid) \
 int32_t __cdecl render_options_set_##Name(void*h,int32_t v){if(!(Valid)){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.Field=v;});} \
 int32_t __cdecl render_options_get_##Name(void*h,int32_t*v){ItemValue o{};if(!v||!snapshot_item(h,o)){++g_item_invalid;return 4;}*v=o.Field;return 0;}
ITEM_I32(field,field,v>=0&&v<=2) ITEM_I32(world_type,world_type,v>=1&&v<=3) ITEM_I32(matte,matte,v>=0&&v<=2)
#undef ITEM_I32
int32_t __cdecl render_options_set_downsample(void*h,int16_t x,int16_t y){if(x<=0||y<=0){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.downsample_x=x;o.downsample_y=y;});}
int32_t __cdecl render_options_get_downsample(void*h,int16_t*x,int16_t*y){ItemValue o{};if(!x||!y||!snapshot_item(h,o)){++g_item_invalid;return 4;}*x=o.downsample_x;*y=o.downsample_y;return 0;}
int32_t __cdecl render_options_set_roi(void*h,const AegpRect*v){if(!v){++g_item_invalid;return 4;}bool z=v->left==0&&v->top==0&&v->right==0&&v->bottom==0;if(!z&&(v->left>=v->right||v->top>=v->bottom)){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.roi=*v;});}
int32_t __cdecl render_options_get_roi(void*h,AegpRect*v){ItemValue o{};if(!v||!snapshot_item(h,o)){++g_item_invalid;return 4;}*v=o.roi;return 0;}
#define ITEM_I8(Name, Field) int32_t __cdecl render_options_set_##Name(void*h,int8_t v){if(v<0||v>1){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.Field=v;});} int32_t __cdecl render_options_get_##Name(void*h,int8_t*v){ItemValue o{};if(!v||!snapshot_item(h,o)){++g_item_invalid;return 4;}*v=o.Field;return 0;}
ITEM_I8(channel_order,channel_order) ITEM_I8(quality,render_quality)
#undef ITEM_I8
int32_t __cdecl render_options_set_guide_layers(void*h,uint8_t v){if(v>1){++g_item_invalid;return 4;}return mutate_item(h,[&](auto&o){o.render_guide_layers=v;});}
int32_t __cdecl render_options_get_guide_layers(void*h,uint8_t*v){ItemValue o{};if(!v||!snapshot_item(h,o)){++g_item_invalid;return 4;}*v=o.render_guide_layers;return 0;}

static int32_t new_layer(int32_t id,void* source,LayerEffectBoundary boundary,void**out){if(out)*out=nullptr;LayerValue v{};if(!out||!g_layer_initializer||!g_layer_initializer(id,source,boundary,&v)){++g_layer_invalid;return 4;}return insert_layer(v,out);}
int32_t __cdecl new_layer_render_options(int32_t id,void*s,void**o){return new_layer(id,s,LayerEffectBoundary::all,o);} int32_t __cdecl new_from_upstream_of_effect(int32_t id,void*s,void**o){return new_layer(id,s,LayerEffectBoundary::upstream,o);} int32_t __cdecl new_from_downstream_of_effect(int32_t id,void*s,void**o){return new_layer(id,s,LayerEffectBoundary::downstream,o);}
int32_t __cdecl duplicate_layer_render_options(int32_t id,void*h,void**out){if(out)*out=nullptr;LayerValue v{};if(id<=0||!out||!snapshot_layer(h,v)){++g_layer_invalid;return 4;}v.owner_plugin_id=id;return insert_layer(v,out);}
int32_t __cdecl dispose_layer_render_options(void*h){std::lock_guard<std::mutex>l(g_layer_mutex);auto i=g_layers.find(key(h));if(!h||i==g_layers.end()){++g_layer_invalid;return 4;}g_layers.erase(i);++g_layer_disposed;return 0;}
#define LAYER_TIME(Name, Field, Valid) int32_t __cdecl set_layer_render_##Name(void*h,AegpTime v){if(!(Valid)){++g_layer_invalid;return 4;}return mutate_layer(h,[&](auto&o){o.Field=v;});} int32_t __cdecl get_layer_render_##Name(void*h,AegpTime*v){LayerValue o{};if(!v||!snapshot_layer(h,o)){++g_layer_invalid;return 4;}*v=o.Field;return 0;}
LAYER_TIME(time,time,v.scale!=0) LAYER_TIME(time_step,time_step,v.scale!=0&&v.value>0)
#undef LAYER_TIME
#define LAYER_I32(Name, Field, Valid) int32_t __cdecl set_layer_render_##Name(void*h,int32_t v){if(!(Valid)){++g_layer_invalid;return 4;}return mutate_layer(h,[&](auto&o){o.Field=v;});} int32_t __cdecl get_layer_render_##Name(void*h,int32_t*v){LayerValue o{};if(!v||!snapshot_layer(h,o)){++g_layer_invalid;return 4;}*v=o.Field;return 0;}
LAYER_I32(world_type,world_type,v>=1&&v<=3) LAYER_I32(matte,matte,v>=0&&v<=2)
#undef LAYER_I32
int32_t __cdecl set_layer_render_downsample(void*h,int16_t x,int16_t y){if(x<=0||y<=0){++g_layer_invalid;return 4;}return mutate_layer(h,[&](auto&o){o.downsample_x=x;o.downsample_y=y;});}
int32_t __cdecl get_layer_render_downsample(void*h,int16_t*x,int16_t*y){LayerValue o{};if(!x||!y||!snapshot_layer(h,o)){++g_layer_invalid;return 4;}*x=o.downsample_x;*y=o.downsample_y;return 0;}
}  // namespace aexcompat::render_options
