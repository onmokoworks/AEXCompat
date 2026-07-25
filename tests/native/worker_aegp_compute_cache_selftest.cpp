#include "worker_aegp_compute_cache.hpp"
#include "worker_session.hpp"
#include "worker_suite_registry.hpp"

#include <atomic>
#include <cassert>
#include <condition_variable>
#include <cstdint>
#include <limits>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

// This target deliberately compiles the real WorkerSession implementation so
// the Compute Cache refusal test below crosses the production finish/unload
// boundary. The selftest target does not otherwise link worker_session.cpp.
#include "../../minihost/src/worker_session.cpp"

namespace cache = aexcompat::worker_runtime::compute_cache;
using aexcompat::worker_runtime::SuiteRegistry;
using aexcompat::worker_runtime::SuiteResolveResult;
using aexcompat::worker_runtime::WorkerSession;

namespace aexcompat::worker_runtime {

// WorkerSession's audit dependency is inert in this focused lifecycle test.
// The production worker links the real implementation; these definitions keep
// this target focused on the unload decision and its Win32 module ownership.
ModuleAuditReport& module_audit_report() noexcept {
  static ModuleAuditReport report;
  return report;
}

ModuleAuditSnapshot capture_module_audit() {
  return {};
}

bool module_audit_passed() {
  return true;
}

std::string module_audit_json() {
  return "{}";
}

void record_module_audit_epoch(uint32_t, ModuleAuditSnapshot,
                               ModuleAuditSnapshot) {}

}  // namespace aexcompat::worker_runtime

namespace {

struct Options {
  int32_t key{};
  int32_t value{};
  bool throw_key{};
  bool throw_compute{};
  bool throw_size{};
  bool null_value{};
  bool oversize{};
  bool reenter{};
  bool block{};
};

std::atomic<int> g_key_calls{};
std::atomic<int> g_compute_calls{};
std::atomic<int> g_size_calls{};
std::atomic<int> g_delete_calls{};
std::atomic<int32_t> g_reentrant_result{-1};
std::mutex g_block_mutex;
std::condition_variable g_block_changed;
bool g_compute_entered{};
bool g_release_compute{};

cache::AEGP_ComputeCacheSuite1 const* api() {
  const auto* value = cache::suite();
  assert(value);
  return value;
}

cache::A_Err __cdecl generate_key(
    cache::AEGP_CCComputeOptionsRefconP opaque,
    cache::AEGP_CCComputeKeyP output) {
  ++g_key_calls;
  if (!opaque || !output) return cache::kErrParameter;
  const auto& options = *static_cast<const Options*>(opaque);
  if (options.throw_key) throw std::runtime_error("key");
  output->bytes[0] = options.key;
  output->bytes[1] = options.key ^ 0x1357;
  output->bytes[2] = options.key ^ 0x2468;
  output->bytes[3] = options.key ^ 0x55aa;
  return cache::kErrNone;
}

cache::A_Err __cdecl compute(
    cache::AEGP_CCComputeOptionsRefconP opaque,
    cache::AEGP_CCComputeValueRefconP* output) {
  ++g_compute_calls;
  if (!opaque || !output) return cache::kErrParameter;
  const auto& options = *static_cast<const Options*>(opaque);
  if (options.throw_compute) throw std::runtime_error("compute");
  if (options.block) {
    std::unique_lock<std::mutex> lock(g_block_mutex);
    g_compute_entered = true;
    g_block_changed.notify_all();
    g_block_changed.wait(lock, [] { return g_release_compute; });
  }
  if (options.reenter) {
    void* nested{};
    g_reentrant_result = api()->AEGP_CheckoutCached(
        "reentrant", opaque, &nested);
    assert(!nested);
  }
  if (options.null_value) {
    *output = nullptr;
    return cache::kErrNone;
  }
  *output = new int(options.value);
  return cache::kErrNone;
}

std::size_t __cdecl approximate_size(cache::AEGP_CCComputeValueRefconP value) {
  ++g_size_calls;
  assert(value);
  const int encoded = *static_cast<const int*>(value);
  if (encoded == -1001) throw std::runtime_error("size");
  if (encoded == -1002) return cache::kMaxValueBytes + 1;
  return sizeof(int);
}

void __cdecl delete_value(cache::AEGP_CCComputeValueRefconP value) {
  ++g_delete_calls;
  delete static_cast<int*>(value);
}

cache::AEGP_ComputeCacheCallbacks callbacks{
    &generate_key, &compute, &approximate_size, &delete_value};

void reset_counts() {
  assert(cache::reset_for_selftest());
  g_key_calls = 0;
  g_compute_calls = 0;
  g_size_calls = 0;
  g_delete_calls = 0;
  g_reentrant_result = -1;
  std::lock_guard<std::mutex> lock(g_block_mutex);
  g_compute_entered = false;
  g_release_compute = false;
}

void check_value(void* receipt, int expected) {
  void* value{};
  assert(api()->AEGP_GetReceiptComputeValue(receipt, &value) ==
         cache::kErrNone);
  assert(value);
  assert(*static_cast<int*>(value) == expected);
}

void test_register_compute_hit_and_unregister() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("basic.v1", &callbacks) == cache::kErrNone);
  assert(api()->AEGP_ClassRegister("basic.v1", &callbacks) ==
         cache::kErrStruct);

  Options options{7, 42};
  void* receipt1{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "basic.v1", &options, true, &receipt1) == cache::kErrNone);
  assert(receipt1);
  check_value(receipt1, 42);
  assert(g_compute_calls == 1);

  void* receipt2{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "basic.v1", &options, true, &receipt2) == cache::kErrNone);
  assert(receipt2 && receipt2 != receipt1);
  assert(g_compute_calls == 1);
  check_value(receipt2, 42);

  void* receipt3{};
  assert(api()->AEGP_CheckoutCached("basic.v1", &options, &receipt3) ==
         cache::kErrNone);
  assert(receipt3);
  assert(g_compute_calls == 1);

  Options missing{8, 9};
  void* missing_receipt = reinterpret_cast<void*>(1);
  assert(api()->AEGP_CheckoutCached(
             "basic.v1", &missing, &missing_receipt) ==
         cache::kErrNotInCacheOrComputePending);
  assert(!missing_receipt);

  assert(api()->AEGP_ClassUnregister("basic.v1") == cache::kErrStruct);
  assert(api()->AEGP_CheckinComputeReceipt(receipt1) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(receipt2) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(receipt3) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(receipt1) == cache::kErrStruct);
  void* stale_value{};
  assert(api()->AEGP_GetReceiptComputeValue(receipt1, &stale_value) ==
         cache::kErrStruct);
  assert(!stale_value);
  assert(api()->AEGP_ClassUnregister("basic.v1") == cache::kErrNone);
  assert(g_delete_calls == 1);
  assert(api()->AEGP_ClassUnregister("basic.v1") == cache::kErrStruct);
}

