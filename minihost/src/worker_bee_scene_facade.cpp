#include "worker_bee_scene_facade.hpp"

#include "worker_suite_call_slot_probe.hpp"
#include "worker_suite_registry.hpp"

#include <windows.h>
#include <intrin.h>

#include <array>
#include <atomic>
#include <iostream>
#include <iterator>
#include <sstream>
#include <string>
#include <string_view>
#include <utility>

namespace aexcompat::worker_runtime::bee_facade {
namespace {

std::atomic<uint32_t> g_trap_count{};
// The last trap's caller: the raw return address is kept only for the
// self-test's frame check and is never written out; the classified string is
// what the stage line carried.
std::atomic<uintptr_t> g_last_trap_return_address{};
SRWLOCK g_last_trap_caller_lock = SRWLOCK_INIT;
std::string g_last_trap_caller;

static_assert(kTrapExceptionBase >=
                  suite_call_slot_probe::kProbeExceptionBase +
                      suite_call_slot_probe::kMaxProbeTargets *
                          suite_call_slot_probe::kProbeExceptionTargetStride,
              "facade trap codes must not overlap the suite call slot probe");

// Module basename plus RVA of a return address, or empty when it lies in no
// module. Only this classification is written out (no raw address), the same
// rule the selector SEH trace follows.
std::string classify_caller(uintptr_t return_address) {
  HMODULE module{};
  if (!return_address ||
      !GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                              GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                          reinterpret_cast<LPCWSTR>(return_address), &module) ||
      !module)
    return {};
  wchar_t path[MAX_PATH]{};
  const DWORD length = GetModuleFileNameW(module, path, MAX_PATH);
  if (length == 0 || length >= MAX_PATH) return {};
  std::wstring_view full(path, length);
  const std::size_t slash = full.find_last_of(L"\\/");
  std::wstring_view base = slash == std::wstring_view::npos ? full : full.substr(slash + 1);
  std::string result;
  for (const wchar_t ch : base) {
    // ASCII printable only, and never the separators of the stage line.
    if (ch < 0x20 || ch > 0x7e || ch == L' ' || ch == L'=') result.push_back('_');
    else result.push_back(static_cast<char>(ch));
  }
  std::ostringstream text;
  text << result << "+0x" << std::hex
       << (return_address - reinterpret_cast<uintptr_t>(module));
  return text.str();
}
// Per observed slot call counters (indexed by layer vtable slot). Besides
// being observable, they keep the two answer-nothing slots (CanStore,
// GetStream) from being folded into one function by identical-code folding,
// which would make the vtable entries indistinguishable in a diagnostic.
std::array<std::atomic<uint32_t>, kLayerVtableSlots> g_observed_calls{};

// A slot BEE.dll or a plug-in reached that no observation covered. Signature-
// agnostic on purpose: whatever the caller passed is ignored, the slot is
// recorded in the report's unsupported_suite_calls list under the object's
// pseudo-suite name, and the call fails the frame through the selector SEH
// containment with a code that names the slot. Returning a guessed value here
// would let BEE.dll continue on a wrong answer and produce a frame that looks
// rendered.
[[noreturn]] void trap(UnsupportedSuiteId object, const char* object_name,
                       uint32_t range, uint32_t slot,
                       uintptr_t return_address) {
  ++g_trap_count;
  (void)record_unsupported_suite_call(object, slot);
  // The caller is what identifies the path (a BEE.dll export, or the plug-in
  // itself); the SEH trace only classifies stack values for access
  // violations, so it is written here.
  std::string caller;
  try {
    caller = classify_caller(return_address);
  } catch (...) {
    caller.clear();
  }
  if (caller.empty()) caller = "unknown";
  try {
    std::cerr << "stage:bee_facade_trap object=" << object_name
              << " slot=" << slot << " caller=" << caller << "\n"
              << std::flush;
  } catch (...) {
  }
  {
    AcquireSRWLockExclusive(&g_last_trap_caller_lock);
    struct ReleaseExclusive {
      SRWLOCK* lock;
      ~ReleaseExclusive() { ReleaseSRWLockExclusive(lock); }
    } release{&g_last_trap_caller_lock};
    g_last_trap_caller.swap(caller);  // noexcept; the pair below stays consistent
    g_last_trap_return_address.store(return_address);
  }
  const ULONG_PTR arguments[] = {static_cast<ULONG_PTR>(object),
                                 static_cast<ULONG_PTR>(slot)};
  RaiseException(kTrapExceptionBase + range + slot, EXCEPTION_NONCONTINUABLE,
                 static_cast<DWORD>(std::size(arguments)), arguments);
  // RaiseException does not return for a non-continuable exception. If it
  // ever did, fail loudly rather than hang a deadline-less route.
  __fastfail(FAST_FAIL_FATAL_APP_EXIT);
}

// `_ReturnAddress()` is evaluated in the trap function itself (the function
// the vtable points at), not in a helper: a helper's return address would be
// the trap, and whether a helper gets inlined is an optimizer decision.
template <std::size_t Slot>
void* __cdecl layer_trap() {
  trap(UnsupportedSuiteId::bee_av_layer_vtable, "BEE_AVLayer",
       kTrapExceptionLayerRange, static_cast<uint32_t>(Slot),
       reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
template <std::size_t Slot>
void* __cdecl item_trap() {
  trap(UnsupportedSuiteId::bee_item_vtable, "BEE_CompItem",
       kTrapExceptionItemRange, static_cast<uint32_t>(Slot),
       reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
template <std::size_t Slot>
void* __cdecl footage_trap() {
  trap(UnsupportedSuiteId::bee_footage_item_vtable, "BEE_FootageItem",
       kTrapExceptionFootageRange, static_cast<uint32_t>(Slot),
       reinterpret_cast<uintptr_t>(_ReturnAddress()));
}
template <std::size_t Slot>
void* __cdecl project_trap() {
  trap(UnsupportedSuiteId::bee_project_vtable, "BEE_Project",
       kTrapExceptionProjectRange, static_cast<uint32_t>(Slot),
       reinterpret_cast<uintptr_t>(_ReturnAddress()));
}

// Observed BEE_AVLayer virtuals. x64 has one calling convention, so `this`
// is the first argument of a plain function.

// BEE_AVLayer::IsLayerType(int) const -> `type == 0` in BEE.dll (0 = AV
// layer). BEE_LayerToSourceTime asks this before looking for a time-remap
// stream.
uint8_t __cdecl is_layer_type(const void* /*self*/, int32_t type) {
  ++g_observed_calls[kLayerSlotIsLayerType];
  return type == 0 ? 1 : 0;
}

// TDB_StreamGroup::CanStore(const TDB_MatchName&) const. The host layer holds
// no dynamic streams (BEE_LayerToSourceTime asks for TIME_REMAP), so the
// answer is "cannot", which makes BEE_LayerToSourceTime return the input time
// unchanged: the same result AE gave for a layer whose time-remap stream has
// no keys.
uint8_t __cdecl can_store(const void* /*self*/, const void* /*match_name*/) {
  ++g_observed_calls[kLayerSlotCanStore];
  return 0;
}

// TDB_NamedStreamGroup::GetStream(const TDB_MatchName&, IfMissing) const.
// Only reachable after CanStore said yes; a caller that asks anyway gets no
// stream.
const void* __cdecl get_stream(const void* /*self*/, const void* /*match_name*/,
                               int32_t /*if_missing*/) {
  ++g_observed_calls[kLayerSlotGetStream];
  return nullptr;
}

// BEE_AVLayer::pGetSourceItem / pGetConstSourceItem(const TDB_ParamBag*).
// BEE.dll's own implementation returns this[0x4e4] (layer+0x2720) when no
// parameter bag is passed; the same field is what these return.
ItemObject* __cdecl get_source_item(LayerObject* self, const void* /*bag*/) {
  ++g_observed_calls[kLayerSlotGetSourceItem];
  return self ? self->source_item : nullptr;
}
const ItemObject* __cdecl get_const_source_item(const LayerObject* self,
                                                const void* /*bag*/) {
  ++g_observed_calls[kLayerSlotGetConstSourceItem];
  return self ? self->source_item : nullptr;
}

template <std::size_t... Slots>
std::array<const void*, sizeof...(Slots)> make_layer_vtable(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<const void*>(&layer_trap<Slots>)...}};
}
template <std::size_t... Slots>
std::array<const void*, sizeof...(Slots)> make_item_vtable(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<const void*>(&item_trap<Slots>)...}};
}
template <std::size_t... Slots>
std::array<const void*, sizeof...(Slots)> make_footage_vtable(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<const void*>(&footage_trap<Slots>)...}};
}
template <std::size_t... Slots>
std::array<const void*, sizeof...(Slots)> make_project_vtable(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<const void*>(&project_trap<Slots>)...}};
}

