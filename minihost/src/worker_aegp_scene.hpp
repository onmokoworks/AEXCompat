#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>

#include "worker_aegp_render_options.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_suite_abi.hpp"

struct AegpMatrix4 { double mat[4][4]{}; };
static_assert(sizeof(AegpMatrix4) == 16 * sizeof(double));

union AegpLegacyStreamVal { double one_d; };

struct AegpItemSuite {
  void* before_get_active_item[2]{};
  int32_t (__cdecl *get_active_item)(void**){};
  void* before_get_item_type[2]{};
  int32_t (__cdecl *get_item_type)(void*, int16_t*){};
  void* after_get_item_type[20]{};
};
static_assert(offsetof(AegpItemSuite, get_active_item) == 16);
static_assert(offsetof(AegpItemSuite, get_item_type) == 40);
static_assert(sizeof(AegpItemSuite) == 208);

struct AegpLegacyItemSuite6 {
  void* before_get_active_item[2]{};
  int32_t (__cdecl *get_active_item)(void**){};
  void* before_get_item_type[2]{};
  int32_t (__cdecl *get_item_type)(void*, int16_t*){};
  void* unsupported[20]{};
};
static_assert(offsetof(AegpLegacyItemSuite6, get_active_item) == 16);
static_assert(offsetof(AegpLegacyItemSuite6, get_item_type) == 40);
static_assert(sizeof(AegpLegacyItemSuite6) == 208);

struct AegpCollectionItem {
  int32_t type{};
  int32_t padding{};
  std::array<std::byte, 40> item{};
  void* stream{};
};
static_assert(sizeof(AegpCollectionItem) == 56);

struct AegpCollectionSuite {
  void* new_collection{};
  int32_t (__cdecl *dispose_collection)(void*){};
  int32_t (__cdecl *get_count)(void*, uint32_t*){};
  int32_t (__cdecl *get_by_index)(void*, uint32_t, AegpCollectionItem*){};
  void* push_back{};
  void* erase{};
};
static_assert(sizeof(AegpCollectionSuite) == 48);

struct AegpLayerTransferMode {
  int32_t mode{};
  int32_t flags{};
  int32_t track_matte{};
};
static_assert(sizeof(AegpLayerTransferMode) == 12);

struct SceneSuiteFactoryHooks {
  bool (__cdecl *render_scene_enabled)(){};
  void* comp_bg_color{};
  void* effect_param_union{};
  std::array<void*, 7> legacy_stream_callbacks{};
  std::array<void*, 22> keyframe_callbacks{};
};

// Private boundary for the clean-room scene worker. Callback addresses keep
// their exact __cdecl ABI; non-scene services are supplied explicitly.
struct SceneHostHooks {
  void (__cdecl *bump_project_timestamp)(){};
  bool (__cdecl *validate_render_options_item)(int32_t, void*){};
  bool (__cdecl *initialize_layer_render_options)(int32_t, void*, int32_t,
                                                  void*){};
  bool (__cdecl *suite_lease_balanced)(){};
  int32_t (*make_utf16_handle)(const std::u16string&, const char*, void**){};
  int32_t (__cdecl *free_mem_handle)(void*){};
  SceneSuiteFactoryHooks suite_factory{};
};

struct SceneContext {
  SceneHostHooks hooks{};
  void* composition_item{};
  void* composition{};
  void* pf_layer{};
  void* pf_effect{};
  int32_t* full_resolution_width{};
  int32_t* full_resolution_height{};
  int32_t (__cdecl *smart_width)(){};
  int32_t (__cdecl *smart_height)(){};
};

enum class SceneSuiteAcquireResult {
  not_handled,
  acquired,
  rejected,
};

bool configure_scene_context(const SceneContext& context) noexcept;
const SceneContext* scene_context() noexcept;
bool scene_translation_unit_linked() noexcept;
bool scene_selftests_translation_unit_linked() noexcept;
SceneSuiteAcquireResult scene_acquire_suite(
    const char* name, int32_t version, const void** suite) noexcept;

inline constexpr unsigned kAegpSceneEffectInstanceLimit = 8;
inline constexpr unsigned kAegpSceneEffectLeaseLimit = 16;
inline constexpr unsigned kAegpSceneLegacyEffectStreamLimit = 16;

