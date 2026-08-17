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
#include <memory>
#include <sstream>
#include <string_view>
#include <unordered_map>
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
std::unordered_map<const void*, std::unique_ptr<WorldObject>> g_objects;
std::atomic<uint32_t> g_trap_count{};
std::atomic<uint32_t> g_copy_rect_calls{};
std::atomic<uint32_t> g_copy_rect_refusals{};
std::atomic<uintptr_t> g_last_trap_return_address{};
SRWLOCK g_last_trap_caller_lock = SRWLOCK_INIT;
std::string g_last_trap_caller;
TrapRecorder g_trap_recorder{};

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
  if (g_trap_recorder) g_trap_recorder(slot);
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
    g_last_trap_return_address.store(return_address);
  }
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
void __cdecl slot_copy_rect(void* self, const void* source, const Rect* rect,
                            int32_t dest_x, int32_t dest_y) {
  ++g_copy_rect_calls;
  const auto refuse = [] { ++g_copy_rect_refusals; };
  if (!self || !source || !rect) return refuse();
  const auto* source_vtable = read_at<const void* const*>(source, 0);
  if (!is_facade_vtable(source_vtable) || depth_of(source_vtable) != Depth) return refuse();
  const auto* src = static_cast<const std::byte*>(source) + kObjectLayerDef;
  auto* dst = static_cast<std::byte*>(self) + kObjectLayerDef;
  const auto* src_pixels = read_at<const std::byte*>(src, kDataOffset);
  auto* dst_pixels = read_at<std::byte*>(dst, kDataOffset);
  const int32_t src_rowbytes = read_at<int32_t>(src, kRowbytesOffset);
  const int32_t dst_rowbytes = read_at<int32_t>(dst, kRowbytesOffset);
  const int32_t src_width = read_at<int32_t>(src, kWidthOffset);
  const int32_t src_height = read_at<int32_t>(src, kHeightOffset);
  const int32_t dst_width = read_at<int32_t>(dst, kWidthOffset);
  const int32_t dst_height = read_at<int32_t>(dst, kHeightOffset);
  const int32_t pixel_bytes = Depth / 8 * 4;
  if (!src_pixels || !dst_pixels || src_width <= 0 || src_height <= 0 ||
      dst_width <= 0 || dst_height <= 0 || src_rowbytes < src_width * pixel_bytes ||
      dst_rowbytes < dst_width * pixel_bytes)
    return refuse();
  // Source rows/columns inside both the rectangle and the source; each maps
  // to dest_x/dest_y + (x - left, y - top) and is copied only when inside the
  // destination.
  const int64_t left = std::max<int64_t>(rect->left, 0);
  const int64_t top = std::max<int64_t>(rect->top, 0);
  const int64_t right = std::min<int64_t>(rect->right, src_width);
  const int64_t bottom = std::min<int64_t>(rect->bottom, src_height);
  if (left >= right || top >= bottom) return;
  for (int64_t y = top; y < bottom; ++y) {
    const int64_t dy = static_cast<int64_t>(dest_y) + (y - static_cast<int64_t>(rect->top));
    if (dy < 0 || dy >= dst_height) continue;
    const int64_t dx0 = static_cast<int64_t>(dest_x) + (left - static_cast<int64_t>(rect->left));
    int64_t x0 = left, x1 = right;
    if (dx0 < 0) x0 += -dx0;
    if (dx0 + (right - left) > dst_width) x1 = left + (dst_width - dx0);
    if (x0 >= x1) continue;
    const int64_t dx = static_cast<int64_t>(dest_x) + (x0 - static_cast<int64_t>(rect->left));
    std::memcpy(dst_pixels + dy * dst_rowbytes + dx * pixel_bytes,
                src_pixels + y * src_rowbytes + x0 * pixel_bytes,
                static_cast<std::size_t>((x1 - x0) * pixel_bytes));
  }
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
  const void* reserved_long4 = read_at<const void*>(world, kReservedLong4Offset);
  if (reserved_long4 != static_cast<const std::byte*>(world) - kObjectLayerDef) return false;
  return is_facade_vtable(read_at<const void*>(reserved_long4, 0));
}

bool publish(WorldObject& object, void* world, int32_t pixel_bytes) noexcept {
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

bool attach(void* world, int32_t pixel_bytes) noexcept {
  if (!world || !vtable_for(pixel_bytes)) return false;
  // Embedded storage already is the object; only the depth may differ from
  // what the layout writer chose (it should not), and that is corrected in
  // place.
  if (embedded(world)) return embed(world, pixel_bytes);
  Exclusive lock;
  WorldObject* object = nullptr;
  const auto found = g_objects.find(world);
  if (found != g_objects.end()) {
    object = found->second.get();
  } else {
    if (g_objects.size() >= kMaxObjects) return false;
    std::unique_ptr<WorldObject> fresh;
    try {
      fresh = std::make_unique<WorldObject>();
      object = fresh.get();
      g_objects.emplace(world, std::move(fresh));
    } catch (...) {
      return false;
    }
  }
  return publish(*object, world, pixel_bytes);
}

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
  g_objects.erase(found);
}

