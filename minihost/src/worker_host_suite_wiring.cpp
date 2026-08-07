#include "worker_host_suite_catalog.hpp"
#include "worker_host_suite_router.hpp"
#include "worker_extended_diag.hpp"
#include "worker_aefx_ace_suite.hpp"
#include "worker_aegp_persistent_data_suite.hpp"
#include "worker_flt_blur_suite.hpp"
#include "worker_suite_call_slot_probe.hpp"
#include "worker_suite_registry.hpp"

#include "pf_cache_on_load_suite.hpp"
#include "gpu_memory_world_transport.hpp"
#include "trace_writer.hpp"
#include "worker_aegp_compute_cache.hpp"
#include "worker_aegp_command_suites.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_pf_interface_suite.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_utility_suite.hpp"
#include "worker_color_settings_runtime.hpp"
#include "worker_drawbot_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_l2_render_abi.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_mask_suite_tables.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_pf_ansi_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_pf_param_suites.hpp"
#include "worker_pf_pixel_data_suite.hpp"
#include "worker_pf_pixel_format_registry.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_pf_world_suite.hpp"
#include "worker_pf_world_transform_runtime.hpp"
#include "worker_world_registry.hpp"

#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iterator>
#include <mutex>
#include <string>
#include <windows.h>

// Host suite catalog wiring moved from worker_main (issue #171): the
// component providers, the assembly hook table, the static suite catalog,
// and the acquire/release entry points now register here. Suite tables and
// callbacks resolve through their owner headers; worker-entry mode state
// reads through its Phase D owners.
namespace aexcompat::l2_detail {

// Current plug-in path, owned by l2_main (bounds the on-demand BIB.dll
// load to the admitted plug-in directory, issue #362).
extern std::wstring g_plugin_file_path;


// Mirrors the l2_main.cpp preamble that worker_l2_suite_abi.hpp relies on:
// the ABI header captures these signatures with decltype before the wiring
// below references the tables.
struct LegacyRect { int32_t left, top, right, bottom; };
int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds);
#include "worker_l2_suite_abi.hpp"

using namespace aexcompat::pf_ae_channel;
using namespace aexcompat::world_registry;
using namespace aexcompat::render_options;
using aexcompat::worker_runtime::SuiteResolveResult;
using aexcompat::suites::cache_on_load_suite;
using aexcompat::worker_runtime::handles::g_aegp_memory_suite;
using aexcompat::worker_runtime::handles::g_handle_suite;
using namespace aexcompat::pf_state_runtime;
using namespace aexcompat::color_settings;
using aexcompat::scene_runtime::composition_handle;
using AegpStreamValue = aexcompat::scene_runtime::AegpStreamValue;
using aexcompat::worker_runtime::host_suites::StaticSuite;
using aexcompat::worker_runtime::host_suites::configure_suite_assembly;
using aexcompat::worker_runtime::host_suites::configure_host_suite_catalog;

// Worker-entry owned mode predicates and the trace writer stay in l2_main;
// the wiring reads them cross-TU.
bool is_render_worker();
extern aexcompat::TraceWriter* g_trace_writer;
int32_t __cdecl get_context_async_manager(void* input, void* extra, void** manager);
int32_t __cdecl aegp_get_new_effect_stream_by_index_v2(
    int32_t plugin_id, void* effect, int32_t index, void** stream);
int32_t __cdecl aegp_dispose_stream_v2(void* stream);
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t force_english, char* name);
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type);
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t time_mode, const AegpTime* time,
    uint8_t pre_expression, AegpStreamValue* output);
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output);
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input);
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set);

