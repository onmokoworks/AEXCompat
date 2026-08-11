#include "worker_handle_runtime.hpp"

#include "trace_writer.hpp"

#include <algorithm>
#include <cstring>
#include <iostream>
#include <limits>
#include <memory>
#include <mutex>
#include <new>
#include <unordered_set>
#include <unordered_map>
#include <vector>

namespace aexcompat::l2_detail {
// Session-lifetime trace sink (defined in l2_main.cpp). Per-callback events are
// high-frequency, so they are emitted only under the writer's verbose opt-in
// (issue #17); the existing unconditional stderr markers are unchanged.
extern aexcompat::TraceWriter* g_trace_writer;
}  // namespace aexcompat::l2_detail

namespace aexcompat::worker_runtime::handles {
namespace {

void trace_callback_invoke() {
  if (aexcompat::l2_detail::g_trace_writer &&
      aexcompat::l2_detail::g_trace_writer->verbose()) {
    aexcompat::l2_detail::g_trace_writer->callback_invoke();
  }
}

struct HandleRecord {
  // This field must remain first: PF_Handle is the address of the movable data
  // pointer, and callbacks recover the containing record from that address.
  void* data{};
  std::size_t size{};
  std::uint32_t lock_count{};
};

std::unordered_set<HandleRecord*> g_handles;
std::vector<HandleRecord*> g_quarantined_handles;
std::uint64_t g_quarantined_bytes{};
bool g_reclamation_quarantine_active{};
std::mutex g_mutex;
Statistics g_statistics;

struct AegpMemoryRecord {
  std::vector<std::byte> bytes;
  std::int32_t plugin_id{};
  std::uint32_t lock_count{};
};
std::unordered_map<void*, std::unique_ptr<AegpMemoryRecord>> g_aegp_memory;
std::mutex g_aegp_memory_mutex;
AegpMemoryStatistics g_aegp_statistics;

void invalid_operation() { ++g_statistics.invalid_operations; }

bool physical_budget_exceeded(std::uint64_t requested,
                              std::uint64_t replaced_live_bytes = 0) {
  if (requested > kMaxHandleBytes ||
      g_statistics.live_bytes < replaced_live_bytes)
    return true;
  const std::uint64_t remaining_live =
      g_statistics.live_bytes - replaced_live_bytes;
  return remaining_live > kMaxHandleBytes - requested ||
         g_quarantined_bytes >
             kMaxHandleBytes - requested - remaining_live;
}

}  // namespace

void** __cdecl new_handle(std::uint64_t size) {
  std::cerr << "callback:new_handle size=" << size << "\n" << std::flush;
  trace_callback_invoke();
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_handles.size() + g_quarantined_handles.size() >= kMaxHandleCount ||
      physical_budget_exceeded(size)) {
    std::cerr << "callback:new_handle_failed reason=budget size=" << size << "\n"
              << std::flush;
    invalid_operation();
    return nullptr;
  }
  auto* record = new (std::nothrow) HandleRecord;
  if (!record) {
    std::cerr << "callback:new_handle_failed reason=record size=" << size << "\n"
              << std::flush;
    invalid_operation();
    return nullptr;
  }
  record->data = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!record->data && size != 0) {
    std::cerr << "callback:new_handle_failed reason=data size=" << size << "\n"
              << std::flush;
    invalid_operation();
    delete record;
    return nullptr;
  }
  if (record->data) std::memset(record->data, 0, static_cast<std::size_t>(size));
  record->size = static_cast<std::size_t>(size);
  g_handles.insert(record);
  ++g_statistics.created;
  g_statistics.live_bytes += size;
  return &record->data;
}

void* __cdecl lock_handle(void** handle) {
  std::cerr << "callback:lock_handle\n" << std::flush;
  trace_callback_invoke();
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_mutex);
  if (!record || !g_handles.count(record)) {
    invalid_operation();
    return nullptr;
  }
  ++record->lock_count;
  ++g_statistics.locks;
  return record->data;
}

void __cdecl unlock_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_mutex);
  if (!record || !g_handles.count(record) || record->lock_count == 0) {
    invalid_operation();
    return;
  }
  --record->lock_count;
  ++g_statistics.unlocks;
}

void __cdecl dispose_handle(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  bool quarantine = false;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (!record || !g_handles.count(record)) {
      invalid_operation();
      return;
    }
    // PF's dispose callback is void, so unlike resize it has no channel for
    // rejecting a live-but-locked handle. Reclaim the host-owned allocation
    // and account for its outstanding locks; unknown, stale and foreign
    // pointers remain fail-closed above.
    g_statistics.locks_released_on_dispose += record->lock_count;
    g_handles.erase(record);
    ++g_statistics.disposed;
    g_statistics.live_bytes -= record->size;
    quarantine = g_reclamation_quarantine_active;
    if (quarantine) {
      g_quarantined_handles.push_back(record);
      g_quarantined_bytes += record->size;
    }
  }
  if (quarantine) return;
  ::operator delete(record->data);
  delete record;
}