using AegpTime = aexcompat::suite_abi::AegpTime;
using AegpLayerEffectBoundary = aexcompat::render_options::LayerEffectBoundary;
using AegpLayerRenderOptionsValue = aexcompat::render_options::LayerValue;
using namespace aexcompat::scene_runtime;

extern bool& g_aegp_effect_live;
extern std::array<AegpEffectInstance, kAegpEffectInstanceCapacity>& g_aegp_effect_instances;
extern std::array<AegpEffectLease, kAegpEffectLeaseCapacity>& g_aegp_effect_leases;
extern uint32_t& g_aegp_effect_lease_generation;
extern AegpTransformStream& g_aegp_transform_stream;
extern std::array<AegpLegacyEffectStream, kAegpLegacyEffectStreamCapacity>&
    g_aegp_legacy_effect_streams;
extern uint32_t& g_aegp_legacy_effect_stream_generation;
extern std::array<AegpTime, 3>& g_aegp_layer_in_points;
extern std::array<AegpTime, 3>& g_aegp_layer_durations;

extern AegpItemSuite g_aegp_item_suite;
extern AegpLegacyItemSuite6 g_aegp_legacy_item_suite6;
extern AegpCollectionSuite g_aegp_collection_suite;
extern std::array<void*, 41> g_aegp_comp_suite10;
extern std::array<void*, 28> g_aegp_comp_suite4;
extern std::array<void*, 44> g_aegp_comp_suite11;
extern std::array<void*, 44> g_aegp_comp_suite12;
extern std::array<void*, 46> g_aegp_layer_suite5;
extern std::array<void*, 50> g_aegp_layer_suite8;
extern std::array<void*, 53> g_aegp_layer_suite9;
extern std::array<void*, 17> g_aegp_effect_suite2;
extern std::array<void*, 17> g_aegp_effect_suite3;
extern std::array<void*, 22> g_aegp_effect_suite4;
extern std::array<void*, 22> g_aegp_stream_suite2;
extern std::array<void*, 23> g_aegp_stream_suite6;
extern std::array<void*, 22> g_aegp_keyframe_suite5;

const AegpEffectInstance* resolve_effect_instance(
    void* effect, int32_t owner = 0, std::size_t* index = nullptr);
bool any_effect_lease_live();
bool layer_effect_boundary_is_live(const AegpLayerRenderOptionsValue& options);
const AegpInstalledEffectRecord* find_installed_effect(int32_t key);
const AegpEffectParameterRecord* find_effect_parameter(int32_t key, int32_t index);
int32_t aegp_layer_index(void* layer);
int32_t __cdecl aegp_get_active_item(void** item);
int32_t __cdecl aegp_get_item_type(void* item, int16_t* item_type);
int32_t __cdecl aegp_get_item_current_time(void* item, AegpTime* time);
int32_t __cdecl aegp_set_item_current_time(void* item, const AegpTime* time);
int32_t __cdecl aegp_get_item_id(void* item, int32_t* id);
int32_t __cdecl aegp_get_item_name(int32_t plugin_id, void* item, void** name);
int32_t __cdecl aegp_get_item_duration(void* item, AegpTime* duration);
int32_t __cdecl aegp_get_comp_from_item(void* item, void** comp);
int32_t __cdecl aegp_get_comp_framerate(void* comp, double* fps);
int32_t __cdecl aegp_get_comp_frame_duration(void* comp, AegpTime* duration);
int32_t __cdecl aegp_get_comp_num_layers(void* comp, int32_t* count);
int32_t __cdecl aegp_get_comp_layer_by_index(void* comp, int32_t index, void** layer);
int32_t __cdecl aegp_get_layer_to_world_xform(
    void* layer, const AegpTime* comp_time, AegpMatrix4* transform);
int32_t __cdecl aegp_get_item_from_comp(void* comp, void** item);
int32_t __cdecl aegp_get_item_dimensions(void* item, int32_t* width, int32_t* height);
int32_t __cdecl aegp_get_layer_stream_value_v2(void*, int32_t, int16_t,
    const AegpTime*, uint8_t, AegpLegacyStreamVal*, int32_t*);
