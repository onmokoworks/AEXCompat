#include "worker_pf_world_facade.hpp"

#include "worker_bee_scene_facade.hpp"
#include "worker_world_safety.hpp"

#include <windows.h>
#include <intrin.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <iostream>
#include <iterator>
#include <deque>
#include <memory>
#include <sstream>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <utility>

namespace aexcompat::worker_runtime::pf_world_facade {
namespace {

static_assert(kTrapExceptionBase == bee_facade::kTrapExceptionBase,
              "one trap code space for the host facades");
static_assert(kTrapExceptionRange >=
                  bee_facade::kTrapExceptionProjectRange +
                      bee_facade::kProjectVtableSlots,
              "PF_World traps must not overlap the BEE facade ranges");

constexpr std::size_t kReservedLong4Offset = 0x50;
// PF_LayerDef field offsets (world_safety::LocalEffectWorld), used through the
// PF_World object where the LayerDef sits at +8.
constexpr std::size_t kObjectLayerDef = 0x08;
constexpr std::size_t kDataOffset = 24;
constexpr std::size_t kRowbytesOffset = 32;
constexpr std::size_t kWidthOffset = 36;
constexpr std::size_t kHeightOffset = 40;
// Bound on live pool objects. Worlds registered from bare structs are the
// self-tests' and PF_NewWorld's callers' (the latter capped at 64 by the world
// registry); the render paths' worlds are embedded and take no pool entry. A
// caller past the bound gets no facade (reserved_long4 stays as it was), which
// is the pre-#1276 behaviour, and the count is reported.
constexpr std::size_t kMaxObjects = 1024;

// SRW lock rather than std::mutex: attach/detach/attached are noexcept and a
// std::mutex lock can throw.
SRWLOCK g_lock = SRWLOCK_INIT;
struct Exclusive {
  Exclusive() { AcquireSRWLockExclusive(&g_lock); }
  ~Exclusive() { ReleaseSRWLockExclusive(&g_lock); }
  Exclusive(const Exclusive&) = delete;
  Exclusive& operator=(const Exclusive&) = delete;
};
struct Shared {
  Shared() { AcquireSRWLockShared(&g_lock); }
  ~Shared() { ReleaseSRWLockShared(&g_lock); }
  Shared(const Shared&) = delete;
  Shared& operator=(const Shared&) = delete;
};
std::unordered_map<const void*, std::unique_ptr<WorldObject>> g_objects;
// Every object this module has published, by address: the pool's own mirrors
// (erased in `detach`) and the world registry's companions (erased in
// `retire`). `attach` asks this set *first*, by pointer, because
// `reserved_long4` is plug-in-writable and reading through a forged value
// would fault the host outside the plug-in's SEH; only once membership has
// proved the pointer is an object this module allocated does it read that
// object's own back-reference.
std::unordered_set<const void*> g_published_objects;
// Objects nobody may call again, kept inert (see `retire_locked`): the world
// registry's companions when a world is disposed, and the pool's mirrors when
// a scope detaches them. Bounded, because a session disposes scratch worlds
// per frame and the quarantine must not grow without limit; past the bound the
// oldest is released, which is the point where a stale pointer to *that*
// object becomes a plain use-after-free again. 4096 objects is 640 KB at 0xa0
// bytes each - a whole interactive session's worth of scratch worlds, and
// negligible beside the 256 MB the world registry itself may hold.
constexpr std::size_t kMaxRetiredObjects = 4096;
std::deque<std::unique_ptr<WorldObject>> g_retired;
std::atomic<uint32_t> g_trap_count{};
std::atomic<uint32_t> g_copy_rect_calls{};
std::atomic<uint32_t> g_copy_rect_refusals{};
SRWLOCK g_last_trap_caller_lock = SRWLOCK_INIT;
std::string g_last_trap_caller;
std::atomic<TrapRecorder> g_trap_recorder{};
std::atomic<WorldResolver> g_world_resolver{};
// One line per distinct refusal reason per worker process: CopyRect sits on a
// per-frame path, so an unlatched line would stream. The reasons are host
// classifications, not plug-in data.
std::atomic<uint32_t> g_reported_refusals{};
// Latch for the first quarantine eviction line, and the count behind it (see
// `retire_locked`): the line is easy to lose - it fires once, mid-session, and
// only reaches a report through the bounded stderr tail - so the fact is also
// readable as a number.
std::atomic<bool> g_reported_eviction{};
std::atomic<uint32_t> g_quarantine_evictions{};

void report_refusal(uint32_t bit, const char* reason) {
  if ((g_reported_refusals.fetch_or(bit) & bit) != 0) return;
  try {
    std::cerr << "stage:callback_denied callback=pf_world_copy_rect reason=" << reason
              << "\n" << std::flush;
  } catch (...) {
  }
}

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
    if (ch < 0x20 || ch > 0x7e || ch == L' ' || ch == L'=') result.push_back('_');
    else result.push_back(static_cast<char>(ch));
  }
  std::ostringstream text;
  text << result << "+0x" << std::hex
       << (return_address - reinterpret_cast<uintptr_t>(module));
  return text.str();
}

