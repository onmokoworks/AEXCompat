#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

// PF_World-compatible object around / behind a handed-out world (issue #1276;
// the shape #1090 / PR #1092 met as an opaque pointer).
//
// In AE the `PF_LayerDef` a plug-in receives is not a free-standing struct: it
// is embedded at +8 of PF.dll's `PF_World` (C++ class `PF_WorldX<T>`, 0x90
// bytes; `PF_WorldX<T>::PF_WorldX(PF_LayerDef*, bool)` at PF.dll+0xb7f0 copies
// `world_flags` to this+0x18, `data` to +0x20, `rowbytes/width/height` to
// +0x28/+0x2c/+0x30, `extent_hint` to +0x34 and writes `this+0x58 = this`),
// and `PF_LayerDef.reserved_long4` (+0x50 = this+0x58) points back at that
// object. Adobe-bundled effects and PF.dll rely on both halves without a null
// check:
//
//   * Glow.aex (`FUN_180009f90`, +0xa0b3) reads `world+0x50`, then calls
//     vtable slot 1 on it and uses the returned short as bits per channel
//     (`>> 3` = bytes per channel) to pick a compositing path;
//   * PF.dll `PFp_WorldDepth(PF_LayerDef*, bool)` (+0xf640) asks the same slot
//     when the pointer is non-null and falls back to `world_flags` byte 3
//     only when it is null;
//   * Spill2.aex (`FUN_180005210`) takes the checked-out input and output
//     `PF_LayerDef*` minus 8 as `PF_World*` and calls PF.dll's
//     `PF_World::CopyWorld(dst, src)`, which calls `dst->vtable[14]`
//     (`CopyRect(const PF_World* src, const M_LRect&, int dest_x, int dest_y)`)
//     after comparing the two objects' slot-1 depths; Curl_Noise computes the
//     same `world - 8`;
//   * Channel Blur (issue #1090) writes the world's origin at
//     `reserved_long4 + 0x70 / +0x74` = the embedded LayerDef's origin_x/y.
//
// So the host now hands out its render worlds in AE's shape: the world
// storage the smart / classic paths own (`world_safety::EffectWorldStorage`)
// is 8 + 120 bytes, the vtable pointer sits right before the LayerDef, and
// `reserved_long4` points at that prefix (`prepare_world_layout` /
// `embed`). Worlds registered for dispatch from a bare 120-byte struct (the
// self-tests, PF_NewWorld's caller-owned struct) get the second-best shape,
// a separate object at `reserved_long4` whose +8 region mirrors the LayerDef
// as handed out (`attach` / `publish`); such a world is not `world - 8`
// addressable. There is one vtable per depth (8 / 16 / 32, like PF.dll's
// three `PF_WorldX<T>::vftable`s): slot 1 answers the depth, slot 14 copies a
// rectangle of the source object's pixels into this object's (the bounded,
// same-depth copy PF_World::CopyWorld describes), and every other slot is an
// identifying trap (recorded as `PF_World vtable` slot N in
// unsupported_suite_calls, `stage:pf_world_facade_trap` line, SEH
// 0xE0428200 + slot) so a caller that needs more fails the frame explicitly
// instead of continuing on a guessed answer.
namespace aexcompat::worker_runtime::pf_world_facade {

// PF_WorldX<T>::vftable has 19 entries (0..18) in AE 2026 (PF.dll RTTI walk,
// issue #1276); the table is padded to 32 so a caller indexing past the real
// table still lands in a trap of this object rather than in whatever follows.
inline constexpr std::size_t kVtableSlots = 32;
inline constexpr std::size_t kSlotDepth = 1;      // short: 8 / 16 / 32
inline constexpr std::size_t kSlotCopyRect = 14;  // (const PF_World* src, const M_LRect&, int dx, int dy)
inline constexpr std::size_t kLayerDefSize = 0x78;

// Trap exception code: base shared with the BEE scene facade
// (worker_bee_scene_facade.hpp kTrapExceptionBase 0xE0428000), range 0x200
// (past the BEE project range 0x180 + 8 slots).
inline constexpr uint32_t kTrapExceptionBase = 0xE0428000u;
inline constexpr uint32_t kTrapExceptionRange = 0x200u;

// The geometry a copy is allowed to use: what the host registered for a world,
// never what the struct currently declares. `CopyRect` (slot 14) resolves both
// of its operands through this before touching a pixel, the way every other
// copy callback in the host resolves its operands
// (`world_registry::resolve_dispatch_world_format`); the fields a plug-in can
// overwrite (data / rowbytes / width / height, and the whole mirror object)
// are therefore not what drives the memcpy.
struct ResolvedWorld {
  void* data{};
  int32_t rowbytes{};
  int32_t width{};
  int32_t height{};
  int32_t pixel_bytes{};
};
// Installed by the worker (worker_entry_wiring) with the host's own resolver.
// Without one, CopyRect refuses every call: an unresolvable operand is a
// refusal, not an admission.
using WorldResolver = bool (*)(const void* layer_def, ResolvedWorld& out) noexcept;
void set_world_resolver(WorldResolver resolver) noexcept;

// The object at `reserved_long4` of a world without embedded storage: AE's
// PF_World shape (vtable, LayerDef at +8 pointing back at the object from
// +0x58, the two trailing words PF_WorldX zeroes) plus host bookkeeping.
struct alignas(16) WorldObject {
  const void* const* vtable{};                 // 0x00
  std::byte layer_def[kLayerDefSize]{};        // 0x08: LayerDef mirror (+0x58 = this)
  void* reserved_80{};                         // 0x80 (unobserved, zero)
  void* reserved_88{};                         // 0x88 (PF_WorldX ctor writes 0)
  const void* attached_world{};                // 0x90: the struct this mirrors (pool key)
  std::byte reserved_98[8]{};
};
static_assert(offsetof(WorldObject, layer_def) == 0x08);
static_assert(offsetof(WorldObject, reserved_88) == 0x88);
static_assert(sizeof(WorldObject) == 0xa0);

// The vtable for a depth (bytes per pixel 4 / 8 / 16), or null for anything
// else. Also what `world_safety::EffectWorldStorage::pf_world_vtable` carries.
const void* const* vtable_for(int32_t pixel_bytes) noexcept;
// Whether `vtable` is one of the three facade vtables, and its depth (8/16/32).
bool is_facade_vtable(const void* vtable) noexcept;
int16_t depth_of(const void* const* vtable) noexcept;

// Embeds a world whose LayerDef is preceded by an 8-byte vtable slot: writes
// the depth's vtable at `world - 8` and points reserved_long4 (+0x50) at it.
// The caller vouches that the 8 bytes before `world` are its storage
// (world_safety::EffectWorldStorage). Returns false for an unknown depth.
bool embed(void* world, int32_t pixel_bytes) noexcept;

// True when `world` carries an embedded facade (reserved_long4 == world - 8
// and the prefix is a facade vtable).
bool embedded(const void* world) noexcept;

// Attaches (or refreshes) a facade to `world` for the given bytes per pixel.
// It never writes outside `world` itself: a world that already carries a
// facade (embedded storage, or an object the world registry published) is left
// exactly as it is, and any other world gets a pool object at reserved_long4
// whose +8 region mirrors the world's current fields. Keyed by the world
// struct's address, so re-registering the same buffer updates its object in
// place (copies a plug-in or the host made of the struct keep pointing at it).
// Returns false, leaving reserved_long4 untouched, when the pool of objects is
// exhausted or the arguments are invalid.
bool attach(void* world, int32_t pixel_bytes) noexcept;

// The same publication into a caller-owned object (no pool): what
// PF_NewWorld's world registry uses, since it owns the world's lifetime and
// disposes through it. Writes `world`'s reserved_long4 too.
bool publish(WorldObject& object, void* world, int32_t pixel_bytes) noexcept;

// Detaches the pool object of `world` (a world the host is done handing out;
// `DispatchWorldFormatScope` does this for everything it attached). The
// world's reserved_long4 is cleared when it still points at the object, and
// the object itself is retired into the same bounded quarantine `retire` uses
// rather than freed - a plug-in may hold a copy of the world that still names
// it. No-op for a world without one.
void detach(void* world) noexcept;

// Retires an object the caller owns (the world registry's, when PF_DisposeWorld
// frees the allocation): nulls the vtable and zeroes the mirror, then keeps the
// object alive in a bounded quarantine instead of freeing it. A plug-in holding
// a stale copy of the disposed struct still has `reserved_long4` pointing here,
// and a stale call then faults on a null vtable slot inside contained dispatch
// rather than jumping through whatever the freed heap block came to hold.
// Beyond the quarantine bound the oldest retired object is released.
void retire(WorldObject* object) noexcept;

// The pool object attached to `world`, or null.
const WorldObject* attached(const void* world) noexcept;

// Where a trap records its slot: the worker installs
// `record_unsupported_suite_call(UnsupportedSuiteId::pf_world_vtable, slot)`
// here at start-up (worker_entry_wiring), so the report's
// unsupported_suite_calls list names the slot as `PF_World vtable`. Kept as a
// hook so this object stays free of the suite registry's link closure (the
// world-safety self-test executables link it without the registry).
using TrapRecorder = void (*)(uint32_t slot) noexcept;
void set_trap_recorder(TrapRecorder recorder) noexcept;

// Number of live pool objects / retired objects / trap slots taken / CopyRect
// calls and refusals so far. The self-test reads these; `retired_count` is
// also what says how close the quarantine is to its bound, and the first
// eviction past it writes a `stage:pf_world_facade_quarantine_evicted` line.
std::size_t live_count() noexcept;
std::size_t retired_count() noexcept;
uint32_t trap_count() noexcept;
uint32_t copy_rect_calls() noexcept;
uint32_t copy_rect_refusals() noexcept;
std::string last_trap_caller();

bool selftest();

}  // namespace aexcompat::worker_runtime::pf_world_facade
