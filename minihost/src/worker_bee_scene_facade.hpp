#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

// BEE.dll-compatible scene objects behind the effect's own AEGP layer handle
// (issue #1210).
//
// Adobe-bundled effects such as Timecode.aex import BEE.dll (AE's scene
// engine) directly and dereference the `AEGP_LayerH` they get from
// `AEGP PF Interface Suite::AEGP_GetEffectLayer` as a `BEE_AVLayer*`: they read
// its fields, call through its vtable, and hand it to BEE.dll exports that read
// the layer, its parent comp item, that item's project, and its source item.
// A tag-only opaque handle makes those reads land in unrelated host memory,
// so the handle handed out for the effect's layer is instead a real object with
// the observed BEE layout: the fields those code paths read carry host values,
// the vtable slots they call are implemented, and every other slot is an
// identifying trap that records the slot and fails the frame explicitly.
//
// Layouts, vtable slot names and the values AE 2026 (26.3, BEE.dll 24.7 MB)
// carries were recovered from BEE.dll's exported symbols and a Frida capture of
// Timecode's RENDER in AE (docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md). Only the
// fields and slots that observation reached are given meaning here; the rest
// of each object is zero and every unobserved vtable slot traps.
//
// This is a host capability, not a per-effect stub: any AEX that reads the
// effect layer handle as a BEE object gets the same facade. Layer handles the
// host issues through the scene registry (borrowed handles, comp layer
// enumeration) are unchanged; extending the facade to those is a separate
// step (issue #1264).

