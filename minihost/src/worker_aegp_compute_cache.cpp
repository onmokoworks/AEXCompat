#include "worker_aegp_compute_cache.hpp"

#include "worker_selector_dispatch.hpp"
#include "worker_suite_registry.hpp"

#include <windows.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <condition_variable>
#include <cstring>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

namespace aexcompat::worker_runtime::compute_cache {
namespace {

struct ComputeKeyLess {
  bool operator()(const AEGP_CCComputeKey& left,
                  const AEGP_CCComputeKey& right) const noexcept {
    return std::lexicographical_compare(
        std::begin(left.bytes), std::end(left.bytes),
        std::begin(right.bytes), std::end(right.bytes));
  }
};

enum class EntryState : uint8_t {
  computing,
  ready,
  failed,
};

struct CacheEntry {
  EntryState state{EntryState::computing};
  std::thread::id computing_thread;
  AEGP_CCComputeValueRefconP value{};
  std::size_t approximate_size{};
  std::size_t checkout_count{};
  A_Err failure{kErrGeneric};
  std::condition_variable changed;
};

struct ClassRecord {
  std::string id;
  uint64_t generation{};
  HMODULE owner{};
  AEGP_ComputeCacheCallbacks callbacks{};
  std::map<AEGP_CCComputeKey, std::shared_ptr<CacheEntry>, ComputeKeyLess>
      entries;
  std::size_t live_receipts{};
  bool retired{};
};

struct ReceiptToken {
  uint64_t generation{};
};

struct ReceiptRecord {
  ReceiptToken* token{};
  uint64_t generation{};
  uint64_t class_generation{};
  std::shared_ptr<ClassRecord> owner;
  std::shared_ptr<CacheEntry> entry;
  bool active{};
};

struct DeleteWork {
  DeleteComputeValue callback{};
  AEGP_CCComputeValueRefconP value{};
};

struct Runtime {
  std::mutex mutex;
  std::unordered_map<std::string, std::shared_ptr<ClassRecord>> classes;
  std::unordered_map<uintptr_t, ReceiptRecord> receipts;
  std::vector<std::unique_ptr<ReceiptToken>> receipt_tokens;
  uint64_t next_class_generation{1};
  uint64_t next_receipt_generation{1};
  std::size_t entry_count{};
  std::size_t active_receipts{};
  std::size_t total_value_bytes{};
};

enum class Outcome : uint8_t {
  registered,
  unregistered,
  computed,
  cache_hit,
  cache_miss,
  compute_pending,
  value_returned,
  checked_in,
  invalid,
  callback_failure,
  capacity_failure,
  cleanup,
  cleanup_deferred,
};

struct TelemetryRecord {
  uint32_t sequence{};
  std::string selector;
  uint32_t slot{};
  std::string operation;
  Outcome outcome{Outcome::invalid};
  A_Err return_code{};
  uint32_t call_count{1};
};

struct Telemetry {
  std::mutex mutex;
  std::vector<TelemetryRecord> records;
  bool truncated{};
};

Runtime& runtime() {
  static Runtime value;
  return value;
}

Telemetry& telemetry() {
  static Telemetry value;
  return value;
}

std::atomic_bool& unload_safe_state() {
  static std::atomic_bool value{true};
  return value;
}

void reject_unload() noexcept {
  unload_safe_state().store(false, std::memory_order_release);
}

const char* outcome_name(Outcome outcome) noexcept {
  switch (outcome) {
    case Outcome::registered:
      return "registered";
    case Outcome::unregistered:
      return "unregistered";
    case Outcome::computed:
      return "computed";
    case Outcome::cache_hit:
      return "cache_hit";
    case Outcome::cache_miss:
      return "cache_miss";
    case Outcome::compute_pending:
      return "compute_pending";
    case Outcome::value_returned:
      return "value_returned";
    case Outcome::checked_in:
      return "checked_in";
    case Outcome::invalid:
      return "invalid";
    case Outcome::callback_failure:
      return "callback_failure";
    case Outcome::capacity_failure:
      return "capacity_failure";
    case Outcome::cleanup:
      return "cleanup";
    case Outcome::cleanup_deferred:
      return "cleanup_deferred";
  }
  return "invalid";
}

void record(uint32_t slot, const char* operation, Outcome outcome,
            A_Err return_code) noexcept {
  try {
    Telemetry& state = telemetry();
    const char* selector = current_suite_timeline_selector();
    const std::string owned_selector =
        selector && selector[0] ? selector : "HOST";
    std::lock_guard<std::mutex> lock(state.mutex);
    if (!state.records.empty()) {
      TelemetryRecord& previous = state.records.back();
      if (previous.selector == owned_selector && previous.slot == slot &&
          previous.operation == operation && previous.outcome == outcome &&
          previous.return_code == return_code) {
        if (previous.call_count != std::numeric_limits<uint32_t>::max())
          ++previous.call_count;
        return;
      }
    }
    if (state.records.size() >= kMaxTelemetryRecords) {
      state.truncated = true;
      return;
    }
    state.records.push_back(
        {static_cast<uint32_t>(state.records.size()), owned_selector, slot,
         operation, outcome, return_code, 1});
  } catch (...) {
    try {
      Telemetry& state = telemetry();
      std::lock_guard<std::mutex> lock(state.mutex);
      state.truncated = true;
    } catch (...) {
    }
  }
}

bool copy_class_id_seh(const char* source,
                       std::array<char, kMaxClassIdBytes + 1>& output,
                       std::size_t& length) noexcept {
  if (!source) return false;
  __try {
    for (std::size_t index = 0; index <= kMaxClassIdBytes; ++index) {
      const char value = source[index];
      if (value == '\0') {
        length = index;
        return index != 0;
      }
      output[index] = value;
    }
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
  return false;
}

bool copy_callbacks_seh(const AEGP_ComputeCacheCallbacks* source,
                        AEGP_ComputeCacheCallbacks& output) noexcept {
  if (!source) return false;
  __try {
    output = *source;
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

bool store_pointer_seh(void** output, void* value) noexcept {
  if (!output) return false;
  __try {
    *output = value;
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

bool class_id(const char* source, std::string& output) {
  std::array<char, kMaxClassIdBytes + 1> buffer{};
  std::size_t length{};
  if (!copy_class_id_seh(source, buffer, length)) return false;
  try {
    output.assign(buffer.data(), length);
    return true;
  } catch (...) {
    return false;
  }
}

HMODULE module_from_address(const void* address) noexcept {
  if (!address) return nullptr;
  HMODULE module{};
  return GetModuleHandleExW(
             GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                 GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
             reinterpret_cast<LPCWSTR>(address), &module)
      ? module
      : nullptr;
}

HMODULE callback_owner(const AEGP_ComputeCacheCallbacks& callbacks) noexcept {
  const std::array<const void*, 4> addresses{{
      reinterpret_cast<const void*>(callbacks.generate_key),
      reinterpret_cast<const void*>(callbacks.compute),
      reinterpret_cast<const void*>(callbacks.approx_size_value),
      reinterpret_cast<const void*>(callbacks.delete_compute_value),
  }};
  for (const void* address : addresses) {
    if (!module_from_address(address)) return nullptr;
  }
  HMODULE owner = static_cast<HMODULE>(active_selector_module());
  return owner ? owner : module_from_address(addresses.front());
}

A_Err invoke_generate_key_seh(GenerateKey callback, void* options,
                              AEGP_CCComputeKey* key) noexcept {
  __try {
    return callback(options, key);
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return kErrGeneric;
  }
}

A_Err invoke_compute_seh(Compute callback, void* options,
                         void** value) noexcept {
  __try {
    return callback(options, value);
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return kErrGeneric;
  }
}

bool invoke_size_seh(ApproxSizeValue callback, void* value,
                     std::size_t& size) noexcept {
  __try {
    size = callback(value);
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

bool invoke_delete_seh(DeleteComputeValue callback, void* value) noexcept {
  __try {
    callback(value);
    return true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

A_Err invoke_generate_key(GenerateKey callback, void* options,
                          AEGP_CCComputeKey* key) noexcept {
  try {
    return invoke_generate_key_seh(callback, options, key);
  } catch (...) {
    return kErrGeneric;
  }
}

A_Err invoke_compute(Compute callback, void* options, void** value) noexcept {
  try {
    return invoke_compute_seh(callback, options, value);
  } catch (...) {
    return kErrGeneric;
  }
}

bool invoke_size(ApproxSizeValue callback, void* value,
                 std::size_t& size) noexcept {
  try {
    return invoke_size_seh(callback, value, size);
  } catch (...) {
    return false;
  }
}

void invoke_delete(DeleteComputeValue callback, void* value) noexcept {
  if (!callback || !value) return;
  try {
    static_cast<void>(invoke_delete_seh(callback, value));
  } catch (...) {
  }
}

bool caller_matches(const ClassRecord& record) noexcept {
  HMODULE caller = static_cast<HMODULE>(active_selector_module());
  return !caller || caller == record.owner;
}

std::shared_ptr<ClassRecord> lookup_class_locked(
    Runtime& state, const std::string& id) {
  const auto found = state.classes.find(id);
  if (found == state.classes.end() || found->second->retired ||
      !caller_matches(*found->second))
    return {};
  return found->second;
}

A_Err make_receipt_locked(Runtime& state,
                          const std::shared_ptr<ClassRecord>& owner,
                          const std::shared_ptr<CacheEntry>& entry,
                          void** receipt) {
  if (state.active_receipts >= kMaxActiveReceipts ||
      state.receipt_tokens.size() >= kMaxReceiptTokens ||
      state.next_receipt_generation == 0 ||
      state.next_receipt_generation == std::numeric_limits<uint64_t>::max())
    return kErrAlloc;
  std::unique_ptr<ReceiptToken> token;
  try {
    token = std::make_unique<ReceiptToken>();
    token->generation = state.next_receipt_generation++;
    state.receipt_tokens.push_back(std::move(token));
    ReceiptToken* const stored_token = state.receipt_tokens.back().get();
    ReceiptRecord record_value{
        stored_token, stored_token->generation, owner->generation, owner,
        entry, true};
    const uintptr_t identity = reinterpret_cast<uintptr_t>(stored_token);
    try {
      const auto inserted = state.receipts.emplace(identity, record_value);
      if (!inserted.second) {
        state.receipt_tokens.pop_back();
        return kErrAlloc;
      }
    } catch (...) {
      state.receipt_tokens.pop_back();
      return kErrAlloc;
    }
    ++state.active_receipts;
    ++owner->live_receipts;
    ++entry->checkout_count;
    *receipt = reinterpret_cast<void*>(identity);
    return kErrNone;
  } catch (...) {
    return kErrAlloc;
  }
}

void collect_class_values_locked(Runtime& state,
                                 const std::shared_ptr<ClassRecord>& record_value,
                                 std::vector<DeleteWork>& work) {
  for (auto& item : record_value->entries) {
    CacheEntry& entry = *item.second;
    if (entry.state == EntryState::ready && entry.value) {
      work.push_back({record_value->callbacks.delete_compute_value, entry.value});
      if (entry.approximate_size <= state.total_value_bytes)
        state.total_value_bytes -= entry.approximate_size;
      else
        state.total_value_bytes = 0;
      entry.value = nullptr;
      entry.approximate_size = 0;
    }
    if (state.entry_count != 0) --state.entry_count;
  }
  record_value->entries.clear();
}

void run_delete_work(std::vector<DeleteWork>& work) noexcept {
  for (const DeleteWork& item : work)
    invoke_delete(item.callback, item.value);
}

A_Err __cdecl class_register(
    AEGP_CCComputeClassIdP compute_class,
    const AEGP_ComputeCacheCallbacks* callbacks_pointer) noexcept {
  std::string id;
  AEGP_ComputeCacheCallbacks callbacks{};
  if (!class_id(compute_class, id) ||
      !copy_callbacks_seh(callbacks_pointer, callbacks) ||
      !callbacks.generate_key || !callbacks.compute ||
      !callbacks.approx_size_value || !callbacks.delete_compute_value) {
    record(0, "class_register", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  const HMODULE owner = callback_owner(callbacks);
  HMODULE caller = static_cast<HMODULE>(active_selector_module());
  if (!owner || (caller && caller != owner)) {
    record(0, "class_register", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }
  Runtime& state = runtime();
  try {
    std::lock_guard<std::mutex> lock(state.mutex);
    if (state.classes.find(id) != state.classes.end()) {
      record(0, "class_register", Outcome::invalid, kErrStruct);
      return kErrStruct;
    }
    if (state.classes.size() >= kMaxClasses ||
        state.next_class_generation == 0 ||
        state.next_class_generation == std::numeric_limits<uint64_t>::max()) {
      record(0, "class_register", Outcome::capacity_failure, kErrAlloc);
      return kErrAlloc;
    }
    auto registered = std::make_shared<ClassRecord>();
    registered->id = id;
    registered->generation = state.next_class_generation++;
    registered->owner = owner;
    registered->callbacks = callbacks;
    state.classes.emplace(id, std::move(registered));
  } catch (...) {
    record(0, "class_register", Outcome::capacity_failure, kErrAlloc);
    return kErrAlloc;
  }
  record(0, "class_register", Outcome::registered, kErrNone);
  return kErrNone;
}

A_Err __cdecl class_unregister(
    AEGP_CCComputeClassIdP compute_class) noexcept {
  std::string id;
  if (!class_id(compute_class, id)) {
    record(1, "class_unregister", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  Runtime& state = runtime();
  std::vector<DeleteWork> work;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    const auto found = state.classes.find(id);
    if (found == state.classes.end() || found->second->retired ||
        !caller_matches(*found->second)) {
      record(1, "class_unregister", Outcome::invalid, kErrStruct);
      return kErrStruct;
    }
    const std::shared_ptr<ClassRecord> owner = found->second;
    const bool computing = std::any_of(
        owner->entries.begin(), owner->entries.end(), [](const auto& item) {
          return item.second->state == EntryState::computing;
        });
    if (computing || owner->live_receipts != 0) {
      record(1, "class_unregister", Outcome::cleanup_deferred, kErrStruct);
      return kErrStruct;
    }
    const std::size_t ready_count = static_cast<std::size_t>(std::count_if(
        owner->entries.begin(), owner->entries.end(), [](const auto& item) {
          return item.second->state == EntryState::ready &&
                 item.second->value != nullptr;
        }));
    try {
      work.reserve(ready_count);
    } catch (...) {
      record(1, "class_unregister", Outcome::capacity_failure, kErrAlloc);
      return kErrAlloc;
    }
    owner->retired = true;
    collect_class_values_locked(state, owner, work);
    state.classes.erase(found);
  }
  run_delete_work(work);
  record(1, "class_unregister", Outcome::unregistered, kErrNone);
  return kErrNone;
}

A_Err compute_or_checkout(AEGP_CCComputeClassIdP compute_class,
                          AEGP_CCComputeOptionsRefconP options,
                          bool wait_for_other_thread,
                          AEGP_CCCheckoutReceiptP* receipt_pointer) noexcept {
  if (!store_pointer_seh(receipt_pointer, nullptr)) {
    record(2, "compute_if_needed_and_checkout", Outcome::invalid,
           kErrParameter);
    return kErrParameter;
  }
  std::string id;
  if (!class_id(compute_class, id)) {
    record(2, "compute_if_needed_and_checkout", Outcome::invalid,
           kErrParameter);
    return kErrParameter;
  }

  Runtime& state = runtime();
  std::shared_ptr<ClassRecord> owner;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    owner = lookup_class_locked(state, id);
  }
  if (!owner) {
    record(2, "compute_if_needed_and_checkout", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }

  AEGP_CCComputeKey key{};
  const A_Err key_error =
      invoke_generate_key(owner->callbacks.generate_key, options, &key);
  if (key_error != kErrNone) {
    record(2, "compute_if_needed_and_checkout", Outcome::callback_failure,
           key_error);
    return key_error;
  }

  std::shared_ptr<CacheEntry> entry;
  bool should_compute = false;
  {
    std::unique_lock<std::mutex> lock(state.mutex);
    const std::shared_ptr<ClassRecord> current = lookup_class_locked(state, id);
    if (!current || current.get() != owner.get() ||
        current->generation != owner->generation) {
      record(2, "compute_if_needed_and_checkout", Outcome::invalid,
             kErrStruct);
      return kErrStruct;
    }
    auto found = owner->entries.find(key);
    if (found != owner->entries.end() &&
        found->second->state == EntryState::failed) {
      owner->entries.erase(found);
      if (state.entry_count != 0) --state.entry_count;
      found = owner->entries.end();
    }
    if (found == owner->entries.end()) {
      if (state.entry_count >= kMaxEntries) {
        record(2, "compute_if_needed_and_checkout",
               Outcome::capacity_failure, kErrAlloc);
        return kErrAlloc;
      }
      try {
        entry = std::make_shared<CacheEntry>();
        entry->computing_thread = std::this_thread::get_id();
        owner->entries.emplace(key, entry);
        ++state.entry_count;
        should_compute = true;
      } catch (...) {
        record(2, "compute_if_needed_and_checkout",
               Outcome::capacity_failure, kErrAlloc);
        return kErrAlloc;
      }
    } else {
      entry = found->second;
      while (entry->state == EntryState::computing) {
        if (!wait_for_other_thread ||
            entry->computing_thread == std::this_thread::get_id()) {
          record(2, "compute_if_needed_and_checkout",
                 Outcome::compute_pending,
                 kErrNotInCacheOrComputePending);
          return kErrNotInCacheOrComputePending;
        }
        entry->changed.wait(lock);
        if (owner->retired) {
          record(2, "compute_if_needed_and_checkout", Outcome::invalid,
                 kErrStruct);
          return kErrStruct;
        }
      }
      if (entry->state == EntryState::failed) {
        const A_Err failure = entry->failure;
        record(2, "compute_if_needed_and_checkout",
               Outcome::callback_failure, failure);
        return failure;
      }
      const A_Err receipt_error =
          make_receipt_locked(state, owner, entry, receipt_pointer);
      record(2, "compute_if_needed_and_checkout",
             receipt_error == kErrNone ? Outcome::cache_hit
                                      : Outcome::capacity_failure,
             receipt_error);
      return receipt_error;
    }
  }

  void* computed_value{};
  A_Err compute_error =
      invoke_compute(owner->callbacks.compute, options, &computed_value);
  std::size_t approximate_size{};
  if (compute_error == kErrNone && !computed_value)
    compute_error = kErrStruct;
  if (compute_error == kErrNone &&
      !invoke_size(owner->callbacks.approx_size_value, computed_value,
                   approximate_size))
    compute_error = kErrGeneric;
  if (compute_error == kErrNone &&
      approximate_size > kMaxValueBytes)
    compute_error = kErrAlloc;

  if (compute_error != kErrNone) {
    invoke_delete(owner->callbacks.delete_compute_value, computed_value);
    {
      std::lock_guard<std::mutex> lock(state.mutex);
      entry->state = EntryState::failed;
      entry->failure = compute_error;
      entry->computing_thread = {};
    }
    entry->changed.notify_all();
    record(2, "compute_if_needed_and_checkout",
           compute_error == kErrAlloc ? Outcome::capacity_failure
                                      : Outcome::callback_failure,
           compute_error);
    return compute_error;
  }

  A_Err receipt_error = kErrNone;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    if (owner->retired ||
        approximate_size > kMaxTotalValueBytes - state.total_value_bytes) {
      entry->state = EntryState::failed;
      entry->failure = owner->retired ? kErrStruct : kErrAlloc;
      entry->computing_thread = {};
      receipt_error = entry->failure;
    } else {
      entry->value = computed_value;
      entry->approximate_size = approximate_size;
      entry->state = EntryState::ready;
      entry->computing_thread = {};
      state.total_value_bytes += approximate_size;
      computed_value = nullptr;
      receipt_error =
          make_receipt_locked(state, owner, entry, receipt_pointer);
    }
  }
  entry->changed.notify_all();
  invoke_delete(owner->callbacks.delete_compute_value, computed_value);
  record(2, "compute_if_needed_and_checkout",
         receipt_error == kErrNone
             ? (should_compute ? Outcome::computed : Outcome::cache_hit)
             : (receipt_error == kErrAlloc ? Outcome::capacity_failure
                                           : Outcome::invalid),
         receipt_error);
  return receipt_error;
}

A_Err __cdecl compute_if_needed_and_checkout(
    AEGP_CCComputeClassIdP compute_class,
    AEGP_CCComputeOptionsRefconP options, bool wait_for_other_thread,
    AEGP_CCCheckoutReceiptP* receipt) noexcept {
  return compute_or_checkout(compute_class, options, wait_for_other_thread,
                             receipt);
}

A_Err __cdecl checkout_cached(
    AEGP_CCComputeClassIdP compute_class,
    AEGP_CCComputeOptionsRefconP options,
    AEGP_CCCheckoutReceiptP* receipt_pointer) noexcept {
  if (!store_pointer_seh(receipt_pointer, nullptr)) {
    record(3, "checkout_cached", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  std::string id;
  if (!class_id(compute_class, id)) {
    record(3, "checkout_cached", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  Runtime& state = runtime();
  std::shared_ptr<ClassRecord> owner;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    owner = lookup_class_locked(state, id);
  }
  if (!owner) {
    record(3, "checkout_cached", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }
  AEGP_CCComputeKey key{};
  const A_Err key_error =
      invoke_generate_key(owner->callbacks.generate_key, options, &key);
  if (key_error != kErrNone) {
    record(3, "checkout_cached", Outcome::callback_failure, key_error);
    return key_error;
  }
  std::lock_guard<std::mutex> lock(state.mutex);
  const std::shared_ptr<ClassRecord> current = lookup_class_locked(state, id);
  if (!current || current.get() != owner.get() ||
      current->generation != owner->generation) {
    record(3, "checkout_cached", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }
  const auto found = owner->entries.find(key);
  if (found == owner->entries.end() ||
      found->second->state != EntryState::ready) {
    record(3, "checkout_cached", Outcome::cache_miss,
           kErrNotInCacheOrComputePending);
    return kErrNotInCacheOrComputePending;
  }
  const A_Err result =
      make_receipt_locked(state, owner, found->second, receipt_pointer);
  record(3, "checkout_cached",
         result == kErrNone ? Outcome::cache_hit : Outcome::capacity_failure,
         result);
  return result;
}

A_Err __cdecl get_receipt_compute_value(
    AEGP_CCCheckoutReceiptP receipt_pointer,
    AEGP_CCComputeValueRefconP* value_pointer) noexcept {
  if (!store_pointer_seh(value_pointer, nullptr) || !receipt_pointer) {
    record(4, "get_receipt_compute_value", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  Runtime& state = runtime();
  std::lock_guard<std::mutex> lock(state.mutex);
  const auto found =
      state.receipts.find(reinterpret_cast<uintptr_t>(receipt_pointer));
  if (found == state.receipts.end() || !found->second.active ||
      found->second.token != receipt_pointer ||
      found->second.generation != found->second.token->generation ||
      !found->second.owner || found->second.owner->retired ||
      found->second.class_generation != found->second.owner->generation ||
      !caller_matches(*found->second.owner) || !found->second.entry ||
      found->second.entry->state != EntryState::ready ||
      !found->second.entry->value) {
    record(4, "get_receipt_compute_value", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }
  if (!store_pointer_seh(value_pointer, found->second.entry->value)) {
    record(4, "get_receipt_compute_value", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  record(4, "get_receipt_compute_value", Outcome::value_returned, kErrNone);
  return kErrNone;
}

A_Err __cdecl checkin_compute_receipt(
    AEGP_CCCheckoutReceiptP receipt_pointer) noexcept {
  if (!receipt_pointer) {
    record(5, "checkin_compute_receipt", Outcome::invalid, kErrParameter);
    return kErrParameter;
  }
  Runtime& state = runtime();
  std::lock_guard<std::mutex> lock(state.mutex);
  const auto found =
      state.receipts.find(reinterpret_cast<uintptr_t>(receipt_pointer));
  if (found == state.receipts.end() || !found->second.active ||
      found->second.token != receipt_pointer ||
      found->second.generation != found->second.token->generation ||
      !found->second.owner ||
      found->second.class_generation != found->second.owner->generation ||
      !caller_matches(*found->second.owner) || !found->second.entry ||
      found->second.entry->checkout_count == 0 ||
      found->second.owner->live_receipts == 0 ||
      state.active_receipts == 0) {
    record(5, "checkin_compute_receipt", Outcome::invalid, kErrStruct);
    return kErrStruct;
  }
  found->second.active = false;
  --found->second.entry->checkout_count;
  --found->second.owner->live_receipts;
  --state.active_receipts;
  record(5, "checkin_compute_receipt", Outcome::checked_in, kErrNone);
  return kErrNone;
}

AEGP_ComputeCacheSuite1 g_suite{
    &class_register,
    &class_unregister,
    &compute_if_needed_and_checkout,
    &checkout_cached,
    &get_receipt_compute_value,
    &checkin_compute_receipt,
};

bool teardown_owner(HMODULE module) noexcept {
  if (!module) {
    reject_unload();
    return false;
  }
  Runtime& state = runtime();
  std::vector<DeleteWork> work;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    std::size_t ready_count{};
    for (const auto& item : state.classes) {
      const std::shared_ptr<ClassRecord>& owner = item.second;
      if (owner->owner != module) continue;
      const bool computing = std::any_of(
          owner->entries.begin(), owner->entries.end(), [](const auto& entry) {
            return entry.second->state == EntryState::computing;
          });
      if (computing || owner->live_receipts != 0) {
        record(1, "global_teardown", Outcome::cleanup_deferred, kErrStruct);
        reject_unload();
        return false;
      }
      ready_count += static_cast<std::size_t>(std::count_if(
          owner->entries.begin(), owner->entries.end(), [](const auto& entry) {
            return entry.second->state == EntryState::ready &&
                   entry.second->value != nullptr;
          }));
    }
    try {
      work.reserve(ready_count);
    } catch (...) {
      record(1, "global_teardown", Outcome::capacity_failure, kErrAlloc);
      reject_unload();
      return false;
    }
    for (auto iterator = state.classes.begin();
         iterator != state.classes.end();) {
      const std::shared_ptr<ClassRecord> owner = iterator->second;
      if (owner->owner != module) {
        ++iterator;
        continue;
      }
      owner->retired = true;
      collect_class_values_locked(state, owner, work);
      iterator = state.classes.erase(iterator);
    }
  }
  run_delete_work(work);
  record(1, "global_teardown", Outcome::cleanup, kErrNone);
  return true;
}

}  // namespace

const AEGP_ComputeCacheSuite1* suite() noexcept { return &g_suite; }

const void* provide_suite1(void*) noexcept { return suite(); }

bool teardown_owner_from_entry(const void* entry) noexcept {
  return teardown_owner(module_from_address(entry));
}

bool unload_safe() noexcept {
  return unload_safe_state().load(std::memory_order_acquire);
}

void reset_telemetry() noexcept {
  Telemetry& state = telemetry();
  std::lock_guard<std::mutex> lock(state.mutex);
  state.records.clear();
  state.truncated = false;
}

std::string telemetry_report_json() {
  Telemetry& state = telemetry();
  std::lock_guard<std::mutex> lock(state.mutex);
  std::ostringstream json;
  json << ",\"compute_cache_timeline\":{\"maximum_records\":"
       << kMaxTelemetryRecords << ",\"records\":[";
  for (std::size_t index = 0; index < state.records.size(); ++index) {
    if (index != 0) json << ',';
    const TelemetryRecord& item = state.records[index];
    json << "{\"sequence\":" << item.sequence
         << ",\"selector\":\"" << item.selector
         << "\",\"slot\":" << item.slot
         << ",\"operation\":\"" << item.operation
         << "\",\"outcome\":\"" << outcome_name(item.outcome)
         << "\",\"return_code\":" << item.return_code
         << ",\"call_count\":" << item.call_count << '}';
  }
  json << "],\"truncated\":" << (state.truncated ? "true" : "false")
       << '}';
  return json.str();
}

bool reset_for_selftest() noexcept {
  Runtime& state = runtime();
  std::vector<DeleteWork> work;
  {
    std::lock_guard<std::mutex> lock(state.mutex);
    std::size_t ready_count{};
    for (const auto& item : state.classes) {
      const auto& owner = item.second;
      const bool computing = std::any_of(
          owner->entries.begin(), owner->entries.end(), [](const auto& entry) {
            return entry.second->state == EntryState::computing;
          });
      if (computing || owner->live_receipts != 0) return false;
      ready_count += static_cast<std::size_t>(std::count_if(
          owner->entries.begin(), owner->entries.end(), [](const auto& entry) {
            return entry.second->state == EntryState::ready &&
                   entry.second->value != nullptr;
          }));
    }
    try {
      work.reserve(ready_count);
    } catch (...) {
      return false;
    }
    for (auto& item : state.classes) {
      item.second->retired = true;
      collect_class_values_locked(state, item.second, work);
    }
    state.classes.clear();
    state.receipts.clear();
    state.entry_count = 0;
    state.active_receipts = 0;
    state.total_value_bytes = 0;
  }
  run_delete_work(work);
  reset_telemetry();
  unload_safe_state().store(true, std::memory_order_release);
  return true;
}

}  // namespace aexcompat::worker_runtime::compute_cache
