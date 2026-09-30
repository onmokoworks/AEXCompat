#pragma once

#include <cstddef>
#include <cstdint>

// AE's `AEGP_ColorProfileP` is a `COR_ACE_Profile*`, not an opaque token this
// host is free to invent (issue #1300).
//
// `ProfileToProfile.aex` acquires `PF Color Settings Suite` v3, calls
// `AEGP_GetNewWorkingSpaceColorProfile`, and then calls
// `COR.dll!COR_ACE_Profile::GetID` on the result **without going through any
// suite**. `PSL_Adjustments.aex` reaches the same instruction from its own
// render thread. Both faulted on `[rcx+0x10]` with `rcx = 0xf`, which is this
// host's first synthetic handle, `(generation << 3) | 7`.
//
// The way out is not a shape that satisfies `GetID`: the very next call is
// `COR_ACE_Profile::GetProfile`, whose result goes straight into ACE's own
// vtable, so anything invented breaks one step later. COR.dll builds these
// objects itself and exports the factory, so the host asks COR for one.
//
// What was read out of COR.dll (AE 2026, 1,114,632 bytes) for this, all at
// image base 0x180000000:
//
//   * `?Make@COR_ACE_Profile@@SAPEAV1@PEBXI@Z` (+0x8410) is
//     `static COR_ACE_Profile* Make(const void* data, unsigned size)`. It calls
//     ACE's `MakeBufferProfile` on the ICC bytes and wraps the result in a
//     40-byte `COR_ACE_Profile` (no vtable): `+0x00` the inner
//     `shared_ptr<ACEProfile*>`, `+0x10` the 16-byte `_t_ACE_ID` that `GetID`
//     returns, `+0x20` the over-range code. It returns a **raw** pointer, so
//     unlike the `boost::shared_ptr` factories nothing here has to know that
//     type's layout or its control-block vtable.
//   * Every COR ACE entry point begins with a lazy guard over the ACE dispatch
//     table, and that guard **throws** a C++ exception rather than returning a
//     failure when the table cannot be resolved. Its input is set up by
//     `COR_Conception`, which this host calls when it initializes the support
//     libraries around plug-in load (`initialize_process_support_libraries` in
//     `l2_main_support.inc`, resolved through `GetProcAddress`, issue #1279).
//     A `Make` that somehow ran before that would take the throwing guard.
//   * COR's exception translator is installed by `COR_Birth`, which this host
//     does not call, so what arrives is a plain MSVC C++ exception.
//
// Three consequences shape the interface below:
//
//   * The objects are **never released**. `Make` hands back COR's own `malloc`
//     block; freeing it would need COR's CRT, and dropping a reference would
//     need the `boost::shared_ptr` control-block vtable. Both are avoidable, so
//     this caches by ICC bytes and keeps every object it built for the life of
//     the worker process. That is what bounds the cost: a plug-in that asks for
//     the working-space profile once per frame gets the same object every time
//     instead of one allocation per frame. COR.dll is pinned on the first
//     successful resolve so that "for the life of the process" is a fact about
//     the module and not an assumption about it.
//   * The cache owns the ICC bytes too. ACE's slot is named `MakeBufferProfile`
//     (as against `MakeRAMProfile`), which reads as though it may reference the
//     caller's buffer rather than copy it; that is not visible from COR.dll, so
//     the bytes outlive the profile rather than the guess being tested in
//     production.
//   * Caching means **equal ICC bytes get the same address**, which is what the
//     colour-settings registry's use count is for. It also means a handle the
//     plug-in disposed becomes valid again if an identical profile is created
//     afterwards. That aliasing is not something this host introduced: AE's
//     handles are heap addresses too, and an address freed by one dispose can
//     come back from the next allocation. What stays fail-closed at all times
//     is the rest: a handle this suite never issued is refused, and a handle
//     disposed more times than it was issued is refused.
namespace aexcompat::cor_ace {

// Whether COR.dll is mapped into this process and exports the profile factory.
//
// This is also what decides whether a real object is required at all: nothing
// can call `COR_ACE_Profile::GetID` on a handle unless COR.dll is in the
// process, so while it is absent the host's own opaque handle cannot be
// mistaken for one. When it is present, a caller may dereference, and the host
// either produces a real object or fails the callback - it does not hand out
// something that faults on first use.
//
// The residual, stated rather than argued away: COR.dll can enter the process
// after a synthetic handle was already issued, and a plug-in still holding that
// handle could then dereference it. The window is narrow - a plug-in that calls
// `COR_ACE_Profile::GetID` imports COR.dll, so COR is mapped from that plug-in's
// own load, before it can acquire any suite - but it is a window, not an
// impossibility.
//
// Asked afresh each time rather than latched, because a later plug-in's load
// can map COR.dll into a process where an earlier one ran without it.
bool profile_factory_available();

// A real `COR_ACE_Profile*` for these ICC bytes, or null when COR refused,
// threw, faulted, or the cache is full. Repeated calls with equal bytes return
// the same object.
void* profile_from_icc(const std::uint8_t* icc, std::size_t size);

// Records, once per process, that a caller fell back to a handle this host
// invented because COR was not in the process. Without it that state leaves no
// trace at all: `profile_from_icc` is never reached on that branch, so none of
// its markers fire, and the residual the header describes (COR mapped later, a
// plug-in dereferencing a handle issued before that) would be invisible in the
// report it shows up in.
void note_synthetic_handle_issued();

// Test seam. The COR path cannot run anywhere COR.dll is absent, which is every
// environment the self-tests and CI run in, so without this the colour-settings
// registry's shared-identity accounting - the use count, the re-issue equality
// check, the cap, the double-dispose refusal - would never be executed by a
// test. Installing a factory makes `profile_factory_available` answer true and
// routes `profile_from_icc` to it, cache and all. Passing null restores COR.
// Nothing but the self-test route calls this.
//
// It also clears the cache, so it must not be called while the colour-settings
// registry still holds a handle: the next request for the same bytes would
// build a second object and the outstanding handle would stop matching it. The
// self-test installs and removes it around a balanced section.
using ProfileFactory = void* (__cdecl *)(const void* data, unsigned int size);
void set_profile_factory_for_test(ProfileFactory factory);

// Bound on distinct cached objects. This is a **lifetime** cap, not a live one:
// nothing here is ever released, so a worker process that builds this many
// distinct profiles refuses every further distinct one for the rest of its
// life, disposes notwithstanding. That is the price of never releasing, and it
// is set well above what a render is expected to need (a working space plus a
// profile per distinct source ICC).
inline constexpr std::size_t kMaxCachedProfiles = 64;

}  // namespace aexcompat::cor_ace
