#pragma once

#include <windows.h>

#include <cstdint>

// Sweet Pea (PICA plug-in host) bootstrap decision (issue #1279).
//
// ae_sweetpea.dll is the SP-suite plug-in host. Starting it with a bare
// `SPInit(nullptr, nullptr, 0)` + `SPStartupPlugins()` gets the SP suites up,
// but that is not the bootstrap the plug-ins that reach SP through U.dll are
// built against. `U_SP_GetSPBasicSuite` hands out a table of U.dll's own
// locking thunks and answers 11 while U.dll's underlying `SPBasicSuite*`
// global is null, and U.dll only latches that pointer out of the
// ("SP Interface", "Startup") message delivered to the "Sweet Pea 2 Adapter"
// host plug-in that U.dll registers itself. The exported `U_SP_Birth` is that
// whole sequence (install U's SPHostProcs, SPInit with them, SPAddHostPlugin
// for the adapter, add the two AS ZString suites, SPStartupPlugins), so when
// U.dll is in the process the vendor's own bootstrap is the one to run.
//
// The latch shape matches worker_legacy_support_init.hpp: an in-place cluster
// session loads plug-ins one after another into one process, so a first
// member without U.dll must decide nothing for the members after it. What is
// remembered is the mapping already attempted - separately for the U path and
// the direct path - so a failed attempt is not retried on the same mapping
// while a later mapping is asked again. That matters more here than for
// U_Birth: the suite resolver is not cached, so this decision is asked on
// every `AcquireSuite`, and a decision that answered "attempt again" on a
// failure would re-run LoadLibraryEx and SPInit on each one.
//
// Running the U bootstrap after a direct start has already happened is
// deliberate: the direct start leaves the adapter unregistered, which is the
// whole point. It is safe because ae_sweetpea's `SPInit` is reference-counted
// (its disassembly guards the body on a counter and returns 0 immediately
// when already initialized, incrementing it), so U's `SPInit` call becomes a
// no-op that installs no second SP context; only `SPStartupPlugins` re-runs,
// which restarts the plug-in list it walks.
namespace aexcompat::worker_runtime::sweetpea_bootstrap {

enum class Bootstrap {
  // Sweet Pea has not been started in this process.
  none,
  // Started through U.dll's `U_SP_Birth`.
  u_sp_birth,
  // Started by calling ae_sweetpea's SPInit/SPStartupPlugins directly.
  direct,
};

struct State {
  Bootstrap bootstrap = Bootstrap::none;
  // The U.dll mapping whose `U_SP_Birth` succeeded, so teardown runs
  // `U_SP_Death` on the mapping that was actually bootstrapped rather than
  // whatever is mapped at exit.
  HMODULE bootstrapped_u_module = nullptr;
  HMODULE attempted_u_module = nullptr;
  // The ae_sweetpea mapping the direct start was attempted against. Null is a
  // real value here: it means "attempted while ae_sweetpea was not mapped",
  // and a later member that maps it is asked again.
  HMODULE attempted_sweetpea_module = nullptr;
  bool direct_attempted = false;
  // Set between `begin_*` and `finish_*`. A bootstrap that is still running
  // must not be answered with "start the other one": `U_SP_Birth` ends in
  // `SPStartupPlugins`, so a suite acquire from inside it would otherwise
  // reach the direct start while U's own SPInit is still in flight.
  bool attempt_in_flight = false;
  // How often each bootstrap was actually entered. The decision is asked on
  // every suite acquire; these count the attempts it let through.
  uint32_t u_attempts = 0;
  uint32_t direct_attempts = 0;
};

enum class Decision {
  // U.dll already bootstrapped Sweet Pea: nothing left to do, ever.
  AlreadyBootstrappedThroughU,
  // Call `U_SP_Birth` on this U.dll mapping now. Record the attempt with
  // `begin_u_sp_birth` *before* calling: `U_SP_Birth` ends in
  // `SPStartupPlugins`, which runs foreign code that can acquire a suite and
  // re-enter this decision.
  CallUSpBirth,
  // No U.dll bootstrap available (or it already failed on this mapping) and
  // Sweet Pea is not started: start ae_sweetpea directly. Record the attempt
  // with `begin_direct_start` before calling, for the same reason.
  StartDirectly,
  // Nothing to do now. A different U.dll or ae_sweetpea mapping would still
  // be asked.
  Nothing,
};

inline Decision decide(const State& state, HMODULE u_module,
                       bool u_sp_birth_exported,
                       HMODULE sweetpea_module) noexcept {
  if (state.bootstrap == Bootstrap::u_sp_birth)
    return Decision::AlreadyBootstrappedThroughU;
  if (state.attempt_in_flight) return Decision::Nothing;
  if (u_module && u_module != state.attempted_u_module && u_sp_birth_exported)
    return Decision::CallUSpBirth;
  if (state.bootstrap != Bootstrap::none) return Decision::Nothing;
  if (!state.direct_attempted ||
      sweetpea_module != state.attempted_sweetpea_module)
    return Decision::StartDirectly;
  return Decision::Nothing;
}

// Record the attempt before the call, so a re-entrant acquire sees this
// mapping as already tried instead of starting a second bootstrap.
inline void begin_u_sp_birth(State& state, HMODULE u_module) noexcept {
  state.attempted_u_module = u_module;
  state.attempt_in_flight = true;
  ++state.u_attempts;
}

inline void finish_u_sp_birth(State& state, HMODULE u_module,
                              bool succeeded) noexcept {
  state.attempt_in_flight = false;
  if (!succeeded) return;
  state.bootstrap = Bootstrap::u_sp_birth;
  state.bootstrapped_u_module = u_module;
}

inline void begin_direct_start(State& state,
                               HMODULE sweetpea_module) noexcept {
  state.direct_attempted = true;
  state.attempted_sweetpea_module = sweetpea_module;
  state.attempt_in_flight = true;
  ++state.direct_attempts;
}

inline void finish_direct_start(State& state, bool succeeded) noexcept {
  state.attempt_in_flight = false;
  if (succeeded) state.bootstrap = Bootstrap::direct;
}

// Teardown has to unwind through the layer that started Sweet Pea: `U_SP_Death`
// is U.dll's own SPShutdownPlugins + SPTerm pair, so calling it and the
// ae_sweetpea exports would shut the plug-in list down twice. Returns the
// U.dll mapping to run `U_SP_Death` on, or null when teardown belongs to
// ae_sweetpea.
inline HMODULE teardown_u_module(const State& state) noexcept {
  return state.bootstrap == Bootstrap::u_sp_birth ? state.bootstrapped_u_module
                                                  : nullptr;
}

}  // namespace aexcompat::worker_runtime::sweetpea_bootstrap
