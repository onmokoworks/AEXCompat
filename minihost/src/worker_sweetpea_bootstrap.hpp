#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

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
// remembered is the attempt key already tried - the U.dll mapping for U's
// bootstrap, and the (ae_sweetpea mapping, admitted plug-in directory) pair
// for the direct one - so a failed attempt is not retried on the same key
// while a later member with a different key is asked again. That matters more
// here than for U_Birth: the suite resolver is not cached, so this decision is
// asked on every `AcquireSuite`, and a decision that answered "attempt again"
// on a failure would re-run LoadLibraryEx and SPInit on each one. The plug-in
// directory is part of the direct key because the ae_sweetpea load resolves
// the sealed path from the current member, so a first member whose directory
// holds no ae_sweetpea.dll must not burn the only load attempt.
//
// Running the U bootstrap after a direct start has already happened is
// deliberate: the direct start leaves the adapter unregistered, which is the
// whole point. What is observed about doing so, from ae_sweetpea's
// disassembly: `SPInit` guards its body on a counter and returns 0 at once
// when already initialized, incrementing it, so U's `SPInit` call installs no
// second SP context; `SPStartupPlugins` walks the plug-in list, zeroes each
// entry's started field and then starts the list, so plug-ins already started
// are started again rather than skipped. Whether any SP plug-in in an AE
// closure minds being started twice is not established here, and the sweep
// that measured this change did not exercise the order that would show it
// (nothing in the corpus started ae_sweetpea directly before a U.dll member
// arrived).
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
  // The direct attempt key. A null module is a real value here: it means
  // "attempted while ae_sweetpea was not mapped", and a later member that
  // maps it, or one admitted from a different directory, is asked again.
  HMODULE attempted_sweetpea_module = nullptr;
  std::wstring attempted_plugin_directory;
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
  // Sweet Pea is not started: start ae_sweetpea directly, loading it first if
  // it is not mapped. Record the attempt with `begin_direct_start` before
  // calling, for the same reason.
  StartDirectly,
  // Nothing to do now. A different U.dll mapping, a newly mapped
  // ae_sweetpea, or a member admitted from a different directory would still
  // be asked.
  Nothing,
};

inline Decision decide(const State& state, HMODULE u_module,
                       bool u_sp_birth_exported, HMODULE sweetpea_module,
                       const std::wstring& plugin_directory) {
  if (state.bootstrap == Bootstrap::u_sp_birth)
    return Decision::AlreadyBootstrappedThroughU;
  if (state.attempt_in_flight) return Decision::Nothing;
  if (u_module && u_module != state.attempted_u_module && u_sp_birth_exported)
    return Decision::CallUSpBirth;
  if (state.bootstrap != Bootstrap::none) return Decision::Nothing;
  if (!state.direct_attempted ||
      sweetpea_module != state.attempted_sweetpea_module ||
      plugin_directory != state.attempted_plugin_directory)
    return Decision::StartDirectly;
  return Decision::Nothing;
}

// Record the attempt before the call, so a re-entrant acquire sees this key
// as already tried instead of starting a second bootstrap.
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

inline void begin_direct_start(State& state, HMODULE sweetpea_module,
                               const std::wstring& plugin_directory) {
  state.direct_attempted = true;
  state.attempted_sweetpea_module = sweetpea_module;
  state.attempted_plugin_directory = plugin_directory;
  state.attempt_in_flight = true;
  ++state.direct_attempts;
}

inline void finish_direct_start(State& state, bool succeeded) noexcept {
  state.attempt_in_flight = false;
  if (succeeded) state.bootstrap = Bootstrap::direct;
}

// Teardown has to unwind through the layer that started Sweet Pea, and only
// that layer: `U_SP_Death` is U.dll's own SPShutdownPlugins + SPTerm pair, so
// running it and the ae_sweetpea exports would shut the plug-in list down
// twice, and running either one when this process never started Sweet Pea
// would tear down a layer it does not own (ae_sweetpea can be mapped by a
// closure without this host having started it).
//
// `teardown_u_module` returns the U.dll mapping to run `U_SP_Death` on, or
// null; `teardown_sweetpea_directly` says whether the ae_sweetpea exports are
// the ones to run. Both answer "no" when Sweet Pea was never started.
inline HMODULE teardown_u_module(const State& state) noexcept {
  return state.bootstrap == Bootstrap::u_sp_birth ? state.bootstrapped_u_module
                                                  : nullptr;
}

inline bool teardown_sweetpea_directly(const State& state) noexcept {
  return state.bootstrap == Bootstrap::direct;
}

}  // namespace aexcompat::worker_runtime::sweetpea_bootstrap