// An unobserved PF_World virtual a plug-in or PF.dll reached. Same shape as
// the BEE facade trap (worker_bee_scene_facade.cpp): record the slot under the
// object's pseudo-suite name, name the caller on stderr, fail the frame
// through the selector SEH containment with a code that names the slot.
[[noreturn]] void trap(uint32_t slot, uintptr_t return_address) {
  ++g_trap_count;
  if (const auto recorder = g_trap_recorder.load()) recorder(slot);
  std::string caller;
  try {
    caller = classify_caller(return_address);
  } catch (...) {
    caller.clear();
  }
  if (caller.empty()) caller = "unknown";
  try {
    std::cerr << "stage:pf_world_facade_trap slot=" << slot << " caller=" << caller
              << "\n" << std::flush;
  } catch (...) {
  }
  {
    AcquireSRWLockExclusive(&g_last_trap_caller_lock);
    struct ReleaseExclusive {
      SRWLOCK* lock;
      ~ReleaseExclusive() { ReleaseSRWLockExclusive(lock); }
    } release{&g_last_trap_caller_lock};
    g_last_trap_caller.swap(caller);
  }
  // RaiseException does not unwind C++ objects, so nothing below this point
  // may own memory: release the local before raising.
  caller.clear();
  caller.shrink_to_fit();
  const ULONG_PTR arguments[] = {static_cast<ULONG_PTR>(kTrapExceptionRange),
                                 static_cast<ULONG_PTR>(slot)};
  RaiseException(kTrapExceptionBase + kTrapExceptionRange + slot,
                 EXCEPTION_NONCONTINUABLE, static_cast<DWORD>(std::size(arguments)),
                 arguments);
  __fastfail(FAST_FAIL_FATAL_APP_EXIT);
}

// `_ReturnAddress()` is evaluated in the trap function itself (the function
// the vtable points at), so the caller it names is the plug-in / PF.dll frame
// that called through the slot. One instantiation per depth table so that a
// slot's function is distinct across the three tables (identical-code folding
// would otherwise merge them, and the report could not tell which table was
// called; the depth is what the tables differ in).
template <int16_t Depth, std::size_t Slot>
void* __cdecl slot_trap() {
  trap(static_cast<uint32_t>(Slot), reinterpret_cast<uintptr_t>(_ReturnAddress()));
}

// PF_WorldX<T> vtable slot 1: the world's bits per channel as a short (8 / 16 /
// 32; PF.dll `PF_WorldX<PF_Pixel8>::vftable[1]` returns 8, the 16 and float
// tables 16 and 32). Glow shifts the answer right by 3 to get bytes per
// channel; PFp_WorldDepth returns it as the depth.
template <int16_t Depth>
int16_t __cdecl slot_depth(const void* /*self*/) {
  return Depth;
}

template <typename T>
T read_at(const void* object, std::size_t offset) noexcept {
  T value{};
  std::memcpy(&value, static_cast<const std::byte*>(object) + offset, sizeof(value));
  return value;
}

struct Rect {
  int32_t left, top, right, bottom;
};

// PF_WorldX<T> vtable slot 14, `CopyRect(const PF_World* src, const M_LRect&
// rect, int dest_x, int dest_y)`: copies `rect` of the source object's pixels
// to (dest_x, dest_y) of this object's. Both objects carry a LayerDef at +8
// (embedded storage or mirror), and PF.dll's own CopyRect throws when the two
// depths differ (this one refuses the same way, quietly: the refusal is
// counted and the destination is untouched). The copy is bounded to both
// buffers, whatever the rectangle says.
template <int16_t Depth>
int32_t __cdecl slot_copy_rect(void* self, const void* source, const Rect* rect,
                               int32_t dest_x, int32_t dest_y) {
  ++g_copy_rect_calls;
  const auto refuse = [](uint32_t bit, const char* reason) {
    ++g_copy_rect_refusals;
    report_refusal(bit, reason);
    return 0;
  };
  if (!self || !source || !rect) return refuse(1u << 0, "null_argument");
  const auto* source_vtable = read_at<const void* const*>(source, 0);
  if (!is_facade_vtable(source_vtable)) return refuse(1u << 1, "foreign_source");
  if (depth_of(source_vtable) != Depth) return refuse(1u << 2, "depth_mismatch");
  const auto resolver = g_world_resolver.load();
  if (!resolver) return refuse(1u << 3, "no_world_resolver");
  // The geometry the host registered, not what the structs declare: both
  // operands' LayerDefs sit at +8 of their object, and an operand the host
  // does not recognise (or whose declared layout no longer matches what was
  // registered) is a refusal, exactly like the other copy callbacks' foreign
  // operand gate. Nothing below reads the plug-in-writable fields.
  ResolvedWorld src{}, dst{};
  if (!resolver(static_cast<const std::byte*>(source) + kObjectLayerDef, src) ||
      !resolver(static_cast<const std::byte*>(self) + kObjectLayerDef, dst))
    return refuse(1u << 4, "unresolved_world");
  const int32_t pixel_bytes = Depth / 8 * 4;
  if (src.pixel_bytes != pixel_bytes || dst.pixel_bytes != pixel_bytes)
    return refuse(1u << 5, "resolved_depth_mismatch");
  // The house bounds (world_safety::bounded_typed_world): every per-row walk
  // stays inside the declared stride for the whole declared height, and the
  // stride comparison is done in 64-bit so a huge width cannot wrap it.
  constexpr int64_t kMaxDimension = 4096;
  constexpr int64_t kMaxPixels = 16'777'216;
  constexpr int64_t kMaxRowbytes = 4096 * 16;
  const auto bounded = [&](const ResolvedWorld& w) {
    const int64_t width = w.width, height = w.height, rowbytes = w.rowbytes;
    return w.data && width > 0 && height > 0 && width <= kMaxDimension &&
        height <= kMaxDimension && width * height <= kMaxPixels &&
        rowbytes >= width * pixel_bytes && rowbytes <= kMaxRowbytes;
  };
  if (!bounded(src) || !bounded(dst)) return refuse(1u << 6, "unbounded_world");
  const auto* src_pixels = static_cast<const std::byte*>(src.data);
  auto* dst_pixels = static_cast<std::byte*>(dst.data);
  // Source rows/columns inside both the rectangle and the source; each maps
  // to dest_x/dest_y + (x - left, y - top) and is copied only where that lands
  // inside the destination.
  const int64_t left = std::max<int64_t>(rect->left, 0);
  const int64_t top = std::max<int64_t>(rect->top, 0);
  const int64_t right = std::min<int64_t>(rect->right, src.width);
  const int64_t bottom = std::min<int64_t>(rect->bottom, src.height);
  if (left >= right || top >= bottom) return 0;
  for (int64_t y = top; y < bottom; ++y) {
    const int64_t dy = static_cast<int64_t>(dest_y) + (y - static_cast<int64_t>(rect->top));
    if (dy < 0 || dy >= dst.height) continue;
    const int64_t dx0 = static_cast<int64_t>(dest_x) + (left - static_cast<int64_t>(rect->left));
    int64_t x0 = left, x1 = right;
    if (dx0 < 0) x0 += -dx0;
    if (dx0 + (right - left) > dst.width) x1 = left + (dst.width - dx0);
    if (x0 >= x1) continue;
    const int64_t dx = static_cast<int64_t>(dest_x) + (x0 - static_cast<int64_t>(rect->left));
    std::memcpy(dst_pixels + dy * dst.rowbytes + dx * pixel_bytes,
                src_pixels + y * src.rowbytes + x0 * pixel_bytes,
                static_cast<std::size_t>((x1 - x0) * pixel_bytes));
  }
  return 0;
}