void dispose_all_live_handles() {
  std::vector<HandleRecord*> records;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    records.assign(g_handles.begin(), g_handles.end());
  }
  for (auto* record : records) dispose_handle(&record->data);
}

void begin_handle_reclamation_quarantine() {
  std::lock_guard<std::mutex> lock(g_mutex);
  // Reserve outside any plug-in callback. The total live+quarantined handle
  // limit below guarantees that dispose_handle cannot grow this vector while
  // crossing the foreign ABI boundary.
  g_quarantined_handles.reserve(kMaxHandleCount);
  g_reclamation_quarantine_active = true;
}

void reclaim_quarantined_handles() {
  std::vector<HandleRecord*> records;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_reclamation_quarantine_active = false;
    records.swap(g_quarantined_handles);
    g_quarantined_bytes = 0;
  }
  for (auto* record : records) {
    ::operator delete(record->data);
    delete record;
  }
}

std::uint64_t __cdecl handle_size(void** handle) {
  auto* record = reinterpret_cast<HandleRecord*>(handle);
  std::lock_guard<std::mutex> lock(g_mutex);
  if (!record || !g_handles.count(record)) {
    invalid_operation();
    return 0;
  }
  return record->size;
}

std::int32_t __cdecl resize_handle(std::uint64_t size, void*** handle) {
  std::lock_guard<std::mutex> lock(g_mutex);
  if (!handle || !*handle || size > kMaxHandleBytes) {
    invalid_operation();
    return 4;
  }
  auto* record = reinterpret_cast<HandleRecord*>(*handle);
  if (!g_handles.count(record) || record->lock_count != 0 ||
      physical_budget_exceeded(size, record->size)) {
    invalid_operation();
    return 4;
  }
  void* replacement = ::operator new(static_cast<std::size_t>(size), std::nothrow);
  if (!replacement && size) {
    std::cerr << "callback:resize_handle_failed reason=data size=" << size << "\n"
              << std::flush;
    invalid_operation();
    return 1;
  }
  if (replacement) {
    std::memset(replacement, 0, static_cast<std::size_t>(size));
    std::memcpy(replacement, record->data,
                (std::min)(record->size, static_cast<std::size_t>(size)));
  }
  ::operator delete(record->data);
  record->data = replacement;
  g_statistics.live_bytes = g_statistics.live_bytes - record->size + size;
  record->size = static_cast<std::size_t>(size);
  return 0;
}

bool handle_lifetimes_balanced() {
  std::lock_guard<std::mutex> lock(g_mutex);
  return g_handles.empty() && g_quarantined_handles.empty() &&
         g_statistics.created == g_statistics.disposed &&
         g_statistics.locks ==
             g_statistics.unlocks + g_statistics.locks_released_on_dispose;
}

bool host_handle_is_live(const void* handle) {
  std::lock_guard<std::mutex> lock(g_mutex);
  return handle &&
         g_handles.count(reinterpret_cast<HandleRecord*>(const_cast<void*>(handle))) != 0;
}

Statistics statistics() {
  std::lock_guard<std::mutex> lock(g_mutex);
  Statistics result = g_statistics;
  result.live_count = g_handles.size();
  result.quarantined_count = g_quarantined_handles.size();
  result.quarantined_bytes = g_quarantined_bytes;
  return result;
}

void record_automatic_pre_render_disposal() {
  std::lock_guard<std::mutex> lock(g_mutex);
  ++g_statistics.automatic_pre_render_disposals;
}

HandleReclamationScope::HandleReclamationScope() {
  begin_handle_reclamation_quarantine();
}

HandleReclamationScope::~HandleReclamationScope() {
  if (active_) reclaim_quarantined_handles();
}

void HandleReclamationScope::reclaim() {
  if (!active_) return;
  reclaim_quarantined_handles();
  active_ = false;
}

HandleSuite g_handle_suite{&new_handle, &lock_handle, &unlock_handle,
                           &dispose_handle, &handle_size, &resize_handle};

std::int32_t __cdecl new_aegp_mem_handle(std::int32_t plugin_id, const char* what,
                                         std::uint32_t size, std::int32_t flags,
                                         void** handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  if (plugin_id != 1 || !what || std::strlen(what) > 127 || !handle ||
      (flags & ~3) != 0 || g_aegp_memory.size() >= kMaxAegpMemoryHandles ||
      size > kMaxAegpMemoryBytes ||
      g_aegp_statistics.live_bytes > kMaxAegpMemoryBytes - size) {
    if (handle) *handle = nullptr;
    ++g_aegp_statistics.invalid_operations;
    return 4;
  }
  auto record = std::make_unique<AegpMemoryRecord>();
  record->plugin_id = plugin_id;
  record->bytes.resize(size);
  if ((flags & 1) == 0 && size) std::memset(record->bytes.data(), 0xcd, size);
  void* key = record.get();
  g_aegp_memory.emplace(key, std::move(record));
  g_aegp_statistics.live_bytes += size;
  ++g_aegp_statistics.created;
  *handle = key;
  return 0;
}