struct Tables {
  std::array<const void*, kLayerVtableSlots> layer;
  std::array<const void*, kItemVtableSlots> item;
  std::array<const void*, kItemVtableSlots> footage;
  std::array<const void*, kProjectVtableSlots> project;
  Tables()
      : layer(make_layer_vtable(std::make_index_sequence<kLayerVtableSlots>{})),
        item(make_item_vtable(std::make_index_sequence<kItemVtableSlots>{})),
        footage(make_footage_vtable(std::make_index_sequence<kItemVtableSlots>{})),
        project(make_project_vtable(
            std::make_index_sequence<kProjectVtableSlots>{})) {
    layer[kLayerSlotCanStore] = reinterpret_cast<const void*>(&can_store);
    layer[kLayerSlotGetStream] = reinterpret_cast<const void*>(&get_stream);
    layer[kLayerSlotIsLayerType] =
        reinterpret_cast<const void*>(&is_layer_type);
    layer[kLayerSlotGetSourceItem] =
        reinterpret_cast<const void*>(&get_source_item);
    layer[kLayerSlotGetConstSourceItem] =
        reinterpret_cast<const void*>(&get_const_source_item);
  }
};

const Tables& tables() {
  static const Tables value;
  return value;
}

struct Objects {
  ProjectObject project{};
  ItemObject comp_item{};
  ItemObject footage_item{};
};