template <int16_t Depth, std::size_t... Slots>
std::array<const void*, sizeof...(Slots)> make_vtable(std::index_sequence<Slots...>) {
  return {{reinterpret_cast<const void*>(&slot_trap<Depth, Slots>)...}};
}

template <int16_t Depth>
std::array<const void*, kVtableSlots> make_depth_vtable() {
  auto slots = make_vtable<Depth>(std::make_index_sequence<kVtableSlots>{});
  slots[kSlotDepth] = reinterpret_cast<const void*>(&slot_depth<Depth>);
  slots[kSlotCopyRect] = reinterpret_cast<const void*>(&slot_copy_rect<Depth>);
  return slots;
}

struct Tables {
  std::array<const void*, kVtableSlots> depth8;
  std::array<const void*, kVtableSlots> depth16;
  std::array<const void*, kVtableSlots> depth32;
  Tables()
      : depth8(make_depth_vtable<8>()),
        depth16(make_depth_vtable<16>()),
        depth32(make_depth_vtable<32>()) {}
};

const Tables& tables() {
  static const Tables value;
  return value;
}

void mirror(WorldObject& object, const void* world) noexcept {
  // The LayerDef as handed out, with reserved_long4 pointing at the object
  // itself (PF_WorldX: this+0x58 = this).
  std::memcpy(object.layer_def, world, kLayerDefSize);
  WorldObject* self = &object;
  std::memcpy(object.layer_def + kReservedLong4Offset, &self, sizeof(self));
}

void* prefix_of(void* world) noexcept {
  return static_cast<std::byte*>(world) - kObjectLayerDef;
}

// Reads the first word of a candidate PF_World prefix and says whether it is
// one of this module's vtables. Guarded: the only caller reaches here with
// `world - 8`, and a plug-in may point a world at an address whose preceding
// page is not mapped.
bool prefix_carries_facade_vtable(const void* prefix) noexcept {
  const void* vtable = nullptr;
  __try {
    std::memcpy(&vtable, prefix, sizeof(vtable));
  } __except (GetExceptionCode() == EXCEPTION_ACCESS_VIOLATION
                  ? EXCEPTION_EXECUTE_HANDLER
                  : EXCEPTION_CONTINUE_SEARCH) {
    // Only an unmapped prefix is answered with "not embedded". Anything else
    // (a stack overflow, say) keeps unwinding rather than being swallowed here
    // with the guard page left unreset.
    return false;
  }
  return is_facade_vtable(vtable);
}

}  // namespace

const void* const* vtable_for(int32_t pixel_bytes) noexcept {
  switch (pixel_bytes) {
    case 4: return tables().depth8.data();
    case 8: return tables().depth16.data();
    case 16: return tables().depth32.data();
    default: return nullptr;
  }
}

bool is_facade_vtable(const void* vtable) noexcept {
  const Tables& t = tables();
  return vtable && (vtable == t.depth8.data() || vtable == t.depth16.data() ||
                    vtable == t.depth32.data());
}

int16_t depth_of(const void* const* vtable) noexcept {
  const Tables& t = tables();
  if (vtable == t.depth8.data()) return 8;
  if (vtable == t.depth16.data()) return 16;
  if (vtable == t.depth32.data()) return 32;
  return 0;
}

