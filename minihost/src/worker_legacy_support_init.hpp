#pragma once

#include <windows.h>

// Legacy-support-library process initialization (issue #362 selector
// families; latch semantics fixed for issue #1063): PIN-era bundled effects
// (Curves, Reshape, FILE.dll-based classics) allocate through U.dll's
// process-wide allocator, which real AE initializes at process start by
// calling U_Birth. Nothing in a plug-in closure calls it (verified: no
// bundled DLL imports U_Birth), so the host must, once per U.dll mapping.
//
// The decision is kept separate from the Win32 lookups so its latch can be
// exercised without a real U.dll. What has to hold is that "U.dll is not
// mapped yet" never latches: a discovery/render session loads plug-ins one
// after another into one process, and the first member must decide nothing
// for the ones after it. Circle (no U.dll) followed by Reshape (U.dll) has to
// birth the allocator when Reshape's load maps it, or Reshape's PARAMS_SETUP
// fails its first U_AllocateHandle (observed as PARAMS_SETUP returning 4,
// PF_Err_OUT_OF_MEMORY, while building the arbitrary-data default; issue
// #1063).
namespace aexcompat::worker_runtime::legacy_support {

// What the latch remembers across calls: the U.dll mapping U_Birth was called
// for. A failed U_Birth is not retried on the same mapping (the plug-ins are
// no worse off than before). A different HMODULE is treated as a fresh
// allocator and birthed again; that identity is trustworthy because the
// worker retires plug-in images instead of freeing them until process exit
// (issue #474), so U.dll stays mapped once loaded. A genuine unload followed by
// a same-base remap would not be told apart, and does not occur on the current
// session paths.
struct State {
  HMODULE birthed{};
};

enum class Decision {
  // U.dll is not mapped: nothing to do now, and nothing latched - a later
  // load in the same process is asked again.
  Absent,
  // This mapping already had U_Birth called (or attempted): done.
  AlreadyInitialized,
  // This mapping exports no U_Birth: nothing callable. Not latched either;
  // the export lookup is cheap and the answer cannot change for a mapping.
  NoBirthExport,
  // Call U_Birth on this mapping now, then `note_called`.
  Call,
};

// The pure decision. `birth_exported` is only consulted when `u_module` is
// non-null and not the birthed mapping.
inline Decision decide(const State& state, HMODULE u_module,
                       bool birth_exported) noexcept {
  if (!u_module) return Decision::Absent;
  if (state.birthed == u_module) return Decision::AlreadyInitialized;
  return birth_exported ? Decision::Call : Decision::NoBirthExport;
}

inline void note_called(State& state, HMODULE u_module) noexcept {
  state.birthed = u_module;
}

}  // namespace aexcompat::worker_runtime::legacy_support