namespace {
auto& g_gpu_device_suite1 =
    aexcompat::gpu_runtime::memory_world_transport::gpu_device_suite1;
bool& g_aegp_init_mode = aexcompat::worker_runtime::aegp_init::state().init_mode;
bool& g_aegp_command_roundtrip_mode =
    aexcompat::scene_runtime::scene_runtime_state().command_roundtrip_mode;

using BibResolver = void* (__cdecl *)(const char* interface_name,
                                      const char* procedure_name,
                                      const char* signature);
using BibGetResolver = BibResolver (__cdecl *)();
using BibInitialize4 = BibResolver (__cdecl *)(
    void*, void*, void*, void*, void*, void*, void*, void*, int32_t,
    uintptr_t, void*);
using BibTerminate = uint32_t (__cdecl *)();
constexpr uintptr_t kBibOwnershipToken = 0x13579BDFu;

struct BibSuiteState {
  std::mutex mutex;
  std::array<void*, 1> suite{};
  BibResolver resolver{};
  BibTerminate terminate{};
  bool attempted{};
  bool owned{};
  bool termination_attempted{};
  uint32_t termination_attempts{};
};

BibSuiteState& bib_suite_state() {
  static BibSuiteState state;
  return state;
}

const void* bib_resolver_entry() noexcept;

const void* provide_bib_suite_locked() {
  auto& state = bib_suite_state();
  std::lock_guard<std::mutex> lock(state.mutex);
  if (state.attempted) return state.resolver ? state.suite.data() : nullptr;
  state.attempted = true;

  // The dependency closure usually loads BIB.dll before the worker reaches
  // the suite catalog. Closures that never link BIB statically (issue #362:
  // Scribble acquires the BIB suite without importing BIB.dll) get one
  // bounded load attempt: USER_DIRS + SYSTEM32 only, so the image can only
  // come from the admitted plug-in directory set, never an arbitrary path,
  // and the module audit observes it exactly like a static import. In real
  // AE the PICA basic is a host facility that is always available.
  HMODULE bib = GetModuleHandleW(L"BIB.dll");
  if (!bib && !g_plugin_file_path.empty()) {
    // The admitted plug-in directory is the only root a suite request
    // may load from (the params-only admission loads the plug-in by
    // absolute path, so USER_DIRS does not cover it).
    const std::filesystem::path sealed_bib =
        std::filesystem::path(g_plugin_file_path).parent_path() / L"BIB.dll";
    bib = LoadLibraryExW(sealed_bib.c_str(), nullptr,
                         LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
                             LOAD_LIBRARY_SEARCH_SYSTEM32);
  }
  // In-place loads (issue #751) keep the host facility beside the real
  // dependency closure, not beside the plug-in: resolve by name through the
  // admitted USER_DIRS search set, which only admission populates.
  if (!bib)
    bib = LoadLibraryExW(L"BIB.dll", nullptr,
                         LOAD_LIBRARY_SEARCH_USER_DIRS |
                             LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:bib_provide module=" << (void*)bib << "\n" << std::flush;
  if (!bib) return nullptr;
  const auto get_resolver = reinterpret_cast<BibGetResolver>(
      GetProcAddress(bib, "BIBGetGetProcAddress"));
  if (get_resolver) state.resolver = get_resolver();
  if (!state.resolver) {
    const auto initialize = reinterpret_cast<BibInitialize4>(
        GetProcAddress(bib, "BIBInitialize4"));
    const auto terminate = reinterpret_cast<BibTerminate>(
        GetProcAddress(bib, "BIBTerminate"));
    if (!initialize || !terminate) return nullptr;
    // AdobePIE's private callbacks are intentionally not guessed here. The
    // verified BIB fallback accepts null callbacks and keeps ownership inside
    // this worker process; shutdown is therefore process-scoped and never
    // releases an Adobe-owned initialization.
    state.resolver = initialize(nullptr, nullptr, nullptr, nullptr, nullptr,
                                nullptr, nullptr, nullptr, 1, kBibOwnershipToken, nullptr);
    if (state.resolver) {
      state.terminate = terminate;
      state.owned = true;
    }
  }
  if (!state.resolver) return nullptr;

  constexpr const char* required[] = {
      "BIBRegisterProcAddress", "BIBReportError",
      "BIBUnregisterInterface", "BIBGetUnregisterCountAddr",
      "BIBIsMultiThreaded",
  };
  for (const char* procedure : required) {
    if (!state.resolver("BIB", procedure, procedure)) {
      if (aexcompat::l2_detail::extended_diag_enabled())
        std::cerr << "extended_diag:bib_provide missing_proc=" << procedure << "\n" << std::flush;
      state.resolver = nullptr;
      return nullptr;
    }
  }
  state.suite[0] = reinterpret_cast<void*>(&bib_resolver_entry);
  return state.suite.data();
}


// Diagnostics (issue #362, PP gs=11): with AEXCOMPAT_EXTENDED_DIAG=1,
// probe the BIB memory interface the plug-ins use next: resolve
// Alloc/Free and round-trip one small block, logging the outcome. The
// fallback BIBInitialize4 passes null host callbacks; if the BIB
// allocator needs them, this probe fails exactly where the plug-in
// would.
typedef void*(__cdecl* BibMemAlloc)(size_t);
typedef void(__cdecl* BibMemFree)(void*);
int bib_mem_probe_seh_filter(EXCEPTION_POINTERS*) {
  return EXCEPTION_EXECUTE_HANDLER;
}
int bib_mem_probe(BibMemAlloc alloc_fn, BibMemFree free_fn) {
  __try {
    void* block = alloc_fn(64);
    if (!block) return 1;
    free_fn(block);
    return 0;
  } __except (bib_mem_probe_seh_filter(GetExceptionInformation())) {
    return -1;
  }
}

void run_bib_memory_probe() {
  if (!aexcompat::l2_detail::extended_diag_enabled()) return;
  auto& state = bib_suite_state();
  if (!state.resolver) return;
  const auto alloc_fn = reinterpret_cast<BibMemAlloc>(
      state.resolver("BIBMemoryInterface", "Alloc", "BIBMemAllocProc"));
  const auto free_fn = reinterpret_cast<BibMemFree>(
      state.resolver("BIBMemoryInterface", "Free", "BIBMemFreeProc"));
  std::cerr << "extended_diag:bib_mem_probe alloc=" << (void*)alloc_fn
            << " free=" << (void*)free_fn;
  if (alloc_fn && free_fn)
    std::cerr << " roundtrip=" << bib_mem_probe(alloc_fn, free_fn);
  std::cerr << "\n" << std::flush;
}
// PICA component DLLs register their BIB interfaces through the DVA Bravo
// initializer, which the real host drives at process start (issue #362:
// ProfileToProfile resolves ACEInterface2 through the BIB resolver, and a
// bare ACEInitialize call crashes inside ACE on host-provided tables that
// only the Bravo init sequence populates). Mirror the host: hand the
// initializer the BIB resolver, then run InitBravoComponents with an
// error-report callback, once per process, after BIB is up and outside
// its mutex. Loads stay bounded to the admitted plug-in directory set.
// ae_sweetpea is the SP-suite plugin host (issue #362, Particle_Playground
// gs=11): the plug-in acquires "SP Adapters Suite" v3 / "SP Plug-ins Suite"
// v4 through U.dll's U_SP_GetSPBasicSuite, and those suites only exist
// after ae_sweetpea's SPInit + SPStartupPlugins, which the real host runs
// at process start. SPInit(nullptr, nullptr, 0) installs ae_sweetpea's own
// default host procs for every null slot (verified in its disassembly).
int bravo_init_seh_filter(EXCEPTION_POINTERS*);
typedef int(__cdecl* SPInitFn)(void*, void*, int32_t);
typedef int(__cdecl* SPStartupPluginsFn)();
int sweetpea_init_guarded(SPInitFn sp_init, SPStartupPluginsFn sp_startup) {
  __try {
    const int init_result = sp_init(nullptr, nullptr, 0);
    if (init_result != 0) return init_result;
    return sp_startup();
  } __except (bravo_init_seh_filter(GetExceptionInformation())) {
    return -1;
  }
}

int bravo_init_seh_filter(EXCEPTION_POINTERS*) {
  return EXCEPTION_EXECUTE_HANDLER;
}

void __cdecl bravo_error_report(const char*, ...) {}

typedef void*(__cdecl* BravoResolver)(const char*, const char*, const char*);
typedef void(__cdecl* BravoErrorReport)(const char*, ...);
typedef void(__cdecl* BravoSetBibProcAddress)(BravoResolver);
typedef BravoResolver(__cdecl* BravoInitComponents)(BravoErrorReport);

int bravo_call_guarded(HMODULE module, const char* export_name,
                       BravoResolver resolver, BravoResolver* out) {
  __try {
    if (resolver == nullptr) {
      const auto set_address = reinterpret_cast<BravoSetBibProcAddress>(
          GetProcAddress(module, export_name));
      if (!set_address) return -1;
      set_address(out ? *out : nullptr);
      return 0;
    }
    const auto init = reinterpret_cast<BravoInitComponents>(
        GetProcAddress(module, export_name));
    if (!init) return -1;
    const BravoResolver initialized = init(&bravo_error_report);
    if (out) *out = initialized;
    return initialized ? 0 : 1;
  } __except (bravo_init_seh_filter(GetExceptionInformation())) {
    return -1;
  }
}

// Reverse-order teardown for the PICA components (issue #362): dvacore
// fast-fails during LdrShutdownProcess when the Bravo/sweetpea-initialized
// subsystems were never torn down through the host path. Registering this
// after their init makes it run first in the EXE atexit chain (LIFO),
// before the DLL detach handlers that would otherwise fatal.
void teardown_pica_components() {
  // Flush the report before any subsystem teardown runs: the CRT flush
  // handlers are registered earlier and therefore run later (LIFO), and
  // the component teardown must not outrun them (issue #362).
  std::cout.flush();
  std::fflush(stdout);
  __try {
    if (HMODULE sweetpea = GetModuleHandleW(L"ae_sweetpea.dll")) {
      const auto sp_shutdown = reinterpret_cast<int(__cdecl*)()>(
          GetProcAddress(sweetpea, "?SPShutdownPlugins@ae_sweetpea@@YAHXZ"));
      const auto sp_term = reinterpret_cast<int(__cdecl*)()>(
          GetProcAddress(sweetpea, "?SPTerm@ae_sweetpea@@YAHXZ"));
      if (sp_shutdown) sp_shutdown();
      if (sp_term) sp_term();
    }
    if (HMODULE bravo = GetModuleHandleW(L"dvabravoinitializer.dll")) {
      const auto terminate = reinterpret_cast<bool(__cdecl*)(bool)>(
          GetProcAddress(
              bravo, "?TerminateBravoComponents@dvabravoinitializer@@YA_N_N@Z"));
      if (terminate) terminate(false);
    }
  } __except (bravo_init_seh_filter(GetExceptionInformation())) {
  }
}

void ensure_pica_components_initialized() {
  auto& state = bib_suite_state();
  static bool attempted = false;
  if (attempted) return;
  attempted = true;
  HMODULE bravo = GetModuleHandleW(L"dvabravoinitializer.dll");
  if (!bravo && !g_plugin_file_path.empty()) {
    const std::filesystem::path sealed_bravo =
        std::filesystem::path(g_plugin_file_path).parent_path() /
        L"dvabravoinitializer.dll";
    bravo = LoadLibraryExW(sealed_bravo.c_str(), nullptr,
                           LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
                               LOAD_LIBRARY_SEARCH_SYSTEM32);
  }
  // In-place loads (issue #751): same admitted USER_DIRS name resolution as
  // the BIB fallback above.
  if (!bravo)
    bravo = LoadLibraryExW(L"dvabravoinitializer.dll", nullptr,
                           LOAD_LIBRARY_SEARCH_USER_DIRS |
                               LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!bravo) {
    if (aexcompat::l2_detail::extended_diag_enabled())
      std::cerr << "extended_diag:pica_component dll=dvabravoinitializer.dll status=absent"
                "\n" << std::flush;
    return;
  }
  BravoResolver current = state.resolver;
  int result = bravo_call_guarded(
      bravo, "?SetBIBProcAddress@dvabravoinitializer@@YAXP6APEAXPEBD00@Z@Z",
      nullptr, &current);
  const BravoResolver before = current;
  result = bravo_call_guarded(
      bravo, "?InitBravoComponents@dvabravoinitializer@@YAP6APEAXPEBD00@ZP6AX0@Z@Z",
      current, &current) == 0
         ? 0 : result;
  if (current && current != before) {
    std::lock_guard<std::mutex> lock(state.mutex);
    state.resolver = current;
  }
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:pica_component dll=dvabravoinitializer.dll status=called result="
              << result << "\n" << std::flush;
  HMODULE sweetpea = GetModuleHandleW(L"ae_sweetpea.dll");
  if (!sweetpea && !g_plugin_file_path.empty()) {
    const std::filesystem::path sealed_sp =
        std::filesystem::path(g_plugin_file_path).parent_path() /
        L"ae_sweetpea.dll";
    sweetpea = LoadLibraryExW(sealed_sp.c_str(), nullptr,
                              LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR |
                                  LOAD_LIBRARY_SEARCH_SYSTEM32);
  }
  // In-place loads (issue #751): same admitted USER_DIRS name resolution as
  // the BIB fallback above.
  if (!sweetpea)
    sweetpea = LoadLibraryExW(L"ae_sweetpea.dll", nullptr,
                              LOAD_LIBRARY_SEARCH_USER_DIRS |
                                  LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (sweetpea) {
    const auto sp_init = reinterpret_cast<SPInitFn>(GetProcAddress(
        sweetpea,
        "?SPInit@ae_sweetpea@@YAHPEAUSPHostProcs@@PEBUSPPlatformFileSpecification@@H@Z"));
    const auto sp_startup = reinterpret_cast<SPStartupPluginsFn>(
        GetProcAddress(sweetpea, "?SPStartupPlugins@ae_sweetpea@@YAHXZ"));
    const int sp_result = (sp_init && sp_startup)
        ? sweetpea_init_guarded(sp_init, sp_startup) : -1;
    if (aexcompat::l2_detail::extended_diag_enabled())
      std::cerr << "extended_diag:pica_component dll=ae_sweetpea.dll status=called result="
                << sp_result << "\n" << std::flush;
  }
  std::atexit(&teardown_pica_components);
}

const void* provide_bib_suite(void*) {
  const void* suite = provide_bib_suite_locked();
  if (suite) {
    ensure_pica_components_initialized();
    run_bib_memory_probe();
  }
  return suite;
}

// The "AEFX Text BIB Suite" entry is arg-agnostic, exactly like the exported
// BIBGetGetProcAddress: plug-ins (e.g. the VR family, issue #362) call it
// with arbitrary register state to obtain the current resolver, then drive
// the resolver with proper arguments themselves. Putting the resolver itself
// in the slot makes that call enter the resolver with garbage arguments and
// crash inside BIB; after teardown the slot naturally yields nullptr, again
// matching BIBGetGetProcAddress on an uninitialized BIB.
// Diagnostics (issue #362 BIB resolver tracing): with
// AEXCOMPAT_EXTENDED_DIAG=1 the suite hands out a proxy that logs every
// (interface, procedure, signature) triple a plug-in resolves and the
// resolver's answer, then forwards unchanged. With the variable unset
// the raw resolver is returned, byte-identical to the production path.
void* __cdecl bib_resolver_trace_proxy(const char* interface_name,
                                       const char* procedure_name,
                                       const char* signature) {
  auto& state = bib_suite_state();
  void* result = state.resolver
      ? state.resolver(interface_name, procedure_name, signature)
      : nullptr;
  std::cerr << "extended_diag:bib_resolve";
  aexcompat::l2_detail::diag_probe_arg("if", interface_name);
  aexcompat::l2_detail::diag_probe_arg("proc", procedure_name);
  aexcompat::l2_detail::diag_probe_arg("sig", signature);
  std::cerr << " -> " << result << "\n" << std::flush;
  return result;
}

const void* bib_resolver_entry() noexcept {
  auto& state = bib_suite_state();
  std::lock_guard<std::mutex> lock(state.mutex);
  if (state.resolver && aexcompat::l2_detail::extended_diag_enabled())
    return reinterpret_cast<const void*>(&bib_resolver_trace_proxy);
  return state.resolver;
}

bool teardown_bib_suite_impl(void*) noexcept {
  auto& state = bib_suite_state();
  std::lock_guard<std::mutex> lock(state.mutex);
  if (!state.owned || state.termination_attempted) return true;
  state.termination_attempted = true;
  ++state.termination_attempts;
  if (!state.terminate) return false;
  const uint32_t token = state.terminate();
  state.owned = false;
  state.resolver = nullptr;
  return token == kBibOwnershipToken;
}
}  // namespace

bool teardown_bib_suite(void* context) noexcept {
  return teardown_bib_suite_impl(context);
}

// Number of owned-BIB termination attempts so far (0 or 1 by construction:
// the latch above makes the teardown single-shot). Read by the cluster
// session's final receipt (owner review P1-1, issue #405).
uint32_t bib_termination_attempt_count() noexcept {
  auto& state = bib_suite_state();
  std::lock_guard<std::mutex> lock(state.mutex);
  return state.termination_attempts;
}

bool mask_suite_provider_available(void*) { return aexcompat::mask_runtime::model_enabled(); }

bool render_options4_provider_available(void*) {
  return is_render_worker() && aexcompat::aegp_layer_render_runtime::active();
}

bool render_suite2_provider_available(void*) {
  return g_aegp_command_roundtrip_mode ||
      (is_render_worker() && aexcompat::aegp_layer_render_runtime::active());
}

bool aegp_init_suite_provider_available(void*) { return g_aegp_init_mode; }
bool render_worker_suite_provider_available(void*) { return is_render_worker(); }

const void* provide_batch_sampling1(void*) {
  g_batch_sampling_suite1 = {&begin_sampling8, &end_sampling8,
      &unsupported_batch_sample_func, &unsupported_batch_sample_func16};
  return &g_batch_sampling_suite1;
}
const void* provide_color_settings7(void*) {
  configure_host_hooks({&composition_handle, &acquire_suite, &release_suite});
  return aexcompat::color_settings::suite();
}
const void* provide_flt_blur1(void*) {
  return aexcompat::flt_blur::suite1();
}
// One catalog entry serves this name and version: the opt-in slot probe when
// it is armed, the implementation otherwise. Registering both as separate
// entries would leave which one answers up to catalog ordering.
const void* provide_aefx_ace1(void*) {
  if (const void* probe = aexcompat::worker_runtime::suite_call_slot_probe::
          provide_aefx_ace_probe1(nullptr))
    return probe;
  return aexcompat::aefx_ace::suite1();
}
// Versions 3, 4 and 6 of "PF Color Settings Suite" are frozen prefixes of the
// v7 table, so all four are served from it (issue #362: the OCIO family acquires
// exactly v6; issue #716: `Unmult.aex` acquires v4; issue #891: DeepGlow2
// acquires v3). Checked against the SDK headers rather than assumed: v3 is
// `AEGP_ColorSettingsSuite2`, whose 10 functions are the first 10 of v4;
// v4 is `AEGP_ColorSettingsSuite3`, whose 11
// functions match the first 11 of v6 (`Suite5`) and v7 (`Suite6`) in name and
// argument types, and v6's 14 match the first 14 of v7. A plug-in that
// acquired the older version reads only that many entries.
const void* provide_iterate8(void*) {
  g_iterate8_suite2.iterate = reinterpret_cast<void*>(&iterate_world8); return &g_iterate8_suite2;
}
const void* provide_sampling8(void*) {
  g_sampling8_suite1[0] = reinterpret_cast<void*>(&nearest_sample8);
  g_sampling8_suite1[1] = reinterpret_cast<void*>(&subpixel_sample8);
  g_sampling8_suite1[2] = reinterpret_cast<void*>(&area_sample8); return g_sampling8_suite1.data();
}
const void* provide_sampling16(void*) {
  g_sampling16_suite1[0] = reinterpret_cast<void*>(&nearest_sample16);
  g_sampling16_suite1[1] = reinterpret_cast<void*>(&subpixel_sample16);
  g_sampling16_suite1[2] = reinterpret_cast<void*>(&area_sample16); return g_sampling16_suite1.data();
}
const void* provide_sampling_float(void*) {
  g_sampling_float_suite1[0] = reinterpret_cast<void*>(&nearest_sample_float);
  g_sampling_float_suite1[1] = reinterpret_cast<void*>(&subpixel_sample_float);
  g_sampling_float_suite1[2] = reinterpret_cast<void*>(&area_sample_float); return g_sampling_float_suite1.data();
}
SuiteResolveResult resolve_scene_suite_provider(
    void*, const char* name, int32_t version, const void** suite) {
  if (scene_context()) {
    const SceneSuiteAcquireResult scene_result =
        scene_acquire_suite(name, version, suite);
    if (scene_result == SceneSuiteAcquireResult::acquired) {
      return SuiteResolveResult::acquired;
    }
    if (scene_result == SceneSuiteAcquireResult::rejected)
      return SuiteResolveResult::rejected_bad_param;
  }
  return SuiteResolveResult::not_found;
}

bool configure_component_suite_catalog() {
  using namespace aexcompat::worker_runtime::host_suites;
  const AssemblyHooks assembly{
      {reinterpret_cast<void*>(&aexcompat::pf_path_runtime::num_paths),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_info),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::checkout_path),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::checkin_path)},
      {reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_is_open),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_num_segments),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_vertex_info),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_prepare_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_eval_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_eval_seg_length_deriv1),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_cleanup_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_is_inverted),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_mask_mode),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_name)},
      reinterpret_cast<void*>(&duck_quack),
      reinterpret_cast<void*>(&set_options_button_name),
      reinterpret_cast<void*>(&adv_app_info_text),
      reinterpret_cast<void*>(&adv_app_info_text3),
      {reinterpret_cast<void*>(&drawbot_get_supplier),
       reinterpret_cast<void*>(&drawbot_get_surface)},
      reinterpret_cast<void*>(&drawbot_new_pen),
      reinterpret_cast<void*>(&drawbot_new_brush),
      reinterpret_cast<void*>(&drawbot_new_path),
      reinterpret_cast<void*>(&drawbot_release_object),
      reinterpret_cast<void*>(&drawbot_paint_rect),
      reinterpret_cast<void*>(&drawbot_fill_path),
      reinterpret_cast<void*>(&drawbot_stroke_path),
      reinterpret_cast<void*>(&drawbot_path_point),
      reinterpret_cast<void*>(&drawbot_add_rect),
      reinterpret_cast<void*>(&get_drawing_reference),
      reinterpret_cast<void*>(&get_context_async_manager),
      reinterpret_cast<void*>(&overlay_foreground),
      reinterpret_cast<void*>(&overlay_stroke_path),
      {reinterpret_cast<void*>(&app_get_background_color),
       reinterpret_cast<void*>(&app_get_color),
       reinterpret_cast<void*>(&app_get_language),
       reinterpret_cast<void*>(&app_get_personal_info),
       reinterpret_cast<void*>(&app_get_font_style),
       reinterpret_cast<void*>(&app_set_cursor),
       reinterpret_cast<void*>(&app_is_render_engine),
       reinterpret_cast<void*>(&app_color_picker),
       reinterpret_cast<void*>(&app_get_mouse),
       reinterpret_cast<void*>(&app_invalidate_rect),
       reinterpret_cast<void*>(&app_convert_local_to_global),
       reinterpret_cast<void*>(&app_get_color_at_global_point),
       reinterpret_cast<void*>(&app_create_progress_dialog),
       reinterpret_cast<void*>(&app_update_progress_dialog),
       reinterpret_cast<void*>(&app_dispose_progress_dialog)},
      {reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan2),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_ceil), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_cos),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_exp), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fabs),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_floor), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fmod),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_hypot), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log10), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_pow),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sin), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sqrt),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_tan), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sprintf),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_asin),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_acos)},
      reinterpret_cast<void*>(&aegp_set_dynamic_stream_flag_v2),
      {reinterpret_cast<void*>(&aegp_world_new_owned), reinterpret_cast<void*>(&aegp_world_dispose), reinterpret_cast<void*>(&aegp_world_get_type), reinterpret_cast<void*>(&aegp_world_get_size), reinterpret_cast<void*>(&aegp_world_get_rowbytes), reinterpret_cast<void*>(&aegp_world_get_base_addr8), reinterpret_cast<void*>(&aegp_world_get_base_addr16), reinterpret_cast<void*>(&aegp_world_get_base_addr32), reinterpret_cast<void*>(&aegp_world_fill_pf_world), reinterpret_cast<void*>(&aegp_world_fast_blur), reinterpret_cast<void*>(&aegp_world_new_platform), reinterpret_cast<void*>(&aegp_world_dispose_platform), reinterpret_cast<void*>(&aegp_world_reference_platform)},
      {reinterpret_cast<void*>(&new_layer_render_options), reinterpret_cast<void*>(&new_from_upstream_of_effect), reinterpret_cast<void*>(&duplicate_layer_render_options), reinterpret_cast<void*>(&dispose_layer_render_options), reinterpret_cast<void*>(&set_layer_render_time), reinterpret_cast<void*>(&get_layer_render_time), reinterpret_cast<void*>(&set_layer_render_time_step), reinterpret_cast<void*>(&get_layer_render_time_step), reinterpret_cast<void*>(&set_layer_render_world_type), reinterpret_cast<void*>(&get_layer_render_world_type), reinterpret_cast<void*>(&set_layer_render_downsample), reinterpret_cast<void*>(&get_layer_render_downsample), reinterpret_cast<void*>(&set_layer_render_matte), reinterpret_cast<void*>(&get_layer_render_matte)},
      {reinterpret_cast<void*>(&new_layer_render_options), reinterpret_cast<void*>(&new_from_upstream_of_effect), reinterpret_cast<void*>(&new_from_downstream_of_effect), reinterpret_cast<void*>(&duplicate_layer_render_options), reinterpret_cast<void*>(&dispose_layer_render_options), reinterpret_cast<void*>(&set_layer_render_time), reinterpret_cast<void*>(&get_layer_render_time), reinterpret_cast<void*>(&set_layer_render_time_step), reinterpret_cast<void*>(&get_layer_render_time_step), reinterpret_cast<void*>(&set_layer_render_world_type), reinterpret_cast<void*>(&get_layer_render_world_type), reinterpret_cast<void*>(&set_layer_render_downsample), reinterpret_cast<void*>(&get_layer_render_downsample), reinterpret_cast<void*>(&set_layer_render_matte), reinterpret_cast<void*>(&get_layer_render_matte)},
      {reinterpret_cast<void*>(&render_options_new_from_item), reinterpret_cast<void*>(&render_options_duplicate), reinterpret_cast<void*>(&render_options_dispose), reinterpret_cast<void*>(&render_options_set_time), reinterpret_cast<void*>(&render_options_get_time), reinterpret_cast<void*>(&render_options_set_time_step), reinterpret_cast<void*>(&render_options_get_time_step), reinterpret_cast<void*>(&render_options_set_field), reinterpret_cast<void*>(&render_options_get_field), reinterpret_cast<void*>(&render_options_set_world_type), reinterpret_cast<void*>(&render_options_get_world_type), reinterpret_cast<void*>(&render_options_set_downsample), reinterpret_cast<void*>(&render_options_get_downsample), reinterpret_cast<void*>(&render_options_set_roi), reinterpret_cast<void*>(&render_options_get_roi), reinterpret_cast<void*>(&render_options_set_matte), reinterpret_cast<void*>(&render_options_get_matte)},
      {reinterpret_cast<void*>(&render_options_new_from_item), reinterpret_cast<void*>(&render_options_duplicate), reinterpret_cast<void*>(&render_options_dispose), reinterpret_cast<void*>(&render_options_set_time), reinterpret_cast<void*>(&render_options_get_time), reinterpret_cast<void*>(&render_options_set_time_step), reinterpret_cast<void*>(&render_options_get_time_step), reinterpret_cast<void*>(&render_options_set_field), reinterpret_cast<void*>(&render_options_get_field), reinterpret_cast<void*>(&render_options_set_world_type), reinterpret_cast<void*>(&render_options_get_world_type), reinterpret_cast<void*>(&render_options_set_downsample), reinterpret_cast<void*>(&render_options_get_downsample), reinterpret_cast<void*>(&render_options_set_roi), reinterpret_cast<void*>(&render_options_get_roi), reinterpret_cast<void*>(&render_options_set_matte), reinterpret_cast<void*>(&render_options_get_matte), reinterpret_cast<void*>(&render_options_set_channel_order), reinterpret_cast<void*>(&render_options_get_channel_order), reinterpret_cast<void*>(&render_options_get_guide_layers), reinterpret_cast<void*>(&render_options_set_guide_layers), reinterpret_cast<void*>(&render_options_get_quality), reinterpret_cast<void*>(&render_options_set_quality)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&render_checkout_layer_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered), reinterpret_cast<void*>(&render_guid_reject)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&render_checkout_layer_v5), reinterpret_cast<void*>(&render_checkout_layer_async_reject), reinterpret_cast<void*>(&render_cancel_async_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered), reinterpret_cast<void*>(&render_guid_reject)},
      {reinterpret_cast<void*>(&checkout_item_frame_async), reinterpret_cast<void*>(&checkout_layer_frame_async)}};
  if (!configure_suite_assembly(assembly)) return false;
  const StaticSuite component_suites[] = {
      {"AE Plugin Helper Suite", 1, aexcompat::pf_helper::suite1()},
      {"AE Plugin Helper Suite2", 2, aexcompat::pf_helper::suite2()},
      {"AEFX Text BIB Suite", 1, nullptr, &provide_bib_suite},
      {"PF Cache On Load Suite", 1, &cache_on_load_suite()},
      {"PF AE Adv Time Suite", 1,
       aexcompat::worker_runtime::pf_adv_time::suite(1)},
      {"PF AE Adv Time Suite", 2,
       aexcompat::worker_runtime::pf_adv_time::suite(2)},
      {"PF AE Adv Time Suite", 3,
       aexcompat::worker_runtime::pf_adv_time::suite(3)},
      {"PF AE Adv Time Suite", 4,
       aexcompat::worker_runtime::pf_adv_time::suite(4)},
      {"AEGP Memory Suite", 1, &g_aegp_memory_suite},
      {"AEGP Utility Suite", 3, &g_utility_suite1},
      {"AEGP Utility Suite", 7, &g_utility_suite3},
      {"AEGP Utility Suite", 11, &g_utility_suite5},
      {"AEGP Utility Suite", 13, &g_utility_suite},
      {"PF Pixel Data Suite", 1, &g_pixel_data_suite1},
      {"PF Pixel Data Suite", 2, &g_pixel_data_suite2},
      {"PF World Suite", 1, g_world_suite1.data()},
      {"PF World Suite", 2, &g_world_suite},
      {"PF Pixel Format Suite", 2, &g_pixel_format_suite},
      {"PF PointParamSuite", 1, &g_point_param_suite},
      {"PF AngleParamSuite", 1, &g_angle_param_suite},
      {"PF ColorParamSuite", 1, &g_color_param_suite1},
      {"PF Param Utils Suite", 2, &g_param_utils_suite1},
      {"PF Param Utils Suite", 3, &g_param_utils_suite},
      {"AEGP PF Interface Suite", 1, &g_pf_interface_suite},
      {"AEGP World Suite", 3, nullptr, &provide_aegp_world_suite3},
      {"AEGP Layer Render Options Suite", 1, nullptr,
       &provide_layer_render_options1},
      {"AEGP Layer Render Options Suite", 2, nullptr,
       &provide_layer_render_options2},
      {"AEGP Render Options Suite", 1, nullptr, &provide_render_options1},
      {"AEGP Render Options Suite", 4, nullptr, &provide_render_options4,
       nullptr, &render_options4_provider_available},
      {"AEGP Render Suite", 2, nullptr, &provide_render_suite2, nullptr,
       &render_suite2_provider_available},
      {"AEGP Render Suite", 5, nullptr, &provide_render_suite5},
      {"AEGP Render Suite", 8, nullptr, &provide_render_suite8},
      {"AEGP Render Asyc Manager Suite", 1, nullptr,
       &provide_render_async_manager1},
      {"AEGP Mask Suite", 1, &g_pf_mask_suite1, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Layer Mask Suite", 6, &g_mask_suite5, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Layer Mask Suite", 7, &g_mask_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Stream Suite", 11, &g_stream_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Keyframe Suite", 5, &g_keyframe_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Dynamic Stream Suite", 5, &g_dynamic_stream_suite,
       nullptr, nullptr, &mask_suite_provider_available},
      {"AEGP Mask Outline Suite", 5, &g_mask_outline_suite,
       nullptr, nullptr, &mask_suite_provider_available},
      {"PF Color Suite", 1, &g_color_suite8},
      {"PF Color16 Suite", 1, &g_color_suite16},
      {"PF ColorFloat Suite", 1, &g_color_suite_float},
      {"PF Batch Sampling Suite", 1, nullptr, &provide_batch_sampling1},
      {"PF Path Query Suite", 1, nullptr, &provide_path_query1, nullptr,
       &mask_suite_provider_available},
      {"PF Path Data Suite", 1, nullptr, &provide_path_data1, nullptr,
       &mask_suite_provider_available},
      {"AEGP Duck Suite", 1, nullptr, &provide_duck1},
      {"AEGP Command Suite", 1, &g_aegp_command_suite, nullptr, nullptr,
       &aegp_init_suite_provider_available},
      {"AEGP Register Suite", 6, &g_aegp_register_suite, nullptr, nullptr,
       &aegp_init_suite_provider_available},
      {"PF Effect UI Suite", 1, nullptr, &provide_effect_ui1},
      {"PF AE Adv App Suite", 1, nullptr, &provide_adv_app1},
      {"PF AE Adv App Suite", 2, nullptr, &provide_adv_app2},
      {aexcompat::worker_runtime::suite_call_slot_probe::
           kPrivateEffectSuiteName,
       aexcompat::worker_runtime::suite_call_slot_probe::
           kPrivateEffectSuiteVersion3,
       nullptr,
       &aexcompat::worker_runtime::suite_call_slot_probe::
           provide_private_effect_probe3,
       nullptr,
       &aexcompat::worker_runtime::suite_call_slot_probe::
           private_effect_probe3_available},
      {aexcompat::worker_runtime::suite_call_slot_probe::
           kPrivateEffectSuiteName,
       aexcompat::worker_runtime::suite_call_slot_probe::
           kPrivateEffectSuiteVersion5,
       nullptr,
       &aexcompat::worker_runtime::suite_call_slot_probe::
           provide_private_effect_probe5,
       nullptr,
       &aexcompat::worker_runtime::suite_call_slot_probe::
           private_effect_probe5_available},
      {aexcompat::worker_runtime::compute_cache::kSuiteName,
       aexcompat::worker_runtime::compute_cache::kSuiteVersion1,
       nullptr,
       &aexcompat::worker_runtime::compute_cache::provide_suite1},
      {"DRAWBOT Draw Suite", 1, nullptr, &provide_drawbot_draw1},
      {"DRAWBOT Supplier Suite", 1, nullptr, &provide_drawbot_supplier1},
      {"DRAWBOT Surface Suite", 2, nullptr, &provide_drawbot_surface2},
      {"DRAWBOT Path Suite", 1, nullptr, &provide_drawbot_path1},
      {"PF Effect Custom UI Suite", 1, nullptr, &provide_custom_ui1},
      {"PF Effect Custom UI Suite", 2, nullptr, &provide_custom_ui2},
      {"PF Effect Custom UI Overlay Theme Suite", 1, nullptr,
       &provide_overlay_theme1},
      {"PF AE App Suite", 6, nullptr, &provide_app_suite4},
      {"PF AE App Suite", 7, nullptr, &provide_app_suite5},
      {"PF AE App Suite", 1, nullptr, &provide_app_suite6},
      {"PF AE Channel Suite", 1, &g_channel_suite1},
      {"PF Effect Sequence Data Suite", 1, &g_effect_sequence_data_suite1},
      {"PF Handle Suite", 2, &g_handle_suite},
      {"PF GPU Device Suite", 1, g_gpu_device_suite1.data()},
      {"PF ANSI Suite", 1, nullptr, &provide_ansi1},
      {"PF ANSI Suite", 2, nullptr, &provide_ansi2},
      {"PF AE Adv Item Suite", 1, &g_adv_item_suite1, nullptr, nullptr,
       &render_worker_suite_provider_available},
      {"PF Color Settings Suite", 3, nullptr, &provide_color_settings7},
      {"PF Color Settings Suite", 4, nullptr, &provide_color_settings7},
      {"PF Color Settings Suite", 6, nullptr, &provide_color_settings7},
      {"PF Color Settings Suite", 7, nullptr, &provide_color_settings7},
      {aexcompat::flt_blur::kSuiteName,
       aexcompat::flt_blur::kSuiteVersion1, nullptr, &provide_flt_blur1},
      {aexcompat::aefx_ace::kSuiteName,
       aexcompat::aefx_ace::kSuiteVersion1, nullptr, &provide_aefx_ace1},
      {"PF Iterate8 Suite", 1, nullptr, &provide_iterate8},
      {"PF Iterate8 Suite", 2, nullptr, &provide_iterate8},
      {"PF iterate16 Suite", 1, &g_iterate16_suite2},
      {"PF iterate16 Suite", 2, &g_iterate16_suite2},
      {"PF iterateFloat Suite", 1, &g_iterate_float_suite2},
      {"PF iterateFloat Suite", 2, &g_iterate_float_suite2},
      {"PF Sampling8 Suite", 1, nullptr, &provide_sampling8},
      {"PF Sampling16 Suite", 1, nullptr, &provide_sampling16},
      {"PF SamplingFloat Suite", 1, nullptr, &provide_sampling_float},
      {"PF World Transform Suite", 1, nullptr,
       &aexcompat::pf_world_transform::provide_world_transform1},
      {"PF Fill Matte Suite", 2, nullptr,
       &aexcompat::pf_world_transform::provide_fill_matte2},
      {"AEGP Dynamic Stream Suite", 2, nullptr, &provide_dynamic_stream2},
      {aexcompat::worker_runtime::persistent_data::kSuiteName,
       aexcompat::worker_runtime::persistent_data::kSuiteVersion3, nullptr,
       &aexcompat::worker_runtime::persistent_data::provide_suite3},
  };
  return configure_host_suite_catalog(
      {component_suites, std::size(component_suites),
       {&resolve_scene_suite_provider, nullptr}});
}

int32_t __cdecl acquire_suite(const char* name, int32_t version,
                              const void** suite) {
  using namespace aexcompat::worker_runtime::host_suites;
  static const bool configured = configure_component_suite_catalog();
  if (!configured) {
    if (suite) *suite = nullptr;
    return 4;
  }
  const int32_t result =
      acquire_catalog_suite(name, version, suite, g_trace_writer);
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:acquire_suite name=\""
              << (name ? name : "(null)") << "\" version=" << version
              << " -> " << result << "\n" << std::flush;
  return result;
}
int32_t __cdecl release_suite(const char* name, int32_t version) {
  return aexcompat::worker_runtime::host_suites::release_catalog_suite(
      name, version, g_trace_writer);
}

}  // namespace aexcompat::l2_detail