bool embed(void* world, int32_t pixel_bytes) noexcept {
  const void* const* vtable = vtable_for(pixel_bytes);
  if (!world || !vtable) return false;
  void* prefix = prefix_of(world);
  std::memcpy(prefix, &vtable, sizeof(vtable));
  std::memcpy(static_cast<std::byte*>(world) + kReservedLong4Offset, &prefix, sizeof(prefix));
  return true;
}

bool embedded(const void* world) noexcept {
  if (!world) return false;
  // The pointer this reads through is never the plug-in's own value: it is
  // accepted only when it equals `world - 8`, so the read is at most the 8
  // bytes in front of a buffer the host handed out - and the same read
  // recognises a *copy* of an embedded storage (the checkout views the host
  // makes), whose prefix carries the vtable although `embed` never ran on it.
  // The fault guard is for a plug-in that forges `world - 8` on a struct whose
  // preceding bytes are not mapped.
  const void* reserved_long4 = read_at<const void*>(world, kReservedLong4Offset);
  if (reserved_long4 != static_cast<const std::byte*>(world) - kObjectLayerDef) return false;
  return prefix_carries_facade_vtable(reserved_long4);
}

// The publication itself. The caller owns the lock (SRW locks do not recurse,
// and `attach` publishes while holding it) and owns recording the object in
// `g_published_objects`.
bool publish_locked(WorldObject& object, void* world, int32_t pixel_bytes) noexcept {
  const void* const* vtable = vtable_for(pixel_bytes);
  if (!world || !vtable) return false;
  object.vtable = vtable;
  object.attached_world = world;
  object.reserved_80 = nullptr;
  object.reserved_88 = nullptr;
  // Point the world at the object first so the mirror carries the pointer.
  WorldObject* self = &object;
  std::memcpy(static_cast<std::byte*>(world) + kReservedLong4Offset, &self, sizeof(self));
  mirror(object, world);
  return true;
}

bool publish(WorldObject& object, void* world, int32_t pixel_bytes) noexcept {
  Exclusive lock;
  try {
    g_published_objects.insert(&object);
  } catch (...) {
    return false;
  }
  if (!publish_locked(object, world, pixel_bytes)) {
    g_published_objects.erase(&object);
    return false;
  }
  return true;
}

bool attach(void* world, int32_t pixel_bytes) noexcept {
  if (!world || !vtable_for(pixel_bytes)) return false;
  // Embedded storage was published by `prepare_world_layout`, which owns the 8
  // bytes before the LayerDef. Nothing is written here: re-deriving it would
  // mean the host writing through a pointer a plug-in can forge.
  if (embedded(world)) return true;
  Exclusive lock;
  const auto found = g_objects.find(world);
  if (found != g_objects.end()) {
    // Our own mirror: refresh it, so a re-registration after the host changed
    // the world (a new depth, a moved buffer, an origin written after the
    // first registration) is what the plug-in sees.
    return publish_locked(*found->second, world, pixel_bytes);
  }
  // An object this module already published (the world registry's, for a
  // PF_NewWorld allocation it owns) stays that object's owner's business;
  // taking a second, pooled object would orphan it and leave dispose freeing
  // the wrong one. The set answers by pointer, so nothing is read through
  // `reserved_long4` until membership has proved the pointer is an object this
  // module allocated - and only then is its back-reference read, so a plug-in
  // that aims one host world at another's object does not keep this world from
  // getting its own.
  {
    const auto* candidate = read_at<const WorldObject*>(world, kReservedLong4Offset);
    if (g_published_objects.count(candidate) != 0 && candidate->attached_world == world)
      return true;
  }
  if (g_objects.size() >= kMaxObjects) return false;
  WorldObject* object = nullptr;
  try {
    auto fresh = std::make_unique<WorldObject>();
    object = fresh.get();
    g_objects.emplace(world, std::move(fresh));
  } catch (...) {
    return false;
  }
  // Membership first, so a failure here has published nothing and the object
  // can simply go away; publishing first and then failing to record it would
  // free an object the world already points at.
  try {
    g_published_objects.insert(object);
  } catch (...) {
    g_objects.erase(world);
    return false;
  }
  if (!publish_locked(*object, world, pixel_bytes)) {
    g_published_objects.erase(object);
    g_objects.erase(world);
    return false;
  }
  return true;
}

// Defined below, next to `retire`.
void retire_locked(WorldObject* object) noexcept;

void detach(void* world) noexcept {
  if (!world) return;
  Exclusive lock;
  const auto found = g_objects.find(world);
  if (found == g_objects.end()) return;
  void* current{};
  std::memcpy(&current, static_cast<const std::byte*>(world) + kReservedLong4Offset,
              sizeof(current));
  if (current == found->second.get()) {
    void* null = nullptr;
    std::memcpy(static_cast<std::byte*>(world) + kReservedLong4Offset, &null,
                sizeof(null));
  }
  // Only *this* struct was taken off the object; a plug-in that copied the
  // world it was handed still has a copy whose reserved_long4 names it. So the
  // object is neutralised and quarantined rather than freed, the same way a
  // disposed world's object is - freeing here would leave that copy pointing
  // at memory that later holds something callable.
  std::unique_ptr<WorldObject> owned = std::move(found->second);
  g_published_objects.erase(owned.get());
  g_objects.erase(found);
  retire_locked(owned.release());
}