Objects& objects() {
  static Objects value;
  return value;
}

int32_t clamp_dimension(int32_t value) noexcept {
  if (value <= 0) return 0;
  return value > INT16_MAX ? INT16_MAX : value;
}

}  // namespace

void prepare_effect_layer(LayerObject& layer, const SceneValues& values) noexcept {
  const Tables& vtables = tables();
  Objects& graph = objects();
  // Hand-outs from concurrent smart-render threads must not tear each other's
  // writes; an SRW lock cannot throw, which a std::mutex under noexcept could.
  // Readers are not synchronised. A hand-out that repeats the values already
  // published writes nothing, so the steady state within a session is
  // read-only; a hand-out that changes them (a render context change between
  // frames of an interactive session) is the one window in which a plug-in
  // reading on another thread can observe a torn width/height.
  static SRWLOCK lock = SRWLOCK_INIT;
  AcquireSRWLockExclusive(&lock);
  struct Release {
    SRWLOCK* lock;
    ~Release() { ReleaseSRWLockExclusive(lock); }
  } release{&lock};
  // One validated frame rate feeds both the comp item (16.16) and the project
  // time display (integer).
  const int32_t fps =
      values.frames_per_second > 0 && values.frames_per_second < (1 << 15)
          ? values.frames_per_second
          : 0;
  const auto width = static_cast<int16_t>(clamp_dimension(values.comp_width));
  const auto height = static_cast<int16_t>(clamp_dimension(values.comp_height));
  // Keyed on the objects' own bytes, not on a shadow copy: a hand-out whose
  // contract is already what the graph carries writes nothing, and a
  // published field a plug-in overwrote is restored at the next hand-out
  // (reserved bytes are not compared or re-zeroed).
  {
    const ProjectObject& project = graph.project;
    const ItemObject& comp = graph.comp_item;
    const ItemObject& footage = graph.footage_item;
    if (layer.vtable == vtables.layer.data() &&
        layer.parent_comp_item == &graph.comp_item &&
        layer.source_item == &graph.footage_item &&
        project.vtable == vtables.project.data() &&
        project.time_display.byte0 == 1 && project.time_display.byte1 == 1 &&
        project.time_display.byte2 == 0 && project.time_display.byte3 == 0 &&
        project.time_display.frames_per_second == fps &&
        project.time_display.frames_per_foot == 16 &&
        project.time_display.byte12 == 2 &&
        comp.vtable == vtables.item.data() && comp.tag == kItemTag &&
        comp.parent_project == &graph.project &&
        comp.type == kItemTypeComposition && comp.flags == kCompItemFlags &&
        comp.duration.value == values.comp_duration.value &&
        comp.duration.scale == values.comp_duration.scale &&
        comp.display_start.value == values.comp_display_start.value &&
        comp.display_start.scale == values.comp_display_start.scale &&
        comp.width == width && comp.height == height &&
        comp.frame_rate_fixed == (fps << 16) &&
        comp.max_2d_motion_blur_samples == 128 &&
        comp.std_motion_blur_samples == 16 &&
        comp.display_dropframe == (values.display_dropframe ? 1 : 0) &&
        footage.vtable == vtables.footage.data() && footage.tag == kItemTag &&
        footage.parent_project == &graph.project &&
        footage.type == kItemTypeFootage &&
        footage.flags == kStillFootageItemFlags)
      return;
  }

  ProjectObject& project = graph.project;
  project.vtable = vtables.project.data();
  // AE 2026's fresh-project time display block (observed: 01 01 00 00,
  // fps 30, 16 frames per foot, 02). Only frames_per_second follows the host
  // contract; the flag bytes are carried as observed and their meaning is not
  // claimed.
  project.time_display.byte0 = 1;
  project.time_display.byte1 = 1;
  project.time_display.byte2 = 0;
  project.time_display.byte3 = 0;
  project.time_display.frames_per_second = fps;
  project.time_display.frames_per_foot = 16;
  project.time_display.byte12 = 2;

  ItemObject& comp = graph.comp_item;
  comp.vtable = vtables.item.data();
  comp.tag = kItemTag;
  comp.parent_project = &project;
  comp.type = kItemTypeComposition;
  comp.flags = kCompItemFlags;
  comp.duration = values.comp_duration;
  comp.display_start = values.comp_display_start;
  comp.width = width;
  comp.height = height;
  comp.frame_rate_fixed = fps << 16;
  // AE defaults observed on the capture comp; motion blur samples are not
  // part of the host contract and only travel through BEE_CompSettings.
  comp.max_2d_motion_blur_samples = 128;
  comp.std_motion_blur_samples = 16;
  comp.display_dropframe = values.display_dropframe ? 1 : 0;

  ItemObject& footage = graph.footage_item;
  footage.vtable = vtables.footage.data();
  footage.tag = kItemTag;
  footage.parent_project = &project;
  footage.type = kItemTypeFootage;
  footage.flags = kStillFootageItemFlags;

  layer.vtable = vtables.layer.data();
  layer.parent_comp_item = &comp;
  layer.source_item = &footage;
}