void test_key_class_and_generation_isolation() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("version.v1", &callbacks) ==
         cache::kErrNone);
  assert(api()->AEGP_ClassRegister("version.v2", &callbacks) ==
         cache::kErrNone);
  Options first{1, 10};
  Options second{2, 20};
  void* a{};
  void* b{};
  void* c{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "version.v1", &first, true, &a) == cache::kErrNone);
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "version.v1", &second, true, &b) == cache::kErrNone);
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "version.v2", &first, true, &c) == cache::kErrNone);
  assert(g_compute_calls == 3);
  check_value(a, 10);
  check_value(b, 20);
  check_value(c, 10);
  assert(api()->AEGP_CheckinComputeReceipt(a) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(b) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(c) == cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("version.v1") == cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("version.v2") == cache::kErrNone);
  assert(g_delete_calls == 3);
}

void test_invalid_foreign_and_callback_failures() {
  reset_counts();
  std::string long_id(cache::kMaxClassIdBytes + 1, 'x');
  assert(api()->AEGP_ClassRegister(nullptr, &callbacks) ==
         cache::kErrParameter);
  assert(api()->AEGP_ClassRegister("", &callbacks) == cache::kErrParameter);
  assert(api()->AEGP_ClassRegister(long_id.c_str(), &callbacks) ==
         cache::kErrParameter);
  auto missing_callback = callbacks;
  missing_callback.compute = nullptr;
  assert(api()->AEGP_ClassRegister("bad.callbacks", &missing_callback) ==
         cache::kErrParameter);

  assert(api()->AEGP_ClassRegister("failures", &callbacks) ==
         cache::kErrNone);
  Options key_exception{1, 1, true};
  void* receipt = reinterpret_cast<void*>(1);
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &key_exception, true, &receipt) ==
         cache::kErrGeneric);
  assert(!receipt);

  Options compute_exception{2, 2};
  compute_exception.throw_compute = true;
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &compute_exception, true, &receipt) ==
         cache::kErrGeneric);
  assert(!receipt);

  Options null_value{3, 3};
  null_value.null_value = true;
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &null_value, true, &receipt) ==
         cache::kErrStruct);
  assert(!receipt);

  Options size_exception{4, -1001};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &size_exception, true, &receipt) ==
         cache::kErrGeneric);
  assert(!receipt);

  Options oversize{5, -1002};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &oversize, true, &receipt) ==
         cache::kErrAlloc);
  assert(!receipt);

  void* value = reinterpret_cast<void*>(1);
  assert(api()->AEGP_GetReceiptComputeValue(
             reinterpret_cast<void*>(0x1234), &value) == cache::kErrStruct);
  assert(!value);
  assert(api()->AEGP_CheckinComputeReceipt(
             reinterpret_cast<void*>(0x1234)) == cache::kErrStruct);
  assert(api()->AEGP_GetReceiptComputeValue(nullptr, &value) ==
         cache::kErrParameter);
  assert(api()->AEGP_CheckinComputeReceipt(nullptr) == cache::kErrParameter);
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "failures", &oversize, true, nullptr) == cache::kErrParameter);
  assert(api()->AEGP_CheckoutCached(
             "failures", &oversize, nullptr) == cache::kErrParameter);
  assert(api()->AEGP_ClassUnregister("foreign") == cache::kErrStruct);
  assert(api()->AEGP_ClassUnregister("failures") == cache::kErrNone);
  assert(g_delete_calls == 2);
}