const WorldObject* attached(const void* world) noexcept {
  if (!world) return nullptr;
  Shared lock;
  const auto found = g_objects.find(world);
  return found == g_objects.end() ? nullptr : found->second.get();
}

void set_trap_recorder(TrapRecorder recorder) noexcept { g_trap_recorder.store(recorder); }
void set_world_resolver(WorldResolver resolver) noexcept { g_world_resolver.store(resolver); }

// Takes ownership of an object nobody should call again. The caller holds the
// lock and has already taken the object out of `g_objects` /
// `g_published_objects`.
void retire_locked(WorldObject* object) noexcept {
  if (!object) return;
  // The object stays readable, but inert: a stale `reserved_long4` a plug-in
  // kept now leads to a null vtable slot (a contained fault at a known place)
  // rather than to freed memory that later holds something callable.
  object->vtable = nullptr;
  object->attached_world = nullptr;
  std::memset(object->layer_def, 0, sizeof(object->layer_def));
  try {
    g_retired.push_back(std::unique_ptr<WorldObject>(object));
  } catch (...) {
    // The quarantine could not take it; the object is inert either way and
    // leaking one is better than freeing memory a plug-in may still name.
    return;
  }
  while (g_retired.size() > kMaxRetiredObjects) {
    // The one moment where a stale reference to *that* object stops being
    // contained, so it is said out loud once. Recording, not enforcement.
    ++g_quarantine_evictions;
    if (!g_reported_eviction.exchange(true)) {
      try {
        std::cerr << "stage:pf_world_facade_quarantine_evicted bound="
                  << kMaxRetiredObjects << "\n" << std::flush;
      } catch (...) {
      }
    }
    g_retired.pop_front();
  }
}

void retire(WorldObject* object) noexcept {
  if (!object) return;
  Exclusive lock;
  g_published_objects.erase(object);
  retire_locked(object);
}

std::size_t live_count() noexcept {
  Shared lock;
  return g_objects.size();
}
std::size_t retired_count() noexcept {
  Shared lock;
  return g_retired.size();
}
uint32_t trap_count() noexcept { return g_trap_count.load(); }
uint32_t copy_rect_calls() noexcept { return g_copy_rect_calls.load(); }
uint32_t copy_rect_refusals() noexcept { return g_copy_rect_refusals.load(); }
uint32_t quarantine_evictions() noexcept { return g_quarantine_evictions.load(); }
std::string last_trap_caller() {
  AcquireSRWLockShared(&g_last_trap_caller_lock);
  struct ReleaseShared {
    SRWLOCK* lock;
    ~ReleaseShared() { ReleaseSRWLockShared(lock); }
  } release{&g_last_trap_caller_lock};
  return g_last_trap_caller;
}

namespace {

// A vtable entry read as the function a caller (plug-in / PF.dll) would call:
// the table holds code addresses, so the const of the table's storage says
// nothing about the callee. `const_cast` before the function-pointer cast is
// what clang requires (MSVC accepts the direct reinterpret_cast).
template <typename Fn>
Fn slot_as(const void* const* vtable, std::size_t slot) {
  return reinterpret_cast<Fn>(const_cast<void*>(vtable[slot]));
}

__declspec(noinline) uint32_t call_slot_expecting_trap(const void* const* vtable,
                                                       std::size_t slot) {
  using Slot = void* (__cdecl*)();
  const auto function = slot_as<Slot>(vtable, slot);
  __try {
    (void)function();
    return 0;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return GetExceptionCode();
  }
}

}  // namespace

namespace {
// What the self-test "registered": the geometry the host would have recorded,
// kept out of the structs so the hostile-geometry case below can corrupt the
// LayerDef and still be refused / clamped against the truth.
struct RegisteredWorld { const void* layer_def; ResolvedWorld resolved; };
std::array<RegisteredWorld, 4> g_selftest_registry{};
std::size_t g_selftest_registry_count{};

bool selftest_resolver(const void* layer_def, ResolvedWorld& out) noexcept {
  for (std::size_t i = 0; i < g_selftest_registry_count; ++i) {
    if (g_selftest_registry[i].layer_def == layer_def) {
      out = g_selftest_registry[i].resolved;
      return true;
    }
  }
  return false;
}
}  // namespace

