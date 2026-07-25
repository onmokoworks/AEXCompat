#include "worker_compute_cache_suite.hpp"

#include <cstring>
#include <deque>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <vector>

namespace aexcompat::compute_cache {
namespace {

// A_Err values (A.h): the suite's error domain is the AEGP one, not PF_Err.
constexpr int32_t kErrNone = 0;
constexpr int32_t kErrGeneric = 1;
constexpr int32_t kErrStruct = 2;
constexpr int32_t kErrParameter = 3;
constexpr int32_t kErrNotInCacheOrComputePending = 22;

constexpr std::size_t kMaxClassIdBytes = 256;
constexpr std::size_t kMaxRegisteredClasses = 64;
constexpr std::size_t kMaxCachedValuesPerClass = 64;

struct CacheKey {
  std::string compute_class;
  AegpGuid key{};
  bool operator<(const CacheKey& other) const {
    if (compute_class != other.compute_class)
      return compute_class < other.compute_class;
    return std::memcmp(key.bytes, other.key.bytes, sizeof(key.bytes)) < 0;
  }
};

struct CacheEntry {
  void* value = nullptr;
};

struct ClassRecord {
  AegpComputeCacheCallbacks callbacks{};
  std::deque<CacheKey> insertion_order;
};

struct Receipt {
  CacheKey cache_key;
  void* value = nullptr;
};

std::mutex g_mutex;
std::map<std::string, ClassRecord> g_classes;
std::map<CacheKey, CacheEntry> g_cache;
std::set<Receipt*> g_live_receipts;

bool valid_class_id(const char* compute_class) {
  return compute_class &&
         strnlen_s(compute_class, kMaxClassIdBytes) < kMaxClassIdBytes;
}

void delete_cached_value(const ClassRecord& record, void* value) {
  if (value && record.callbacks.delete_compute_value)
    record.callbacks.delete_compute_value(value);
}

// Caller must hold g_mutex. Evicts the oldest entries of the class while the
// class exceeds the per-class bound, mirroring the real host's purge
// heuristic (approx_size_value feeds the real host's byte accounting; the
// worker bounds by entry count instead).
void evict_overflow(const std::string& compute_class, ClassRecord& record) {
  while (record.insertion_order.size() > kMaxCachedValuesPerClass) {
    const CacheKey& oldest = record.insertion_order.front();
    const auto found = g_cache.find(oldest);
    if (found != g_cache.end()) {
      delete_cached_value(record, found->second.value);
      g_cache.erase(found);
    }
    record.insertion_order.pop_front();
  }
}

int32_t checkout_hit(const CacheKey& cache_key, void* value,
                     void** compute_receipt) {
  auto* receipt = new (std::nothrow) Receipt{cache_key, value};
  if (!receipt) return kErrGeneric;
  g_live_receipts.insert(receipt);
  *compute_receipt = receipt;
  return kErrNone;
}

int32_t __cdecl class_register_impl(
    const char* compute_class, const AegpComputeCacheCallbacks* callbacks) {
  if (!valid_class_id(compute_class) || !callbacks ||
      !callbacks->generate_key || !callbacks->compute ||
      !callbacks->approx_size_value || !callbacks->delete_compute_value)
    return kErrParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_classes.count(compute_class)) return kErrStruct;
  if (g_classes.size() >= kMaxRegisteredClasses) return kErrGeneric;
  ClassRecord record;
  record.callbacks = *callbacks;
  g_classes.emplace(compute_class, record);
  return kErrNone;
}

int32_t __cdecl class_unregister_impl(const char* compute_class) {
  if (!valid_class_id(compute_class)) return kErrParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_classes.find(compute_class);
  if (found == g_classes.end()) return kErrStruct;
  for (const CacheKey& cache_key : found->second.insertion_order) {
    const auto cached = g_cache.find(cache_key);
    if (cached != g_cache.end()) {
      delete_cached_value(found->second, cached->second.value);
      g_cache.erase(cached);
    }
  }
  g_classes.erase(found);
  return kErrNone;
}

// Shared checkout path: resolve the class, hash the key, and (when allowed)
// compute on a miss. Returns kErrNotInCacheOrComputePending on a cache miss
// when compute_may_run is false.
int32_t checkout_common(const char* compute_class, void* opaque_options,
                        bool compute_may_run, void** compute_receipt) {
  if (!valid_class_id(compute_class) || !compute_receipt)
    return kErrParameter;
  *compute_receipt = nullptr;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_classes.find(compute_class);
  if (found == g_classes.end()) return kErrStruct;
  ClassRecord& record = found->second;
  AegpGuid key{};
  const int32_t key_error =
      record.callbacks.generate_key(opaque_options, &key);
  if (key_error != kErrNone) return key_error;
  CacheKey cache_key{compute_class, key};
  const auto cached = g_cache.find(cache_key);
  if (cached != g_cache.end())
    return checkout_hit(cache_key, cached->second.value, compute_receipt);
  if (!compute_may_run) return kErrNotInCacheOrComputePending;
  void* value = nullptr;
  const int32_t compute_error =
      record.callbacks.compute(opaque_options, &value);
  if (compute_error != kErrNone) return compute_error;
  g_cache[cache_key] = CacheEntry{value};
  record.insertion_order.push_back(cache_key);
  evict_overflow(compute_class, record);
  return checkout_hit(cache_key, value, compute_receipt);
}

int32_t __cdecl compute_if_needed_and_checkout_impl(
    const char* compute_class, void* opaque_options,
    uint8_t wait_for_other_thread, void** compute_receipt) {
  // The worker is single-threaded on the selector path, so there is never a
  // competing compute to wait for: both wait states degenerate to "compute
  // and checkout" per the suite's state table.
  (void)wait_for_other_thread;
  return checkout_common(compute_class, opaque_options,
                         /*compute_may_run=*/true, compute_receipt);
}

int32_t __cdecl checkout_cached_impl(const char* compute_class,
                                     void* opaque_options,
                                     void** compute_receipt) {
  return checkout_common(compute_class, opaque_options,
                         /*compute_may_run=*/false, compute_receipt);
}

int32_t __cdecl get_receipt_compute_value_impl(const void* compute_receipt,
                                               void** compute_value) {
  if (!compute_receipt || !compute_value) return kErrParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  const auto* receipt = static_cast<const Receipt*>(compute_receipt);
  if (!g_live_receipts.count(const_cast<Receipt*>(receipt)))
    return kErrParameter;
  *compute_value = receipt->value;
  return kErrNone;
}

int32_t __cdecl checkin_compute_receipt_impl(void* compute_receipt) {
  if (!compute_receipt) return kErrParameter;
  std::lock_guard<std::mutex> lock(g_mutex);
  auto* receipt = static_cast<Receipt*>(compute_receipt);
  const auto found = g_live_receipts.find(receipt);
  if (found == g_live_receipts.end()) return kErrParameter;
  g_live_receipts.erase(found);
  // The value stays in the cache; only the receipt lease ends.
  delete receipt;
  return kErrNone;
}

}  // namespace

