#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::gpu_runtime::opencl {

using Int = int32_t;
using UInt = uint32_t;
using Ulong = uint64_t;
using Mem = void*;

inline constexpr Int kSuccess = 0;
inline constexpr Ulong kMemReadWrite = 1u << 0;

bool begin_context(uint32_t active_device_index);
bool end_context();

bool active();
uint32_t active_device_index();
uint32_t device_count();
uint32_t last_device_count();
uint32_t last_device_index();

Mem create_buffer(uint32_t device_index, Ulong flags, std::size_t size,
                  void* host_pointer, Int* error);
Int release_mem(Mem memory);
Int enqueue_write(uint32_t device_index, Mem memory, UInt blocking,
                  std::size_t offset, std::size_t size, const void* source);
Int enqueue_read(uint32_t device_index, Mem memory, UInt blocking,
                 std::size_t offset, std::size_t size, void* destination);
Int enqueue_fill(uint32_t device_index, Mem memory, const void* pattern,
                 std::size_t pattern_size, std::size_t offset, std::size_t size);
Int finish(uint32_t device_index);

}  // namespace aexcompat::gpu_runtime::opencl