const WorldObject* attached(const void* world) noexcept {
  if (!world) return nullptr;
  Exclusive lock;
  const auto found = g_objects.find(world);
  return found == g_objects.end() ? nullptr : found->second.get();
}

void set_trap_recorder(TrapRecorder recorder) noexcept { g_trap_recorder = recorder; }

std::size_t live_count() noexcept {
  Exclusive lock;
  return g_objects.size();
}
uint32_t trap_count() noexcept { return g_trap_count.load(); }
uint32_t copy_rect_calls() noexcept { return g_copy_rect_calls.load(); }
uint32_t copy_rect_refusals() noexcept { return g_copy_rect_refusals.load(); }
std::string last_trap_caller() {
  AcquireSRWLockShared(&g_last_trap_caller_lock);
  struct ReleaseShared {
    SRWLOCK* lock;
    ~ReleaseShared() { ReleaseSRWLockShared(lock); }
  } release{&g_last_trap_caller_lock};
  return g_last_trap_caller;
}

namespace {

__declspec(noinline) uint32_t call_slot_expecting_trap(const void* const* vtable,
                                                       std::size_t slot) {
  using Slot = void* (__cdecl*)();
  const auto function = reinterpret_cast<Slot>(vtable[slot]);
  __try {
    (void)function();
    return 0;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return GetExceptionCode();
  }
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
  using CopyRect = void (__cdecl*)(void*, const void*, const Rect*, int32_t, int32_t);
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
  check(reinterpret_cast<Depth>(vtable[kSlotDepth])(object) == 8,
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
            reinterpret_cast<Depth>(object->vtable[kSlotDepth])(object) == 32,
        "re-attach did not update the same object to 32");
  check(attach(deep.data(), 8) && attached(deep.data()) != object &&
            reinterpret_cast<Depth>(attached(deep.data())->vtable[kSlotDepth])(
                attached(deep.data())) == 16,
        "a second world did not get its own 16-bit object");
  check(live_count() == live_before + 2, "live count did not advance by two");

  // 2. The embedded shape: storage with the vtable slot before the LayerDef.
  world_safety::EffectWorldStorage source{};
  world_safety::EffectWorldStorage destination{};
  std::array<uint32_t, 4 * 3> src_pixels{};
  std::array<uint32_t, 4 * 3> dst_pixels{};
  for (std::size_t i = 0; i < src_pixels.size(); ++i) src_pixels[i] = 0x01000000u * 0 + static_cast<uint32_t>(i + 1);
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
  // A world - 8 caller: slot 1 on the prefix, then PF_World::CopyWorld's
  // slot 14 with the whole source rect at (0,0).
  const void* src_object = source.data() - 8;
  void* dst_object = destination.data() - 8;
  const auto* src_vtable = read_at<const void* const*>(src_object, 0);
  check(reinterpret_cast<Depth>(src_vtable[kSlotDepth])(src_object) == 8,
        "embedded slot 1 did not answer 8");
  const auto copy_rect = reinterpret_cast<CopyRect>(src_vtable[kSlotCopyRect]);
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
  // Refusals: a source of another depth, a source that is no facade object, a
  // rectangle wider than the source (bounded to it, not refused), null.
  dst_pixels.fill(0xffffffffu);
  check(embed(source.data(), 8), "re-embed at 16 failed");
  copy_rect(dst_object, src_object, &whole, 0, 0);
  check(dst_pixels[0] == 0xffffffffu && copy_rect_refusals() == refusals_before + 1,
        "CopyRect accepted a source of another depth");
  check(embed(source.data(), 4), "re-embed at 8 failed");
  alignas(16) std::array<std::byte, 0xa0> not_an_object{};
  copy_rect(dst_object, not_an_object.data(), &whole, 0, 0);
  copy_rect(dst_object, nullptr, &whole, 0, 0);
  copy_rect(dst_object, src_object, nullptr, 0, 0);
  check(dst_pixels[0] == 0xffffffffu && copy_rect_refusals() == refusals_before + 4,
        "CopyRect accepted a non-facade source, a null source or a null rect");
  const Rect oversized{-5, -5, 40, 30};
  copy_rect(dst_object, src_object, &oversized, -5, -5);
  check(dst_pixels == src_pixels && copy_rect_refusals() == refusals_before + 4,
        "CopyRect did not bound an oversized rectangle to both worlds");
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
  // 4. Detach clears the pointer it installed and frees the object; a struct
  // the plug-in re-pointed elsewhere is left alone.
  detach(world.data());
  check(read_ptr(world.data(), kReservedLong4Offset) == nullptr && attached(world.data()) == nullptr,
        "detach did not clear reserved_long4 / the pool entry");
  void* foreign = pixels.data();
  store(deep, kReservedLong4Offset, foreign);
  detach(deep.data());
  check(read_ptr(deep.data(), kReservedLong4Offset) == foreign && attached(deep.data()) == nullptr,
        "detach touched a reserved_long4 the plug-in re-pointed");
  check(live_count() == live_before, "objects were not released");
  detach(nullptr);
  return passed;
}

}  // namespace aexcompat::worker_runtime::pf_world_facade