const ItemObject& comp_item() noexcept { return objects().comp_item; }
const ItemObject& footage_item() noexcept { return objects().footage_item; }
const ProjectObject& project() noexcept { return objects().project; }
const void* const* layer_vtable() noexcept { return tables().layer.data(); }
uint32_t trap_count() noexcept { return g_trap_count.load(); }
std::string last_trap_caller() {
  AcquireSRWLockShared(&g_last_trap_caller_lock);
  struct ReleaseShared {
    SRWLOCK* lock;
    ~ReleaseShared() { ReleaseSRWLockShared(lock); }
  } release{&g_last_trap_caller_lock};
  return g_last_trap_caller;
}
uint32_t observed_call_count(std::size_t layer_slot) noexcept {
  return layer_slot < kLayerVtableSlots ? g_observed_calls[layer_slot].load() : 0;
}

namespace {

// Calls a trap slot the way a caller with an unknown signature would and
// reports the SEH code it raised (0 when nothing was raised).
// A PC inside the calling function, taken the same way the trap takes its
// caller (a return address into that function): the frame check below
// compares unwind entries of two PCs, which stays right under ILT thunks and
// inlining because both PCs move together.
__declspec(noinline) uintptr_t pc_in_caller() {
  return reinterpret_cast<uintptr_t>(_ReturnAddress());
}

__declspec(noinline) uint32_t call_slot_expecting_trap(
    const void* const* vtable, std::size_t slot, uintptr_t* caller_pc) {
  using Slot = void* (__cdecl*)();
  if (caller_pc) *caller_pc = pc_in_caller();
  const auto function = reinterpret_cast<Slot>(vtable[slot]);
  __try {
    (void)function();
    return 0;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return GetExceptionCode();
  }
}

bool same_function(uintptr_t first, uintptr_t second) {
  DWORD64 first_base = 0, second_base = 0;
  const RUNTIME_FUNCTION* first_entry =
      RtlLookupFunctionEntry(static_cast<DWORD64>(first), &first_base, nullptr);
  const RUNTIME_FUNCTION* second_entry =
      RtlLookupFunctionEntry(static_cast<DWORD64>(second), &second_base, nullptr);
  return first_entry && second_entry && first_base == second_base &&
      first_entry->BeginAddress == second_entry->BeginAddress;
}

bool distinct_entries(const void* const* vtable, std::size_t count) {
  for (std::size_t slot = 0; slot < count; ++slot) {
    if (!vtable[slot]) return false;
    for (std::size_t earlier = 0; earlier < slot; ++earlier)
      if (vtable[earlier] == vtable[slot]) return false;
  }
  return true;
}

}  // namespace