bool selftest() {
  bool passed = true;
  auto check = [&](bool condition, const char* what) {
    if (condition) return;
    passed = false;
    std::cerr << "pf_world_facade: " << what << "\n" << std::flush;
  };
  using Depth = int16_t (__cdecl*)(const void*);
  using CopyRect = int32_t (__cdecl*)(void*, const void*, const Rect*, int32_t, int32_t);
  auto store = [](auto& buffer, std::size_t offset, const auto& value) {
    std::memcpy(buffer.data() + offset, &value, sizeof(value));
  };
  auto read_ptr = [](const void* object, std::size_t offset) {
    return read_at<void*>(object, offset);
  };

  // 1. The bare-struct shape: a 120-byte world gets a pool object.
  alignas(16) std::array<std::byte, world_safety::kEffectWorldSize> world{};
  alignas(16) std::array<std::byte, world_safety::kEffectWorldSize> deep{};
  std::array<std::byte, 64> pixels{};
  void* data = pixels.data();
  store(world, kDataOffset, data);
  store(world, kRowbytesOffset, int32_t{16});
  store(world, kWidthOffset, int32_t{4});
  store(world, kHeightOffset, int32_t{1});
  store(world, 0x68, int32_t{-3});
  store(world, 0x6c, int32_t{7});
  const std::size_t live_before = live_count();
  check(!attach(nullptr, 4), "attach accepted a null world");
  check(!attach(world.data(), 3), "attach accepted 3 bytes per pixel");
  check(attach(world.data(), 4), "attach failed for an 8-bit world");
  void* reserved_long4 = read_ptr(world.data(), kReservedLong4Offset);
  check(reserved_long4 != nullptr && reserved_long4 == attached(world.data()),
        "reserved_long4 does not point at the attached object");
  check(!embedded(world.data()), "a bare struct reads as embedded");
  if (!passed) return false;
  const auto* object = static_cast<const WorldObject*>(reserved_long4);
  const void* const* vtable = object->vtable;
  check(vtable == vtable_for(4) && is_facade_vtable(vtable) && depth_of(vtable) == 8,
        "attached object does not carry the 8-bit vtable");
  if (!passed) return false;
  check(slot_as<Depth>(vtable, kSlotDepth)(object) == 8,
        "slot 1 did not answer 8 for an 8-bit world");
  // The mirror: the LayerDef as handed out, +0x58 pointing at the object
  // (PF_WorldX: data at +0x20, rowbytes +0x28, width +0x2c, height +0x30,
  // origin at +0x70/+0x74, self at +0x58).
  check(read_ptr(object, 0x20) == data && read_at<int32_t>(object, 0x28) == 16 &&
            read_at<int32_t>(object, 0x2c) == 4 && read_at<int32_t>(object, 0x30) == 1 &&
            read_at<int32_t>(object, 0x70) == -3 && read_at<int32_t>(object, 0x74) == 7,
        "mirror does not carry the handed-out LayerDef fields");
  check(read_ptr(object, 0x58) == object, "mirror +0x58 is not the object itself");
  check(read_ptr(object, 0x88) == nullptr, "+0x88 is not zero");
  // Re-attaching the same struct at another depth updates in place (same
  // object, same address a copy would still hold).
  check(attach(world.data(), 16) && attached(world.data()) == object &&
            slot_as<Depth>(object->vtable, kSlotDepth)(object) == 32,
        "re-attach did not update the same object to 32");
  check(attach(deep.data(), 8) && attached(deep.data()) != object &&
            slot_as<Depth>(attached(deep.data())->vtable, kSlotDepth)(
                attached(deep.data())) == 16,
        "a second world did not get its own 16-bit object");
  check(live_count() == live_before + 2, "live count did not advance by two");

  // 2. The embedded shape: storage with the vtable slot before the LayerDef.
  world_safety::EffectWorldStorage source{};
  world_safety::EffectWorldStorage destination{};
  std::array<uint32_t, 4 * 3> src_pixels{};
  std::array<uint32_t, 4 * 3> dst_pixels{};
  for (std::size_t i = 0; i < src_pixels.size(); ++i) src_pixels[i] = static_cast<uint32_t>(i + 1);
  dst_pixels.fill(0xffffffffu);
  auto layout = [&](world_safety::EffectWorldStorage& storage, void* buffer) {
    storage.fill(std::byte{});
    store(storage, kDataOffset, buffer);
    store(storage, kRowbytesOffset, int32_t{16});
    store(storage, kWidthOffset, int32_t{4});
    store(storage, kHeightOffset, int32_t{3});
  };
  layout(source, src_pixels.data());
  layout(destination, dst_pixels.data());
  check(!embed(source.data(), 5), "embed accepted 5 bytes per pixel");
  check(embed(source.data(), 4) && embed(destination.data(), 4), "embed failed");
  check(embedded(source.data()) && embedded(destination.data()), "embedded worlds do not read as embedded");
  check(source.pf_world_vtable == vtable_for(4) &&
            read_ptr(source.data(), kReservedLong4Offset) == source.data() - 8,
        "embed did not write the prefix vtable and reserved_long4 = world - 8");
  // The host's resolver stands in for the registry here: the two worlds are
  // "registered" with their true geometry, and restored on the way out.
  const auto saved_resolver = g_world_resolver.load();
  g_selftest_registry_count = 0;
  g_selftest_registry[g_selftest_registry_count++] = {source.data(), {src_pixels.data(), 16, 4, 3, 4}};
  g_selftest_registry[g_selftest_registry_count++] = {destination.data(), {dst_pixels.data(), 16, 4, 3, 4}};
  set_world_resolver(&selftest_resolver);
  struct RestoreResolver {
    WorldResolver previous;
    ~RestoreResolver() { set_world_resolver(previous); }
  } restore_resolver{saved_resolver};

  // A world - 8 caller: slot 1 on the prefix, then PF_World::CopyWorld's
  // slot 14 with the whole source rect at (0,0).
  const void* src_object = source.data() - 8;
  void* dst_object = destination.data() - 8;
  const auto* src_vtable = read_at<const void* const*>(src_object, 0);
  check(slot_as<Depth>(src_vtable, kSlotDepth)(src_object) == 8,
        "embedded slot 1 did not answer 8");
  const auto copy_rect = slot_as<CopyRect>(src_vtable, kSlotCopyRect);
  const uint32_t copies_before = copy_rect_calls();
  const uint32_t refusals_before = copy_rect_refusals();
  const Rect whole{0, 0, 4, 3};
  copy_rect(dst_object, src_object, &whole, 0, 0);
  check(dst_pixels == src_pixels, "CopyRect of the whole world did not copy the pixels");
  // A sub-rectangle to an offset, clipped at the destination edge.
  dst_pixels.fill(0xffffffffu);
  const Rect part{1, 1, 3, 3};  // src (1..2, 1..2)
  copy_rect(dst_object, src_object, &part, 3, 2);  // lands at (3..4, 2..3): only (3,2)
  {
    std::array<uint32_t, 4 * 3> expected{};
    expected.fill(0xffffffffu);
    expected[2 * 4 + 3] = src_pixels[1 * 4 + 1];
    check(dst_pixels == expected, "CopyRect did not clip the sub-rectangle at the destination");
  }
  // A negative destination clips at the origin.
  dst_pixels.fill(0xffffffffu);
  copy_rect(dst_object, src_object, &whole, -3, -2);  // only src (3,2) -> dst (0,0)
  {
    std::array<uint32_t, 4 * 3> expected{};
    expected.fill(0xffffffffu);
    expected[0] = src_pixels[2 * 4 + 3];
    check(dst_pixels == expected, "CopyRect did not clip a negative destination");
  }
  check(copy_rect_calls() == copies_before + 3 && copy_rect_refusals() == refusals_before,
        "CopyRect call counters did not advance");
  // The depth-mismatch refusal below re-embeds the source, which the resolver
  // table still describes at its honest geometry.
  // Refusals: a source of another depth, a source that is no facade object, a
  // rectangle wider than the source (bounded to it, not refused), null.
  dst_pixels.fill(0xffffffffu);
  check(embed(source.data(), 8), "re-embed at 16 failed");
  copy_rect(dst_object, src_object, &whole, 0, 0);
  check(dst_pixels[0] == 0xffffffffu && copy_rect_refusals() > refusals_before,
        "CopyRect accepted a source of another depth");
  check(embed(source.data(), 4), "re-embed at 8 failed");
  alignas(16) std::array<std::byte, 0xa0> not_an_object{};
  copy_rect(dst_object, not_an_object.data(), &whole, 0, 0);
  copy_rect(dst_object, nullptr, &whole, 0, 0);
  copy_rect(dst_object, src_object, nullptr, 0, 0);
  check(dst_pixels[0] == 0xffffffffu && copy_rect_refusals() >= refusals_before + 4,
        "CopyRect accepted a non-facade source, a null source or a null rect");
  const Rect oversized{-5, -5, 40, 30};
  copy_rect(dst_object, src_object, &oversized, -5, -5);
  check(dst_pixels == src_pixels, "CopyRect did not bound an oversized rectangle to both worlds");
  // Hostile geometry: a plug-in owns the LayerDef bytes, so the copy must run
  // on the registered geometry, not on what the struct claims. Both a wild
  // stride and a wild height must leave the destination exactly as the honest
  // copy would (here: unchanged, because the registered geometry still bounds
  // the copy) and must never walk past the buffers.
  {
    std::array<uint32_t, 4 * 3> guarded{};
    guarded.fill(0xfeedfaceu);
    const auto saved_dst = dst_pixels;
    store(destination, kRowbytesOffset, int32_t{0x10000000});
    store(destination, kHeightOffset, int32_t{0x7fffffff});
    store(source, kWidthOffset, int32_t{0x20000000});
    copy_rect(dst_object, src_object, &whole, 0, 0);
    check(guarded[0] == 0xfeedfaceu, "CopyRect wrote past the destination on corrupted geometry");
    // Restore the honest fields and confirm the copy still behaves.
    store(destination, kRowbytesOffset, int32_t{16});
    store(destination, kHeightOffset, int32_t{3});
    store(source, kWidthOffset, int32_t{4});
    dst_pixels = saved_dst;
    copy_rect(dst_object, src_object, &whole, 0, 0);
    check(dst_pixels == src_pixels, "CopyRect stopped working after corrupted geometry");
  }
  // With no resolver at all every call is refused (the worker installs one).
  {
    const auto had = g_world_resolver.load();
    set_world_resolver(nullptr);
    const uint32_t before_refusals = copy_rect_refusals();
    dst_pixels.fill(0x5a5a5a5au);
    copy_rect(dst_object, src_object, &whole, 0, 0);
    check(dst_pixels[0] == 0x5a5a5a5au && copy_rect_refusals() == before_refusals + 1,
          "CopyRect copied without a world resolver");
    set_world_resolver(had);
    dst_pixels.fill(0xffffffffu);
    copy_rect(dst_object, src_object, &whole, 0, 0);
    check(dst_pixels == src_pixels, "CopyRect did not resume with the resolver back");
  }
  // Storage copies re-point reserved_long4 at their own prefix.
  world_safety::EffectWorldStorage copy = source;
  check(embedded(copy.data()) && read_ptr(copy.data(), kReservedLong4Offset) == copy.data() - 8 &&
            copy.pf_world_vtable == source.pf_world_vtable &&
            read_ptr(copy.data(), kDataOffset) == src_pixels.data(),
        "a storage copy is not embedded on its own");
  // attach() on an embedded world takes no pool entry.
  const std::size_t live_embedded = live_count();
  check(attach(source.data(), 4) && live_count() == live_embedded && attached(source.data()) == nullptr,
        "attach took a pool entry for an embedded world");

  // 3. Every other slot traps with a code that names it (first, an interior
  // one of PF_WorldX's real table, last), on the pool object and the prefix.
  const uint32_t traps_before = trap_count();
  check(call_slot_expecting_trap(vtable, 0) ==
            kTrapExceptionBase + kTrapExceptionRange + 0,
        "slot 0 did not trap with its code");
  check(call_slot_expecting_trap(vtable, 10) ==
            kTrapExceptionBase + kTrapExceptionRange + 10,
        "slot 10 (Premultiply) did not trap with its code");
  check(call_slot_expecting_trap(src_vtable, kVtableSlots - 1) ==
            kTrapExceptionBase + kTrapExceptionRange +
                static_cast<uint32_t>(kVtableSlots - 1),
        "last slot did not trap with its code");
  check(trap_count() == traps_before + 3, "trap count did not advance by three");
  {
    const std::string caller = last_trap_caller();
    const std::size_t plus = caller.find("+0x");
    check(!caller.empty() && caller != "unknown" && plus != std::string::npos &&
              caller.find(' ') == std::string::npos && caller.find('=') == std::string::npos,
          "trap caller classification is not <module>+0x<rva>");
  }
  // 4. Detach takes the world off the object and retires the object: it stays
  // allocated but inert, because a plug-in may hold a copy of the world that
  // still names it. Reading the object after the detach is what asserts that -
  // it was a use-after-free before the object started being quarantined, and
  // it is the property that would notice a regression back to freeing.
  const std::size_t retired_before = retired_count();
  const auto* detached_object = attached(world.data());
  check(detached_object != nullptr, "nothing attached to detach");
  detach(world.data());
  check(read_ptr(world.data(), kReservedLong4Offset) == nullptr && attached(world.data()) == nullptr,
        "detach did not clear reserved_long4 / the pool entry");
  const bool retired = retired_count() == retired_before + 1;
  check(retired, "detach did not retire the object");
  // Only read the object when the count says it is still alive: in a build
  // that regressed to freeing, reading it here would crash the worker and the
  // route would never print the verdict that names the regression.
  if (retired && detached_object) {
    // A stale copy of the world would reach exactly this: a live object whose
    // vtable slot is null (a contained fault at a known place) and whose
    // mirror no longer describes anybody's pixels.
    check(detached_object->vtable == nullptr, "a detached object kept its vtable");
    check(detached_object->attached_world == nullptr,
          "a detached object kept its back-reference");
    bool mirror_zeroed = true;
    for (const std::byte byte : detached_object->layer_def)
      if (byte != std::byte{}) mirror_zeroed = false;
    check(mirror_zeroed, "a detached object kept its LayerDef mirror");
  }
  void* foreign = pixels.data();
  store(deep, kReservedLong4Offset, foreign);
  detach(deep.data());
  check(read_ptr(deep.data(), kReservedLong4Offset) == foreign && attached(deep.data()) == nullptr,
        "detach touched a reserved_long4 the plug-in re-pointed");
  check(live_count() == live_before, "objects were not released");
  check(retired_count() == retired_before + 2, "the second detach did not retire its object");
  // The bound itself: attach/detach one struct past the quarantine's capacity
  // and the deque must stop growing and count an eviction. This is the only
  // place the eviction branch runs, and it must stay the last section, because
  // it leaves the quarantine full **for the rest of the process**: every later
  // retirement then evicts, and what it frees is the object that has been
  // quarantined longest, so from here on an object's stay is bounded by 4096
  // further retirements instead of by the session. The one route that reaches
  // `selftest()` renders no plug-in, so there is no production object in the
  // deque to lose.
  // Two properties nothing here asserts: that the eviction takes the *oldest*
  // object (a `pop_back` regression would leave the same count and the same
  // eviction tally while dropping containment for the most recently retired
  // object, which is the one a stale world copy most likely names), and that
  // the stderr line stays latched to one.
  {
    const uint32_t evictions_before = quarantine_evictions();
    alignas(16) std::array<std::byte, world_safety::kEffectWorldSize> churn{};
    std::array<std::byte, 16> churn_pixels{};
    void* churn_data = churn_pixels.data();
    store(churn, kDataOffset, churn_data);
    store(churn, kRowbytesOffset, int32_t{16});
    store(churn, kWidthOffset, int32_t{4});
    store(churn, kHeightOffset, int32_t{1});
    // `detach` clears `reserved_long4` on the way out, so each pass starts
    // from a struct that carries no object again.
    for (std::size_t i = 0; i <= kMaxRetiredObjects; ++i) {
      if (!attach(churn.data(), 4)) {
        check(false, "churn attach failed");
        break;
      }
      detach(churn.data());
    }
    check(retired_count() == kMaxRetiredObjects,
          "the quarantine did not stop at its bound");
    check(quarantine_evictions() > evictions_before,
          "the quarantine did not report an eviction at the bound");
    check(live_count() == live_before, "the churn left pool objects behind");
    check(read_ptr(churn.data(), kReservedLong4Offset) == nullptr,
          "the churn left an object pointer in the struct");
  }
  detach(nullptr);
  return passed;
}

}  // namespace aexcompat::worker_runtime::pf_world_facade