int32_t __cdecl aegp_get_active_layer(void** layer);
int32_t __cdecl aegp_get_layer_index(void* layer, int32_t* index);
int32_t __cdecl aegp_get_layer_source_item(void* layer, void** item);
int32_t __cdecl aegp_get_layer_parent_comp(void* layer, void** comp);
int32_t __cdecl aegp_get_layer_name(int32_t, void*, void**, void**);
int32_t __cdecl aegp_get_layer_parent(void* layer, void** parent);
int32_t __cdecl aegp_get_layer_from_id(void* comp, int32_t id, void** layer);
int32_t __cdecl aegp_get_comp_selection(int32_t, void*, void**);
int32_t __cdecl aegp_dispose_collection(void* collection);
int32_t __cdecl aegp_get_collection_count(void* collection, uint32_t* count);
int32_t __cdecl aegp_get_collection_item(void*, uint32_t, AegpCollectionItem*);
int32_t __cdecl aegp_get_layer_id(void* layer, int32_t* id);
int32_t __cdecl aegp_get_layer_flags(void* layer, uint32_t* flags);
int32_t __cdecl aegp_set_layer_flag(void* layer, uint32_t flag, uint8_t value);
int32_t __cdecl aegp_get_layer_transfer_mode(void*, AegpLayerTransferMode*);
int32_t __cdecl aegp_get_layer_object_type(void* layer, int32_t* type);
int32_t __cdecl aegp_get_layer_in_point(void*, int32_t, AegpTime*);
int32_t __cdecl aegp_get_layer_duration(void*, int32_t, AegpTime*);
int32_t __cdecl aegp_set_layer_in_point_and_duration(
    void*, int32_t, const AegpTime*, const AegpTime*);
int32_t __cdecl aegp_get_layer_num_effects(void*, int32_t*);
int32_t __cdecl aegp_get_layer_effect_by_index(int32_t, void*, int32_t, void**);
int32_t __cdecl aegp_get_installed_key_from_layer_effect(void*, int32_t*);
int32_t __cdecl aegp_get_effect_flags(void*, uint32_t*);
int32_t __cdecl aegp_set_effect_flags(void*, uint32_t, uint32_t);
int32_t __cdecl aegp_reorder_effect(void*, int32_t);
int32_t __cdecl aegp_dispose_effect(void*);
int32_t __cdecl aegp_apply_effect(int32_t, void*, int32_t, void**);
int32_t __cdecl aegp_delete_layer_effect(void*);
int32_t __cdecl aegp_duplicate_effect(void*, void**);
int32_t __cdecl get_new_effect_for_effect(int32_t, void*, void**);
int32_t __cdecl aegp_get_num_installed_effects(int32_t*);
int32_t __cdecl aegp_get_next_installed_effect(int32_t, int32_t*);
int32_t __cdecl aegp_get_effect_name(int32_t, char*);
int32_t __cdecl aegp_get_effect_match_name(int32_t, char*);
int32_t __cdecl aegp_get_effect_category(int32_t, char*);
int32_t __cdecl aegp_get_effect_num_param_streams_v2(void*, int32_t*);
int32_t __cdecl aegp_get_new_layer_stream(int32_t, void*, int32_t, void**);
int32_t __cdecl aegp_get_new_effect_stream_by_index(int32_t, void*, int32_t, void**);
int32_t __cdecl aegp_get_effect_num_param_streams_v6(void*, int32_t*);
int32_t __cdecl aegp_get_stream_type(void*, int32_t*);
int32_t __cdecl aegp_get_stream_num_keyframes(void*, int32_t*);
int32_t __cdecl aegp_get_keyframe_time(void*, int32_t, int32_t, AegpTime*);
int32_t __cdecl aegp_get_new_keyframe_value(int32_t, void*, int32_t, AegpStreamValue*);
int32_t __cdecl aegp_get_keyframe_interpolation(void*, int32_t, int32_t*, int32_t*);
int32_t __cdecl aegp_get_new_stream_value(
    int32_t, void*, int32_t, const AegpTime*, uint8_t, AegpStreamValue*);
int32_t __cdecl aegp_get_stream_name(int32_t, void*, uint8_t, void**);
int32_t __cdecl aegp_set_effect_stream_value(int32_t, void*, AegpStreamValue*);
int32_t __cdecl aegp_dispose_stream_value(AegpStreamValue*);
int32_t __cdecl aegp_dispose_stream(void*);