AegpComputeCacheSuite1& suite_table() {
  static AegpComputeCacheSuite1 table = {
      &class_register_impl,
      &class_unregister_impl,
      &compute_if_needed_and_checkout_impl,
      &checkout_cached_impl,
      &get_receipt_compute_value_impl,
      &checkin_compute_receipt_impl,
  };
  return table;
}

void purge_registry() {
  std::lock_guard<std::mutex> lock(g_mutex);
  for (const auto& [compute_class, record] : g_classes) {
    for (const CacheKey& cache_key : record.insertion_order) {
      const auto cached = g_cache.find(cache_key);
      if (cached != g_cache.end())
        delete_cached_value(record, cached->second.value);
    }
  }
  g_cache.clear();
  g_classes.clear();
  for (Receipt* receipt : g_live_receipts) delete receipt;
  g_live_receipts.clear();
}

namespace {
int32_t __cdecl fake_generate_key(void* options, AegpGuid* out_key) {
  if (!options || !out_key) return kErrParameter;
  out_key->bytes[0] = *static_cast<int32_t*>(options);
  return kErrNone;
}
int32_t __cdecl fake_compute(void* options, void** out_value) {
  if (!options || !out_value) return kErrParameter;
  *out_value = options;
  return kErrNone;
}
size_t __cdecl fake_approx_size(void*) { return sizeof(int32_t); }
void __cdecl fake_delete(void*) {}
}  // namespace

bool selftest() {
  purge_registry();
  const AegpComputeCacheCallbacks callbacks = {
      &fake_generate_key, &fake_compute, &fake_approx_size, &fake_delete};
  int32_t options = 7;
  void* receipt = nullptr;
  void* value = nullptr;
  bool ok = class_register_impl("aexcompat.selftest.cache_v_1", &callbacks) ==
                kErrNone &&
            class_register_impl("aexcompat.selftest.cache_v_1", &callbacks) ==
                kErrStruct &&
            checkout_cached_impl("aexcompat.selftest.cache_v_1", &options,
                                 &receipt) ==
                kErrNotInCacheOrComputePending &&
            receipt == nullptr &&
            compute_if_needed_and_checkout_impl(
                "aexcompat.selftest.cache_v_1", &options, 1, &receipt) ==
                kErrNone &&
            receipt != nullptr &&
            get_receipt_compute_value_impl(receipt, &value) == kErrNone &&
            value == &options &&
            checkin_compute_receipt_impl(receipt) == kErrNone &&
            checkin_compute_receipt_impl(receipt) == kErrParameter;
  receipt = nullptr;
  ok = ok &&
       checkout_cached_impl("aexcompat.selftest.cache_v_1", &options,
                            &receipt) == kErrNone &&
       receipt != nullptr &&
       checkin_compute_receipt_impl(receipt) == kErrNone &&
       class_unregister_impl("aexcompat.selftest.cache_v_1") == kErrNone &&
       checkout_cached_impl("aexcompat.selftest.cache_v_1", &options,
                            &receipt) == kErrStruct;
  purge_registry();
  return ok;
}

}  // namespace aexcompat::compute_cache