void test_concurrency_and_reentrancy() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("concurrent", &callbacks) ==
         cache::kErrNone);
  Options options{71, 99};
  options.block = true;
  void* first{};
  void* waited{};
  cache::A_Err first_result = -1;
  cache::A_Err waited_result = -1;
  std::thread computing([&] {
    first_result = api()->AEGP_ComputeIfNeededAndCheckout(
        "concurrent", &options, true, &first);
  });
  {
    std::unique_lock<std::mutex> lock(g_block_mutex);
    g_block_changed.wait(lock, [] { return g_compute_entered; });
  }
  void* pending = reinterpret_cast<void*>(1);
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "concurrent", &options, false, &pending) ==
         cache::kErrNotInCacheOrComputePending);
  assert(!pending);
  std::thread waiting([&] {
    waited_result = api()->AEGP_ComputeIfNeededAndCheckout(
        "concurrent", &options, true, &waited);
  });
  {
    std::lock_guard<std::mutex> lock(g_block_mutex);
    g_release_compute = true;
  }
  g_block_changed.notify_all();
  computing.join();
  waiting.join();
  assert(first_result == cache::kErrNone);
  assert(waited_result == cache::kErrNone);
  assert(first && waited);
  assert(g_compute_calls == 1);
  assert(api()->AEGP_CheckinComputeReceipt(first) == cache::kErrNone);
  assert(api()->AEGP_CheckinComputeReceipt(waited) == cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("concurrent") == cache::kErrNone);

  assert(api()->AEGP_ClassRegister("reentrant", &callbacks) ==
         cache::kErrNone);
  Options reentrant{81, 101};
  reentrant.reenter = true;
  void* receipt{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "reentrant", &reentrant, true, &receipt) == cache::kErrNone);
  assert(g_reentrant_result == cache::kErrNotInCacheOrComputePending);
  assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("reentrant") == cache::kErrNone);
}