bool selftest() {
  static LayerObject layer{};
  SceneValues values{};
  values.comp_width = 320;
  values.comp_height = 180;
  prepare_effect_layer(layer, values);
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "bee_scene_facade: " << what << "\n" << std::flush;
  };

  using IsLayerType = uint8_t (__cdecl*)(const void*, int32_t);
  using CanStore = uint8_t (__cdecl*)(const void*, const void*);
  using GetStream = const void* (__cdecl*)(const void*, const void*, int32_t);
  using GetSourceItem = const ItemObject* (__cdecl*)(const void*, const void*);

  const void* const* vtable = layer.vtable;
  check(vtable == layer_vtable(), "layer vtable is not the facade vtable");
  check(distinct_entries(vtable, kLayerVtableSlots), "layer vtable slots are null or repeated");
  check(distinct_entries(comp_item().vtable, kItemVtableSlots), "item vtable slots are null or repeated");
  check(distinct_entries(footage_item().vtable, kItemVtableSlots), "footage vtable slots are null or repeated");
  check(comp_item().vtable != footage_item().vtable, "comp and footage items share a vtable");
  check(distinct_entries(project().vtable, kProjectVtableSlots), "project vtable slots are null or repeated");
  if (!passed) return false;

  const auto is_type = reinterpret_cast<IsLayerType>(vtable[kLayerSlotIsLayerType]);
  const auto store = reinterpret_cast<CanStore>(vtable[kLayerSlotCanStore]);
  const auto stream = reinterpret_cast<GetStream>(vtable[kLayerSlotGetStream]);
  const auto source = reinterpret_cast<GetSourceItem>(vtable[kLayerSlotGetSourceItem]);
  const auto const_source =
      reinterpret_cast<GetSourceItem>(vtable[kLayerSlotGetConstSourceItem]);
  const char match_name[] = "ADBE Time Remapping";
  const uint32_t observed_before[] = {
      observed_call_count(kLayerSlotIsLayerType), observed_call_count(kLayerSlotCanStore),
      observed_call_count(kLayerSlotGetStream), observed_call_count(kLayerSlotGetSourceItem),
      observed_call_count(kLayerSlotGetConstSourceItem)};
  check(is_type(&layer, 0) == 1 && is_type(&layer, 1) == 0 && is_type(&layer, -1) == 0,
        "IsLayerType does not answer 0 -> true, other -> false");
  check(store(&layer, match_name) == 0, "CanStore did not answer false");
  check(stream(&layer, match_name, 1) == nullptr, "GetStream did not answer null");
  check(source(&layer, nullptr) == &footage_item() &&
            const_source(&layer, nullptr) == &footage_item(),
        "pGetSourceItem / pGetConstSourceItem did not answer the source item");
  check(observed_call_count(kLayerSlotIsLayerType) == observed_before[0] + 3 &&
            observed_call_count(kLayerSlotCanStore) == observed_before[1] + 1 &&
            observed_call_count(kLayerSlotGetStream) == observed_before[2] + 1 &&
            observed_call_count(kLayerSlotGetSourceItem) == observed_before[3] + 1 &&
            observed_call_count(kLayerSlotGetConstSourceItem) == observed_before[4] + 1,
        "observed slot call counters did not advance");

  // The object graph Timecode walks: layer+0x260 -> comp item -> +0x38 project.
  const auto* comp_via_field =
      *reinterpret_cast<ItemObject* const*>(
          reinterpret_cast<const std::byte*>(&layer) + 0x260);
  check(comp_via_field == &comp_item(), "layer+0x260 is not the comp item");
  const auto* source_via_field =
      *reinterpret_cast<ItemObject* const*>(
          reinterpret_cast<const std::byte*>(&layer) + 0x2720);
  check(source_via_field == &footage_item(), "layer+0x2720 is not the source item");
  const ItemObject& comp = comp_item();
  check(comp.tag == kItemTag && comp.type == kItemTypeComposition &&
            comp.flags == kCompItemFlags && comp.parent_project == &project() &&
            *reinterpret_cast<const uint16_t*>(
                reinterpret_cast<const std::byte*>(&comp) + 8) == kItemTag,
        "comp item header is not the observed BEE_CompItem shape");
  check(comp.duration.value == 300 && comp.duration.scale == 30 &&
            comp.display_start.value == 0 && comp.display_start.scale == 1 &&
            comp.width == 320 && comp.height == 180 &&
            comp.frame_rate_fixed == (30 << 16) && comp.display_dropframe == 0,
        "comp item values do not follow the published scene values");
  const ItemObject& footage = footage_item();
  check(footage.tag == kItemTag && footage.type == kItemTypeFootage &&
            footage.flags == kStillFootageItemFlags &&
            footage.parent_project == &project() && (footage.flags & 0x10) != 0,
        "footage item header is not the observed BEE_FootageItem shape");
  check(project().time_display.frames_per_second == 30 &&
            project().time_display.frames_per_foot == 16,
        "project time display does not follow the published scene values");

  // Re-preparing with other values updates in place: same addresses, new
  // values, so a handle a plug-in kept stays valid and current.
  values.comp_width = 1920;
  values.comp_height = 1080;
  values.frames_per_second = 24;
  values.display_dropframe = true;
  prepare_effect_layer(layer, values);
  check(comp_via_field == &comp_item() && comp.width == 1920 &&
            comp.height == 1080 && comp.frame_rate_fixed == (24 << 16) &&
            comp.display_dropframe == 1 &&
            project().time_display.frames_per_second == 24,
        "re-preparing did not update the published values in place");
  values = SceneValues{};
  values.comp_width = 40000;  // past int16: clamps rather than wraps
  prepare_effect_layer(layer, values);
  check(comp.width == INT16_MAX && comp.height == 0 &&
            comp.frame_rate_fixed == (30 << 16),
        "out-of-range dimensions did not clamp");

  // Every unobserved slot traps with a code that names it, on all three
  // objects, and the traps are counted.
  const uint32_t before = trap_count();
  uintptr_t caller_pc = 0;
  check(call_slot_expecting_trap(vtable, 0, nullptr) ==
            kTrapExceptionBase + kTrapExceptionLayerRange + 0,
        "layer slot 0 did not trap with its code");
  check(call_slot_expecting_trap(vtable, kLayerVtableSlots - 1, nullptr) ==
            kTrapExceptionBase + kTrapExceptionLayerRange +
                static_cast<uint32_t>(kLayerVtableSlots - 1),
        "last layer slot did not trap with its code");
  check(call_slot_expecting_trap(comp.vtable, 3, nullptr) ==
            kTrapExceptionBase + kTrapExceptionItemRange + 3,
        "item slot 3 did not trap with its code");
  check(call_slot_expecting_trap(footage.vtable, 3, nullptr) ==
            kTrapExceptionBase + kTrapExceptionFootageRange + 3,
        "footage slot 3 did not trap with its code");
  check(call_slot_expecting_trap(project().vtable, kProjectVtableSlots - 1,
                                 &caller_pc) ==
            kTrapExceptionBase + kTrapExceptionProjectRange +
                static_cast<uint32_t>(kProjectVtableSlots - 1),
        "last project slot did not trap with its code");
  check(trap_count() == before + 5, "trap count did not advance by the traps taken");
  // The caller the last trap classified must be this module and the frame
  // that called through the slot (call_slot_expecting_trap), not the trap
  // function itself: that is what a `_ReturnAddress()` taken in the wrong
  // frame would produce, and both would print this module's name.
  {
    const uintptr_t last = g_last_trap_return_address.load();
    check(caller_pc != 0 && same_function(last, caller_pc),
          "trap return address is not inside the frame that called the slot");
    const std::string caller = last_trap_caller();
    wchar_t self_path[MAX_PATH]{};
    const DWORD self_length = GetModuleFileNameW(nullptr, self_path, MAX_PATH);
    std::string self_base;
    if (self_length > 0 && self_length < MAX_PATH) {
      std::wstring_view full(self_path, self_length);
      const std::size_t slash = full.find_last_of(L"\\/");
      for (const wchar_t ch : slash == std::wstring_view::npos ? full : full.substr(slash + 1))
        self_base.push_back(ch < 0x20 || ch > 0x7e ? '_' : static_cast<char>(ch));
    }
    const std::size_t plus = caller.find("+0x");
    check(!caller.empty() && caller != "unknown" && plus != std::string::npos &&
              plus + 3 < caller.size() &&
              caller.find(' ') == std::string::npos &&
              caller.find('=') == std::string::npos &&
              !self_base.empty() && caller.substr(0, plus) == self_base,
          "trap caller classification is not <this module>+0x<rva>");
  }
  // Leave the shared graph as a fresh hand-out would (every AEGP_GetEffectLayer
  // re-publishes anyway).
  prepare_effect_layer(layer, SceneValues{});
  return passed;
}

}  // namespace aexcompat::worker_runtime::bee_facade
