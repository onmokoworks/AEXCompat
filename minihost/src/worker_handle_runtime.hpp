#pragma once

#include <cstddef>
#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::handles {

constexpr std::uint64_t kMaxHandleBytes = 2ULL * 1024ULL * 1024ULL * 1024ULL;
constexpr std::uint64_t kObservedLargeHandleBytes = 333294848ULL;
static_assert(kMaxHandleBytes >= kObservedLargeHandleBytes);
constexpr std::size_t kMaxHandleCount = 16384;

struct Statistics {
  std::uint32_t created{};
  std::uint32_t disposed{};
  std::uint32_t locks{};
  std::uint32_t unlocks{};
  std::uint32_t locks_released_on_dispose{};
  std::uint32_t invalid_operations{};
  std::uint32_t automatic_pre_render_disposals{};
  std::size_t live_count{};
  std::uint64_t live_bytes{};
};

void** __cdecl new_handle(std::uint64_t size);
void* __cdecl lock_handle(void** handle);
void __cdecl unlock_handle(void** handle);
void __cdecl dispose_handle(void** handle);
void dispose_all_live_handles();
std::uint64_t __cdecl handle_size(void** handle);
std::int32_t __cdecl resize_handle(std::uint64_t size, void*** handle);

bool handle_lifetimes_balanced();
bool host_handle_is_live(const void* handle);
Statistics statistics();
void record_automatic_pre_render_disposal();

struct HandleSuite {
  decltype(&new_handle) create;
  decltype(&lock_handle) lock;
  decltype(&unlock_handle) unlock;
  decltype(&dispose_handle) dispose;
  decltype(&handle_size) size;
  decltype(&resize_handle) resize;
};

extern HandleSuite g_handle_suite;

constexpr std::size_t kMaxAegpMemoryHandles = 256;
constexpr std::uint64_t kMaxAegpMemoryBytes = 16ULL * 1024ULL * 1024ULL;

struct AegpMemoryStatistics {
  std::uint32_t created{};
  std::uint32_t freed{};
  std::uint32_t invalid_operations{};
  std::size_t live_count{};
  std::uint64_t live_bytes{};
  bool reporting{};
};

std::int32_t __cdecl new_aegp_mem_handle(std::int32_t plugin_id, const char* what,
                                         std::uint32_t size, std::int32_t flags,
                                         void** handle);
std::int32_t __cdecl free_aegp_mem_handle(void* handle);
std::int32_t __cdecl lock_aegp_mem_handle(void* handle, void** data);
std::int32_t __cdecl unlock_aegp_mem_handle(void* handle);
std::int32_t __cdecl get_aegp_mem_handle_size(void* handle, std::uint32_t* size);
std::int32_t __cdecl resize_aegp_mem_handle(const char* what, std::uint32_t size,
                                            void* handle);
std::int32_t __cdecl set_aegp_mem_reporting(std::uint8_t enabled);
std::int32_t __cdecl get_aegp_mem_stats(std::int32_t plugin_id, std::int32_t* count,
                                        std::int32_t* size);

bool aegp_memory_balanced();
AegpMemoryStatistics aegp_memory_statistics();
std::int32_t make_utf16_handle(const std::u16string& text, const char* label,
                               void** handle);

struct AegpMemorySuite {
  decltype(&new_aegp_mem_handle) new_mem_handle;
  decltype(&free_aegp_mem_handle) free_mem_handle;
  decltype(&lock_aegp_mem_handle) lock_mem_handle;
  decltype(&unlock_aegp_mem_handle) unlock_mem_handle;
  decltype(&get_aegp_mem_handle_size) get_mem_handle_size;
  decltype(&resize_aegp_mem_handle) resize_mem_handle;
  decltype(&set_aegp_mem_reporting) set_mem_reporting_on;
  decltype(&get_aegp_mem_stats) get_mem_stats;
};

static_assert(sizeof(AegpMemorySuite) == 8 * sizeof(void*));
extern AegpMemorySuite g_aegp_memory_suite;

}  // namespace aexcompat::worker_runtime::handles