void test_bounds_and_global_cleanup() {
  reset_counts();
  std::vector<std::string> classes;
  for (std::size_t index = 0; index < cache::kMaxClasses; ++index) {
    classes.push_back("bounded." + std::to_string(index));
    assert(api()->AEGP_ClassRegister(classes.back().c_str(), &callbacks) ==
           cache::kErrNone);
  }
  assert(api()->AEGP_ClassRegister("bounded.overflow", &callbacks) ==
         cache::kErrAlloc);
  for (const std::string& id : classes)
    assert(api()->AEGP_ClassUnregister(id.c_str()) == cache::kErrNone);

  assert(api()->AEGP_ClassRegister("entry.bounds", &callbacks) ==
         cache::kErrNone);
  Options options{};
  for (std::size_t index = 0; index < cache::kMaxEntries; ++index) {
    options.key = static_cast<int32_t>(index);
    options.value = static_cast<int32_t>(index);
    void* receipt{};
    assert(api()->AEGP_ComputeIfNeededAndCheckout(
               "entry.bounds", &options, true, &receipt) == cache::kErrNone);
    assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  }
  options.key = static_cast<int32_t>(cache::kMaxEntries);
  void* overflow{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "entry.bounds", &options, true, &overflow) == cache::kErrAlloc);
  assert(!overflow);
  assert(api()->AEGP_ClassUnregister("entry.bounds") == cache::kErrNone);

  assert(api()->AEGP_ClassRegister("teardown", &callbacks) ==
         cache::kErrNone);
  Options cleanup{1, 17};
  void* receipt{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "teardown", &cleanup, true, &receipt) == cache::kErrNone);
  assert(cache::unload_safe());
  assert(!cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(!cache::unload_safe());
  assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  const int before = g_delete_calls.load();
  assert(cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  // The refusal is process-sticky: a later callback cannot retroactively make
  // unloading safe in this worker.
  assert(!cache::unload_safe());
  assert(g_delete_calls == before + 1);
  assert(api()->AEGP_ClassUnregister("teardown") == cache::kErrStruct);

  reset_counts();
  assert(cache::unload_safe());
  assert(api()->AEGP_ClassRegister("teardown.clean", &callbacks) ==
         cache::kErrNone);
  assert(cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(cache::unload_safe());
}

void assert_worker_session_finish_preserves_unique_module() {
  wchar_t temporary_directory[MAX_PATH]{};
  assert(GetTempPathW(static_cast<DWORD>(std::size(temporary_directory)),
                      temporary_directory) != 0);
  wchar_t temporary_name[MAX_PATH]{};
  assert(GetTempFileNameW(temporary_directory, L"aex", 0, temporary_name) != 0);
  assert(DeleteFileW(temporary_name) != FALSE);
  const std::filesystem::path module_path =
      std::filesystem::path(temporary_name).concat(L".dll");

  wchar_t system_directory[MAX_PATH]{};
  assert(GetSystemDirectoryW(system_directory,
                             static_cast<UINT>(std::size(system_directory))) !=
         0);
  const std::filesystem::path source_module =
      std::filesystem::path(system_directory) / L"version.dll";
  assert(CopyFileW(source_module.c_str(), module_path.c_str(), TRUE) != FALSE);
  HMODULE module = LoadLibraryW(module_path.c_str());
  assert(module);
  const std::wstring module_basename = module_path.filename().wstring();

  aexcompat::worker_runtime::RuntimeContext context;
  context.plugin_path = module_path;
  context.module = module;
  {
    WorkerSession session(context, nullptr, nullptr);
    assert(!context.module);
    assert(session.finish(0) == 14);
    // This is the actual production finish()/unload_module() boundary. If it
    // called FreeLibrary despite the rejected teardown, the unique module copy
    // would no longer be present.
    assert(GetModuleHandleW(module_basename.c_str()) == module);
  }
  assert(GetModuleHandleW(module_basename.c_str()) == module);

  assert(FreeLibrary(module) != FALSE);
  assert(GetModuleHandleW(module_basename.c_str()) == nullptr);
  assert(DeleteFileW(module_path.c_str()) != FALSE);
}

void test_worker_session_finish_refuses_unload_with_live_receipt() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("session.unload.receipt", &callbacks) ==
         cache::kErrNone);
  Options options{101, 202};
  void* receipt{};
  assert(api()->AEGP_ComputeIfNeededAndCheckout(
             "session.unload.receipt", &options, true, &receipt) ==
         cache::kErrNone);
  assert(receipt);

  // A live receipt makes owner teardown unsafe and permanently closes the
  // process-wide unload gate before WorkerSession reaches its finish path.
  assert(!cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(!cache::unload_safe());
  assert_worker_session_finish_preserves_unique_module();

  assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  assert(cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(!cache::unload_safe());
  reset_counts();
}

void test_worker_session_finish_refuses_unload_with_inflight_compute() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("session.unload.compute", &callbacks) ==
         cache::kErrNone);
  Options options{303, 404};
  options.block = true;
  void* receipt{};
  cache::A_Err result = -1;
  std::thread computing([&] {
    result = api()->AEGP_ComputeIfNeededAndCheckout(
        "session.unload.compute", &options, true, &receipt);
  });
  {
    std::unique_lock<std::mutex> lock(g_block_mutex);
    g_block_changed.wait(lock, [] { return g_compute_entered; });
  }

  assert(!cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(!cache::unload_safe());
  assert_worker_session_finish_preserves_unique_module();

  {
    std::lock_guard<std::mutex> lock(g_block_mutex);
    g_release_compute = true;
  }
  g_block_changed.notify_all();
  computing.join();
  assert(result == cache::kErrNone);
  assert(receipt);
  assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  assert(cache::teardown_owner_from_entry(
      reinterpret_cast<const void*>(&generate_key)));
  assert(!cache::unload_safe());
  reset_counts();
}

SuiteResolveResult resolve_compute_cache(
    void*, const char* name, int32_t version, const void** suite) {
  if (!name || !suite) return SuiteResolveResult::rejected_bad_param;
  if (std::string(name) != cache::kSuiteName ||
      version != cache::kSuiteVersion1)
    return SuiteResolveResult::not_found;
  *suite = cache::suite();
  return SuiteResolveResult::acquired;
}

void test_acquire_release_balance_and_telemetry() {
  reset_counts();
  SuiteRegistry registry;
  const void* table{};
  assert(registry.acquire(cache::kSuiteName, cache::kSuiteVersion1, &table,
                          &resolve_compute_cache, nullptr, nullptr) == 0);
  assert(table == cache::suite());
  assert(registry.acquire(cache::kSuiteName, cache::kSuiteVersion1, &table,
                          &resolve_compute_cache, nullptr, nullptr) == 0);
  assert(!registry.balanced());
  assert(registry.release(cache::kSuiteName, cache::kSuiteVersion1, nullptr) ==
         0);
  assert(registry.release(cache::kSuiteName, cache::kSuiteVersion1, nullptr) ==
         0);
  assert(registry.balanced());

  assert(api()->AEGP_ClassRegister("telemetry", &callbacks) ==
         cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("telemetry") == cache::kErrNone);
  const std::string report = cache::telemetry_report_json();
  assert(report.find("\"maximum_records\":128") != std::string::npos);
  assert(report.find("\"slot\":0") != std::string::npos);
  assert(report.find("\"slot\":1") != std::string::npos);
  assert(report.find("\"truncated\":false") != std::string::npos);
}

void test_receipt_and_telemetry_bounds() {
  reset_counts();
  assert(api()->AEGP_ClassRegister("receipt.bounds", &callbacks) ==
         cache::kErrNone);
  Options options{91, 111};
  std::vector<void*> receipts;
  receipts.reserve(cache::kMaxActiveReceipts);
  for (std::size_t index = 0; index < cache::kMaxActiveReceipts; ++index) {
    void* receipt{};
    assert(api()->AEGP_ComputeIfNeededAndCheckout(
               "receipt.bounds", &options, true, &receipt) ==
           cache::kErrNone);
    assert(receipt);
    receipts.push_back(receipt);
  }
  void* overflow{};
  assert(api()->AEGP_CheckoutCached(
             "receipt.bounds", &options, &overflow) == cache::kErrAlloc);
  assert(!overflow);
  for (void* receipt : receipts)
    assert(api()->AEGP_CheckinComputeReceipt(receipt) == cache::kErrNone);
  assert(api()->AEGP_ClassUnregister("receipt.bounds") == cache::kErrNone);

  cache::reset_telemetry();
  assert(api()->AEGP_ClassRegister("telemetry.bounds", &callbacks) ==
         cache::kErrNone);
  Options missing{92, 112};
  for (std::size_t index = 0; index <= cache::kMaxTelemetryRecords; ++index) {
    const char* selector =
        (index & 1u) == 0 ? "PARAMS_SETUP" : "GLOBAL_SETUP";
    aexcompat::worker_runtime::set_suite_timeline_selector(selector);
    void* receipt{};
    assert(api()->AEGP_CheckoutCached(
               "telemetry.bounds", &missing, &receipt) ==
           cache::kErrNotInCacheOrComputePending);
    assert(!receipt);
  }
  aexcompat::worker_runtime::set_suite_timeline_selector("HOST");
  const std::string report = cache::telemetry_report_json();
  assert(report.find("\"maximum_records\":128") != std::string::npos);
  assert(report.find("\"truncated\":true") != std::string::npos);
  assert(api()->AEGP_ClassUnregister("telemetry.bounds") == cache::kErrNone);
}

}  // namespace

int main() {
  test_register_compute_hit_and_unregister();
  test_key_class_and_generation_isolation();
  test_invalid_foreign_and_callback_failures();
  test_concurrency_and_reentrancy();
  test_bounds_and_global_cleanup();
  test_worker_session_finish_refuses_unload_with_live_receipt();
  test_worker_session_finish_refuses_unload_with_inflight_compute();
  test_acquire_release_balance_and_telemetry();
  test_receipt_and_telemetry_bounds();
  assert(cache::reset_for_selftest());
  return 0;
}