std::int32_t __cdecl free_aegp_mem_handle(void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || found->second->lock_count != 0) {
    ++g_aegp_statistics.invalid_operations;
    return 4;
  }
  g_aegp_statistics.live_bytes -= found->second->bytes.size();
  g_aegp_memory.erase(found);
  ++g_aegp_statistics.freed;
  return 0;
}

std::int32_t __cdecl lock_aegp_mem_handle(void* handle, void** data) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || !data) {
    ++g_aegp_statistics.invalid_operations;
    return 4;
  }
  ++found->second->lock_count;
  *data = found->second->bytes.empty() ? nullptr : found->second->bytes.data();
  return 0;
}

std::int32_t __cdecl unlock_aegp_mem_handle(void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || found->second->lock_count == 0) {
    ++g_aegp_statistics.invalid_operations;
    return 4;
  }
  --found->second->lock_count;
  return 0;
}

std::int32_t __cdecl get_aegp_mem_handle_size(void* handle, std::uint32_t* size) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (found == g_aegp_memory.end() || !size) return 4;
  *size = static_cast<std::uint32_t>(found->second->bytes.size());
  return 0;
}

std::int32_t __cdecl resize_aegp_mem_handle(const char* what, std::uint32_t size,
                                            void* handle) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  const auto found = g_aegp_memory.find(handle);
  if (!what || std::strlen(what) > 127 || found == g_aegp_memory.end() ||
      found->second->lock_count != 0 || size > kMaxAegpMemoryBytes ||
      g_aegp_statistics.live_bytes - found->second->bytes.size() >
          kMaxAegpMemoryBytes - size) {
    ++g_aegp_statistics.invalid_operations;
    return 4;
  }
  const std::size_t old_size = found->second->bytes.size();
  found->second->bytes.resize(size);
  g_aegp_statistics.live_bytes = g_aegp_statistics.live_bytes - old_size + size;
  return 0;
}

std::int32_t __cdecl set_aegp_mem_reporting(std::uint8_t enabled) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  g_aegp_statistics.reporting = enabled != 0;
  return 0;
}

std::int32_t __cdecl get_aegp_mem_stats(std::int32_t plugin_id, std::int32_t* count,
                                        std::int32_t* size) {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  if (plugin_id != 1 || !count || !size) return 4;
  std::uint64_t total{};
  std::int32_t handles{};
  for (const auto& item : g_aegp_memory) {
    if (item.second->plugin_id == plugin_id) {
      ++handles;
      total += item.second->bytes.size();
    }
  }
  if (total > static_cast<std::uint64_t>((std::numeric_limits<std::int32_t>::max)()))
    return 4;
  *count = handles;
  *size = static_cast<std::int32_t>(total);
  return 0;
}

bool aegp_memory_balanced() {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  return g_aegp_memory.empty() && g_aegp_statistics.live_bytes == 0 &&
         g_aegp_statistics.created == g_aegp_statistics.freed;
}

AegpMemoryStatistics aegp_memory_statistics() {
  std::lock_guard<std::mutex> lock(g_aegp_memory_mutex);
  AegpMemoryStatistics result = g_aegp_statistics;
  result.live_count = g_aegp_memory.size();
  return result;
}

std::int32_t make_utf16_handle(const std::u16string& text, const char* label,
                               void** handle) {
  const std::uint64_t bytes = (text.size() + 1) * sizeof(char16_t);
  if (bytes > (std::numeric_limits<std::uint32_t>::max)() ||
      new_aegp_mem_handle(1, label, static_cast<std::uint32_t>(bytes), 1, handle))
    return 4;
  void* data = nullptr;
  if (lock_aegp_mem_handle(*handle, &data) != 0) {
    free_aegp_mem_handle(*handle);
    *handle = nullptr;
    return 4;
  }
  std::memcpy(data, text.c_str(), static_cast<std::size_t>(bytes));
  return unlock_aegp_mem_handle(*handle);
}

AegpMemorySuite g_aegp_memory_suite{
    &new_aegp_mem_handle, &free_aegp_mem_handle, &lock_aegp_mem_handle,
    &unlock_aegp_mem_handle, &get_aegp_mem_handle_size, &resize_aegp_mem_handle,
    &set_aegp_mem_reporting, &get_aegp_mem_stats};

}  // namespace aexcompat::worker_runtime::handles
