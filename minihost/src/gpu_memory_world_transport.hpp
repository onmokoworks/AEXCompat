#pragma once

#include "worker_world_safety.hpp"

#include <array>
#include <cstddef>
#include <cstdint>

namespace aexcompat::gpu_runtime::memory_world_transport {

using HostNewWorld = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t,
                                       int32_t, void*);
using HostDisposeWorld = int32_t(__cdecl*)(void*, void*);
using HostOwnsWorld = bool (*)(void*);
using HostRecognizesSmartWorld = bool (*)(void*);

void configure_host_world_fallback(HostNewWorld new_world,
                                   HostDisposeWorld dispose_world,
                                   HostOwnsWorld owns_world,
                                   HostRecognizesSmartWorld recognizes_smart_world);

bool begin_cuda_context(uint32_t active_device_index,
                        void* borrowed_context = nullptr);
bool end_cuda_context();
bool begin_backend_context(int32_t framework, uint32_t active_device_index,
                           void* borrowed_context = nullptr);
bool end_backend_context(int32_t framework);
void* active_cuda_context();
uint32_t active_gpu_device_index();

int32_t __cdecl gpu_acquire_exclusive(void*, uint32_t index);
int32_t __cdecl gpu_release_exclusive(void*, uint32_t index);
int32_t __cdecl gpu_allocate_device_memory(void*, uint32_t index,
                                            std::size_t size, void** memory);
int32_t __cdecl gpu_free_device_memory(void*, uint32_t index, void* memory);
int32_t __cdecl gpu_purge_memory(void*, uint32_t index, std::size_t requested,
                                 std::size_t* purged);
int32_t __cdecl gpu_allocate_host_memory(void*, uint32_t index,
                                          std::size_t size, void** memory);
int32_t __cdecl gpu_free_host_memory(void*, uint32_t index, void* memory);
int32_t __cdecl gpu_create_world(void*, uint32_t index, int32_t width,
                                  int32_t height,
                                  world_safety::LocalRationalScale scale,
                                  int32_t field, int32_t pixel_format,
                                  uint8_t clear_pixels, void** world);
int32_t __cdecl gpu_dispose_world(void*, void* world);
int32_t __cdecl gpu_get_world_data(void*, void* world, void** pixels);
int32_t __cdecl gpu_get_world_size(void*, void* world, std::size_t* size);
int32_t __cdecl gpu_get_world_device_index(void*, void* world, uint32_t* index);

struct RenderTransport {
  void* input_world{};
  void* output_world{};
  void* input_host{};
  void* output_host{};
  void* input_device{};
  void* output_device{};
  int32_t input_width{};
  int32_t input_height{};
  int32_t input_rowbytes{};
  int32_t output_width{};
  int32_t output_height{};
  int32_t output_rowbytes{};
};

bool prepare_render_transport(void* input_world, void* output_world,
                              RenderTransport& transport);
bool finish_render_transport(RenderTransport& transport);
bool gpu_memory_lifetimes_balanced();
std::size_t live_allocation_count();
std::size_t live_memory_bytes();
uint32_t exclusive_access_depth();

extern std::array<void*, 15> gpu_device_suite1;
extern uint64_t cuda_upload_bytes;
extern uint64_t cuda_download_bytes;
extern uint32_t cuda_sync_failures;
extern uint32_t last_cuda_device_count;
extern uint32_t last_cuda_device_index;
extern uint64_t opencl_upload_bytes;
extern uint64_t opencl_download_bytes;
extern uint32_t opencl_sync_failures;
extern uint64_t allocations_created;
extern uint64_t allocations_freed;
extern uint64_t invalid_memory_operations;

}  // namespace aexcompat::gpu_runtime::memory_world_transport