namespace aexcompat::worker_runtime::bee_facade {

// T_Time (U.dll): value / scale.
struct Time {
  int32_t value{};
  uint32_t scale{1};
};
static_assert(sizeof(Time) == 8);

// BEE_TimeDisplayFormat as `BEE_Project::GetTimeDisplay` returns it (16
// bytes at project+0x84) and as `BEE_TimeDisplayFormat2T_TimeFormatInfo`
// consumes it. Bytes 0..3 and 12 are flags/enums whose individual meaning is
// unverified; `frames_per_second` is copied into T_TimeFormatInfo; the field
// named `frames_per_foot` here (value 16 observed) is only consumed when byte3
// is nonzero, which was not observed, so the name is an inference.
struct TimeDisplayFormat {
  uint8_t byte0{};
  uint8_t byte1{};
  uint8_t byte2{};
  uint8_t byte3{};
  int32_t frames_per_second{};
  int32_t frames_per_foot{};
  uint8_t byte12{};
  uint8_t reserved[3]{};
};
static_assert(sizeof(TimeDisplayFormat) == 16);

inline constexpr std::size_t kLayerVtableSlots = 246;    // BEE_AVLayer
inline constexpr std::size_t kItemVtableSlots = 15;      // BEE_CompItem / BEE_FootageItem
inline constexpr std::size_t kProjectVtableSlots = 8;    // BEE_Project
inline constexpr uint16_t kItemTag = 0xBEE1;             // BEE_Item + 0x08
inline constexpr int16_t kItemTypeComposition = 4;       // BEE_Item::GetType
inline constexpr int16_t kItemTypeFootage = 7;
// BEE_Item::GetFlags bits observed on the comp item (0x20) and on a still
// footage item (0x30). Timecode skips the source-time-format lookup when the
// source item carries 0x10, which is what AE did for the still-footage layer.
inline constexpr uint32_t kCompItemFlags = 0x20;
inline constexpr uint32_t kStillFootageItemFlags = 0x30;

// Observed vtable slots.
inline constexpr std::size_t kLayerSlotCanStore = 56;              // TDB_StreamGroup::CanStore(const TDB_MatchName&) const
inline constexpr std::size_t kLayerSlotGetStream = 65;             // TDB_NamedStreamGroup::GetStream(const TDB_MatchName&, IfMissing) const
inline constexpr std::size_t kLayerSlotIsLayerType = 79;           // BEE_AVLayer::IsLayerType(int) const
inline constexpr std::size_t kLayerSlotGetSourceItem = 183;        // BEE_AVLayer::pGetSourceItem(const TDB_ParamBag*)
inline constexpr std::size_t kLayerSlotGetConstSourceItem = 184;   // BEE_AVLayer::pGetConstSourceItem(const TDB_ParamBag*) const

// Every unobserved slot raises this code plus the slot index (SEH, contained
// by the selector dispatch), after recording the slot in the report's
// unsupported_suite_calls list under the object's pseudo-suite name and
// writing a `stage:bee_facade_trap` line that names the caller module. The
// base sits above the suite-call-slot probe's code space
// (worker_suite_call_slot_probe.hpp; asserted in the .cpp).
inline constexpr uint32_t kTrapExceptionBase = 0xE0428000u;
inline constexpr uint32_t kTrapExceptionLayerRange = 0x000u;    // + slot (0..245)
inline constexpr uint32_t kTrapExceptionItemRange = 0x100u;     // comp item, + slot (0..14)
inline constexpr uint32_t kTrapExceptionFootageRange = 0x140u;  // footage item, + slot (0..14)
inline constexpr uint32_t kTrapExceptionProjectRange = 0x180u;  // + slot (0..7)

struct ProjectObject;
struct ItemObject;

// BEE_Project: the time display block (BEE_GetProjectSettings through
// BEE_Project::GetTimeDisplay) and the project colour settings
// (`BEE_CompItem::GetColorSettings` copies the `shared_ptr<PF_ColorSettings>`
// at project+0xe8 / +0xf0 out of `BEE_Item::GetParentProject()`; ShapeBlur
// (Camera Lens Blur) reads it through the effect layer's parent comp, issue
// #1264). The host publishes no colour settings, so both words stay null and
// the copy is an empty shared_ptr, which the observed caller handles (it takes
// its no-linear-blending-tables path).
struct alignas(16) ProjectObject {
  const void* const* vtable{};                 // 0x000
  std::byte reserved_008[0x84 - 0x08]{};
  TimeDisplayFormat time_display{};            // 0x084 BEE_Project::GetTimeDisplay
  std::byte reserved_094[0x0e8 - 0x94]{};
  void* color_settings{};                      // 0x0e8 shared_ptr<PF_ColorSettings>::ptr
  void* color_settings_control{};              // 0x0f0 shared_ptr control block (refcount at +8)
  std::byte reserved_0f8[0x100 - 0xf8]{};
};
static_assert(offsetof(ProjectObject, time_display) == 0x84);
static_assert(offsetof(ProjectObject, color_settings) == 0xe8);
static_assert(offsetof(ProjectObject, color_settings_control) == 0xf0);
static_assert(sizeof(ProjectObject) == 0x100);

// BEE_Item with the BEE_CompItem fields BEE_GetCompSettings reads. The same
// object type serves the footage item (type 7); its comp fields are
// zero-initialised and not maintained by the hand-out.
struct alignas(16) ItemObject {
  const void* const* vtable{};                 // 0x000
  uint16_t tag{};                              // 0x008 == 0xBEE1 (BEE.dll item check)
  std::byte reserved_00a[0x38 - 0x0a]{};
  ProjectObject* parent_project{};             // 0x038 BEE_Item::GetParentProject
  std::byte reserved_040[0x48 - 0x40]{};
  int16_t type{};                              // 0x048 BEE_Item::GetType
  uint16_t reserved_04a{};
  uint32_t flags{};                            // 0x04c BEE_Item::GetFlags (std::atomic<int>)
  std::byte reserved_050[0x298 - 0x50]{};
  Time duration{};                             // 0x298 BEE_CompSettings[1]
  Time display_start{};                        // 0x2a0 BEE_CompSettings[2] (BEE_SourceMediaInfo start)
  std::byte reserved_2a8[0x2b8 - 0x2a8]{};
  int16_t width{};                             // 0x2b8 BEE_CompSettings +0x18
  int16_t height{};                            // 0x2ba BEE_CompSettings +0x1a
  int32_t frame_rate_fixed{};                  // 0x2bc 16.16 fps (BEE_CompSettings +0x1c, source fps getter)
  std::byte reserved_2c0[0x418 - 0x2c0]{};
  int32_t max_2d_motion_blur_samples{};        // 0x418 BEE_CompItem::GetMax2DMotionBlurSamples
  int32_t std_motion_blur_samples{};           // 0x41c BEE_CompItem::GetStdMotionBlurSamples
  std::byte reserved_420[0x424 - 0x420]{};
  uint8_t display_dropframe{};                 // 0x424 BEE_CompItem::GetDisplayDropframe
  std::byte reserved_425[0x430 - 0x425]{};
};
static_assert(offsetof(ItemObject, tag) == 0x08);
static_assert(offsetof(ItemObject, parent_project) == 0x38);
static_assert(offsetof(ItemObject, type) == 0x48);
static_assert(offsetof(ItemObject, flags) == 0x4c);
static_assert(offsetof(ItemObject, duration) == 0x298);
static_assert(offsetof(ItemObject, display_start) == 0x2a0);
static_assert(offsetof(ItemObject, width) == 0x2b8);
static_assert(offsetof(ItemObject, height) == 0x2ba);
static_assert(offsetof(ItemObject, frame_rate_fixed) == 0x2bc);
static_assert(offsetof(ItemObject, max_2d_motion_blur_samples) == 0x418);
static_assert(offsetof(ItemObject, std_motion_blur_samples) == 0x41c);
static_assert(offsetof(ItemObject, display_dropframe) == 0x424);
static_assert(sizeof(ItemObject) == 0x430);

// BEE_AVLayer: the parent comp item at +0x260 (read directly by Timecode and
// by BEE_GetSourceTimeFormat) and the source item at +0x2720
// (BEE_AVLayer::pGetConstSourceItem reads this[0x4e4]).
struct alignas(16) LayerObject {
  const void* const* vtable{};                 // 0x0000
  std::byte reserved_0008[0x260 - 0x08]{};
  ItemObject* parent_comp_item{};              // 0x0260
  std::byte reserved_0268[0x2720 - 0x268]{};
  ItemObject* source_item{};                   // 0x2720
  std::byte reserved_2728[0x2740 - 0x2728]{};
};
static_assert(offsetof(LayerObject, parent_comp_item) == 0x260);
static_assert(offsetof(LayerObject, source_item) == 0x2720);
static_assert(sizeof(LayerObject) == 0x2740);

// Values the facade publishes; taken from the host's scene contract (30 fps,
// 300 frames, comp dimensions from the render context) at hand-out time.
struct SceneValues {
  Time comp_duration{300, 30};
  Time comp_display_start{0, 1};
  int32_t frames_per_second{30};
  int32_t comp_width{};
  int32_t comp_height{};
  bool display_dropframe{};
};

// Installs the vtables and object graph behind `layer` (idempotent) and
// publishes the values. Called every time the host hands the effect layer
// handle out: when the graph already carries exactly these values nothing is
// written, otherwise (a first hand-out, a render context change, a published
// field a plug-in overwrote) the graph is rewritten to the contract. Reserved
// bytes are not re-zeroed.
void prepare_effect_layer(LayerObject& layer, const SceneValues& values) noexcept;

// The objects `prepare_effect_layer` links to `layer`.
const ItemObject& comp_item() noexcept;
// The comp item as a handle: what AEGP_GetLayerParentComp answers for the
// effect layer (worker_aegp_scene), so a caller that reads its AEGP_CompH as
// a `BEE_CompItem*` (ShapeBlur, issue #1264) finds the same object the layer's
// +0x260 names. Only meaningful after a prepare_effect_layer.
void* comp_item_handle() noexcept;
const ItemObject& footage_item() noexcept;
const ProjectObject& project() noexcept;
const void* const* layer_vtable() noexcept;

// Number of trap slots taken so far (any of the objects). The slot identities
// themselves are in the report's unsupported_suite_calls list.
uint32_t trap_count() noexcept;
// Calls the observed layer vtable slots have taken (by slot index).
uint32_t observed_call_count(std::size_t layer_slot) noexcept;
// The `<module>+0x<rva>` classification the last trap wrote (empty before the
// first trap); for the self-test.
std::string last_trap_caller();

bool selftest();

}  // namespace aexcompat::worker_runtime::bee_facade
