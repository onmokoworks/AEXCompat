#include "gpu_memory_world_transport.hpp"

#include "gpu_cuda_backend.hpp"
#include "gpu_directx_backend.hpp"
#include "gpu_device_info_registry.hpp"
#include "gpu_opencl_backend.hpp"

#include <d3d12.h>
#include <windows.h>

#include <cstring>
#include <memory>
#include <mutex>
#include <new>
#include <unordered_map>
#include <vector>

namespace aexcompat::gpu_runtime::memory_world_transport {
namespace {

namespace directx = directx_backend;
namespace opencl = gpu_runtime::opencl;

constexpr std::size_t kMaxGpuAllocations = 256;
constexpr std::size_t kMaxGpuAllocationBytes = 256u * 1024u * 1024u;
constexpr int32_t kPixelFormatGpuBgra128 = 1094992704;
constexpr int32_t kMaxWorldDimension = 4096;

struct HostWorldFallback {
  HostNewWorld new_world{};
  HostDisposeWorld dispose_world{};
  HostOwnsWorld owns_world{};
  HostRecognizesSmartWorld recognizes_smart_world{};
};

HostWorldFallback g_host{};
std::mutex g_memory_mutex;
std::unordered_map<void*, std::size_t> g_device_memory;
std::unordered_map<void*, std::size_t> g_host_memory;
std::unordered_map<void*, uint32_t> g_created_worlds;
std::size_t g_memory_bytes{};
uint32_t g_exclusive_access_depth{};

bool allocation_request_valid(uint32_t index, std::size_t size, void** memory) {
  return index == active_gpu_device_index() && memory && size != 0 &&
      size <= kMaxGpuAllocationBytes;
}

bool has_allocation_capacity(std::size_t size) {
  return g_device_memory.size() + g_host_memory.size() < kMaxGpuAllocations &&
      g_memory_bytes <= kMaxGpuAllocationBytes - size;
}

int32_t allocate_fallback(uint32_t index, std::size_t size, void** memory,
                          std::unordered_map<void*, std::size_t>& allocations) {
  if (!allocation_request_valid(index, size, memory)) return 4;
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  *memory = nullptr;
  if (!has_allocation_capacity(size)) return 4;
  void* allocated = ::operator new(size, std::nothrow);
  if (!allocated) return 2;
  allocations.emplace(allocated, size);
  g_memory_bytes += size;
  ++allocations_created;
  *memory = allocated;
  return 0;
}

int32_t free_fallback(uint32_t index, void* memory,
                      std::unordered_map<void*, std::size_t>& allocations) {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  const auto found = allocations.find(memory);
  if (index != active_gpu_device_index() || !memory || found == allocations.end()) {
    ++invalid_memory_operations;
    return 4;
  }
  g_memory_bytes -= found->second;
  allocations.erase(found);
  ::operator delete(memory);
  ++allocations_freed;
  return 0;
}

bool read_world(void* world, void*& pixels, int32_t& rowbytes,
                int32_t& width, int32_t& height) {
  if (!world) return false;
  auto* bytes = static_cast<std::byte*>(world);
  std::memcpy(&pixels, bytes + 24, sizeof(pixels));
  std::memcpy(&rowbytes, bytes + 32, sizeof(rowbytes));
  std::memcpy(&width, bytes + 36, sizeof(width));
  std::memcpy(&height, bytes + 40, sizeof(height));
  return pixels && width > 0 && height > 0 && width <= kMaxWorldDimension &&
      height <= kMaxWorldDimension && rowbytes >= width * 16 &&
      rowbytes <= kMaxWorldDimension * 16;
}

bool is_active_world(void* world) {
  return world && (g_created_worlds.count(world) != 0 ||
                   (g_host.recognizes_smart_world &&
                    g_host.recognizes_smart_world(world)));
}

}  // namespace

uint64_t cuda_upload_bytes{};
uint64_t cuda_download_bytes{};
uint32_t cuda_sync_failures{};
uint32_t last_cuda_device_count{};
uint32_t last_cuda_device_index{};
uint64_t opencl_upload_bytes{};
uint64_t opencl_download_bytes{};
uint32_t opencl_sync_failures{};
uint64_t allocations_created{};
uint64_t allocations_freed{};
uint64_t invalid_memory_operations{};

void configure_host_world_fallback(HostNewWorld new_world,
                                   HostDisposeWorld dispose_world,
                                   HostOwnsWorld owns_world,
                                   HostRecognizesSmartWorld recognizes_smart_world) {
  g_host = {new_world, dispose_world, owns_world, recognizes_smart_world};
}

bool begin_cuda_context(uint32_t active_device_index) {
  const bool started = cuda_backend().begin(active_device_index);
  if (cuda_backend().device_count() != 0)
    last_cuda_device_count = cuda_backend().device_count();
  last_cuda_device_index = active_device_index;
  return started;
}

bool end_cuda_context() { return cuda_backend().end(); }

bool begin_backend_context(int32_t framework, uint32_t active_device_index) {
  if (framework == 3) return begin_cuda_context(active_device_index);
  if (framework == 1) return opencl::begin_context(active_device_index);
  if (framework == 4) return directx::begin_context(active_device_index);
  return true;
}

bool end_backend_context(int32_t framework) {
  if (framework == 3) return end_cuda_context();
  if (framework == 1) return opencl::end_context();
  if (framework == 4) return directx::end_context();
  return true;
}

void* active_cuda_context() { return cuda_backend().active_context(); }

uint32_t active_gpu_device_index() {
  if (directx::active()) return directx::active_device_index();
  return opencl::active() ? opencl::active_device_index()
                          : cuda_backend().active_device_index();
}

int32_t __cdecl gpu_acquire_exclusive(void*, uint32_t index) {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  if (index != active_gpu_device_index() || g_exclusive_access_depth == 64) {
    ++invalid_memory_operations;
    return 4;
  }
  if (cuda_backend().active() && !cuda_backend().push(index)) {
    ++invalid_memory_operations;
    return 4;
  }
  ++g_exclusive_access_depth;
  return 0;
}

int32_t __cdecl gpu_release_exclusive(void*, uint32_t index) {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  if (index != active_gpu_device_index() || g_exclusive_access_depth == 0) {
    ++invalid_memory_operations;
    return 4;
  }
  if (cuda_backend().active() && !cuda_backend().pop(index)) {
    ++invalid_memory_operations;
    return 4;
  }
  --g_exclusive_access_depth;
  return 0;
}

int32_t __cdecl gpu_allocate_device_memory(void*, uint32_t index,
                                            std::size_t size, void** memory) {
  if (!directx::active() && !opencl::active() && !cuda_backend().active())
    return allocate_fallback(index, size, memory, g_device_memory);
  if (!allocation_request_valid(index, size, memory)) return 4;
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  *memory = nullptr;
  if (!has_allocation_capacity(size)) return 4;
  if (directx::active()) {
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC description{};
    description.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    description.Width = size;
    description.Height = 1;
    description.DepthOrArraySize = 1;
    description.MipLevels = 1;
    description.SampleDesc.Count = 1;
    description.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    description.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    ID3D12Resource* resource{};
    if (FAILED(directx::device(index)->CreateCommittedResource(
            &heap, D3D12_HEAP_FLAG_NONE, &description, D3D12_RESOURCE_STATE_COMMON,
            nullptr, IID_PPV_ARGS(&resource)))) return 2;
    *memory = resource;
  } else if (opencl::active()) {
    opencl::Int error = opencl::kSuccess;
    *memory = opencl::create_buffer(index, opencl::kMemReadWrite, size, nullptr, &error);
    if (error != opencl::kSuccess || !*memory) return 2;
  } else {
    CudaDevicePointer allocated{};
    if (!cuda_backend().allocate(&allocated, size) || allocated == 0) return 2;
    *memory = reinterpret_cast<void*>(static_cast<uintptr_t>(allocated));
  }
  g_device_memory.emplace(*memory, size);
  g_memory_bytes += size;
  ++allocations_created;
  return 0;
}

int32_t __cdecl gpu_free_device_memory(void*, uint32_t index, void* memory) {
  if (!directx::active() && !opencl::active() && !cuda_backend().active())
    return free_fallback(index, memory, g_device_memory);
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  const auto found = g_device_memory.find(memory);
  if (index != active_gpu_device_index() || !memory || found == g_device_memory.end()) {
    ++invalid_memory_operations;
    return 4;
  }
  bool freed = false;
  if (directx::active()) {
    static_cast<ID3D12Resource*>(memory)->Release();
    freed = true;
  } else if (opencl::active()) {
    freed = opencl::release_mem(memory) == opencl::kSuccess;
  } else {
    freed = cuda_backend().free(static_cast<CudaDevicePointer>(
        reinterpret_cast<uintptr_t>(memory)));
  }
  if (!freed) return 4;
  g_memory_bytes -= found->second;
  g_device_memory.erase(found);
  ++allocations_freed;
  return 0;
}

int32_t __cdecl gpu_purge_memory(void*, uint32_t index, std::size_t,
                                 std::size_t* purged) {
  if (index != active_gpu_device_index()) return 4;
  if (purged) *purged = 0;
  return 0;
}

int32_t __cdecl gpu_allocate_host_memory(void*, uint32_t index,
                                          std::size_t size, void** memory) {
  if (!cuda_backend().active())
    return allocate_fallback(index, size, memory, g_host_memory);
  if (!allocation_request_valid(index, size, memory)) return 4;
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  *memory = nullptr;
  if (!has_allocation_capacity(size)) return 4;
  if (!cuda_backend().allocate_host(memory, size) || !*memory) return 2;
  g_host_memory.emplace(*memory, size);
  g_memory_bytes += size;
  ++allocations_created;
  return 0;
}

int32_t __cdecl gpu_free_host_memory(void*, uint32_t index, void* memory) {
  if (!cuda_backend().active()) return free_fallback(index, memory, g_host_memory);
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  const auto found = g_host_memory.find(memory);
  if (index != active_gpu_device_index() || !memory || found == g_host_memory.end()) {
    ++invalid_memory_operations;
    return 4;
  }
  if (!cuda_backend().free_host(memory)) return 4;
  g_memory_bytes -= found->second;
  g_host_memory.erase(found);
  ++allocations_freed;
  return 0;
}

bool gpu_memory_lifetimes_balanced() {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  return g_device_memory.empty() && g_host_memory.empty() &&
      g_memory_bytes == 0 &&
      g_exclusive_access_depth == 0;
}

std::size_t live_allocation_count() {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  return g_device_memory.size() + g_host_memory.size();
}

std::size_t live_memory_bytes() {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  return g_memory_bytes;
}

uint32_t exclusive_access_depth() {
  std::lock_guard<std::mutex> lock(g_memory_mutex);
  return g_exclusive_access_depth;
}

int32_t __cdecl gpu_create_world(void*, uint32_t index, int32_t width,
                                  int32_t height, world_safety::LocalRationalScale,
                                  int32_t, int32_t pixel_format,
                                  uint8_t clear_pixels, void** world) {
  if (index != active_gpu_device_index() || !world ||
      pixel_format != kPixelFormatGpuBgra128 || width <= 0 || height <= 0 ||
      width > kMaxWorldDimension || height > kMaxWorldDimension) return 4;
  if (!cuda_backend().active() && !opencl::active() && !directx::active()) {
    if (!g_host.new_world) return 4;
    auto descriptor = std::make_unique<std::array<std::byte, world_safety::kEffectWorldSize>>();
    const int32_t error = g_host.new_world(nullptr, width, height, clear_pixels != 0,
                                           pixel_format, descriptor->data());
    if (error != 0) return error;
    *world = descriptor.release();
    g_created_worlds.emplace(*world, index);
    return 0;
  }
  const uint64_t rowbytes64 = static_cast<uint64_t>(width) * 16;
  const uint64_t size64 = rowbytes64 * static_cast<uint64_t>(height);
  if (size64 == 0 || size64 > kMaxGpuAllocationBytes) return 4;
  auto descriptor = std::make_unique<std::array<std::byte, world_safety::kEffectWorldSize>>();
  void* pixels{};
  const int32_t error = gpu_allocate_device_memory(nullptr, index,
      static_cast<std::size_t>(size64), &pixels);
  if (error != 0) return error;
  bool initialized = directx::active();
  if (opencl::active()) {
    const unsigned char pattern = clear_pixels ? 0 : 0xCD;
    initialized = opencl::enqueue_fill(index, pixels, &pattern, sizeof(pattern), 0,
        static_cast<std::size_t>(size64)) == opencl::kSuccess;
  } else if (cuda_backend().active()) {
    initialized = cuda_backend().memset(static_cast<CudaDevicePointer>(
        reinterpret_cast<uintptr_t>(pixels)), clear_pixels ? 0 : 0xCD,
        static_cast<std::size_t>(size64));
  }
  if (!initialized) {
    gpu_free_device_memory(nullptr, index, pixels);
    return 4;
  }
  descriptor->fill(std::byte{});
  const int32_t flags = 3;
  const int32_t rowbytes = static_cast<int32_t>(rowbytes64);
  const std::array<int32_t, 4> extent{0, 0, width, height};
  const int32_t aspect_num = 1;
  const uint32_t aspect_den = 1;
  std::memcpy(descriptor->data() + 16, &flags, sizeof(flags));
  std::memcpy(descriptor->data() + 24, &pixels, sizeof(pixels));
  std::memcpy(descriptor->data() + 32, &rowbytes, sizeof(rowbytes));
  std::memcpy(descriptor->data() + 36, &width, sizeof(width));
  std::memcpy(descriptor->data() + 40, &height, sizeof(height));
  std::memcpy(descriptor->data() + 44, extent.data(), sizeof(extent));
  std::memcpy(descriptor->data() + 88, &aspect_num, sizeof(aspect_num));
  std::memcpy(descriptor->data() + 92, &aspect_den, sizeof(aspect_den));
  *world = descriptor.release();
  g_created_worlds.emplace(*world, index);
  return 0;
}

int32_t __cdecl gpu_dispose_world(void*, void* world) {
  const auto owned = g_created_worlds.find(world);
  if (!world || owned == g_created_worlds.end()) return 4;
  const uint32_t index = owned->second;
  g_created_worlds.erase(owned);
  if (g_host.owns_world && g_host.owns_world(world)) {
    if (!g_host.dispose_world) return 4;
    const int32_t error = g_host.dispose_world(nullptr, world);
    delete static_cast<std::array<std::byte, world_safety::kEffectWorldSize>*>(world);
    return error;
  }
  void* pixels{};
  std::memcpy(&pixels, static_cast<std::byte*>(world) + 24, sizeof(pixels));
  const int32_t error = gpu_free_device_memory(nullptr, index, pixels);
  std::memset(world, 0, world_safety::kEffectWorldSize);
  delete static_cast<std::array<std::byte, world_safety::kEffectWorldSize>*>(world);
  return error;
}

bool prepare_render_transport(void* input_world, void* output_world,
                              RenderTransport& transport) {
  if ((!cuda_backend().active() && !opencl::active() && !directx::active()) ||
      !input_world || !output_world) return false;
  transport.input_world = input_world;
  transport.output_world = output_world;
  if (!read_world(input_world, transport.input_host, transport.input_rowbytes,
                  transport.input_width, transport.input_height) ||
      !read_world(output_world, transport.output_host, transport.output_rowbytes,
                  transport.output_width, transport.output_height)) return false;
  const std::size_t input_size = static_cast<std::size_t>(transport.input_width) *
      transport.input_height * 16;
  const std::size_t output_size = static_cast<std::size_t>(transport.output_width) *
      transport.output_height * 16;
  const uint32_t index = active_gpu_device_index();
  if (gpu_allocate_device_memory(nullptr, index, input_size,
                                 &transport.input_device) != 0 ||
      gpu_allocate_device_memory(nullptr, index, output_size,
                                 &transport.output_device) != 0) {
    if (transport.input_device)
      gpu_free_device_memory(nullptr, index, transport.input_device);
    return false;
  }
  std::vector<float> bgra(input_size / sizeof(float));
  for (int32_t y = 0; y < transport.input_height; ++y) {
    const auto* source = static_cast<const float*>(transport.input_host) +
        static_cast<std::size_t>(y) * transport.input_rowbytes / sizeof(float);
    auto* destination = bgra.data() + static_cast<std::size_t>(y) *
        transport.input_width * 4;
    for (int32_t x = 0; x < transport.input_width; ++x) {
      destination[x * 4] = source[x * 4 + 3];
      destination[x * 4 + 1] = source[x * 4 + 2];
      destination[x * 4 + 2] = source[x * 4 + 1];
      destination[x * 4 + 3] = source[x * 4];
    }
  }
  bool uploaded = false;
  if (directx::active()) {
    std::vector<unsigned char> sentinel(output_size, 0xCC);
    uploaded = directx::copy_buffer(static_cast<ID3D12Resource*>(transport.input_device),
                                    bgra.data(), input_size, true) &&
        directx::copy_buffer(static_cast<ID3D12Resource*>(transport.output_device),
                             sentinel.data(), output_size, true);
  } else if (opencl::active()) {
    const unsigned char pattern = 0xCC;
    uploaded = opencl::enqueue_write(index, transport.input_device, 1, 0,
        input_size, bgra.data()) == opencl::kSuccess &&
        opencl::enqueue_fill(index, transport.output_device, &pattern,
        sizeof(pattern), 0, output_size) == opencl::kSuccess;
  } else {
    uploaded = cuda_backend().copy_to_device(static_cast<CudaDevicePointer>(
        reinterpret_cast<uintptr_t>(transport.input_device)), bgra.data(), input_size) &&
        cuda_backend().memset(static_cast<CudaDevicePointer>(
        reinterpret_cast<uintptr_t>(transport.output_device)), 0xCC, output_size);
  }
  if (!uploaded) {
    gpu_free_device_memory(nullptr, index, transport.output_device);
    gpu_free_device_memory(nullptr, index, transport.input_device);
    transport.input_device = transport.output_device = nullptr;
    return false;
  }
  if (directx::active()) directx::record_upload(input_size);
  else if (opencl::active()) opencl_upload_bytes += input_size;
  else cuda_upload_bytes += input_size;
  std::memcpy(static_cast<std::byte*>(input_world) + 24, &transport.input_device,
              sizeof(transport.input_device));
  std::memcpy(static_cast<std::byte*>(output_world) + 24, &transport.output_device,
              sizeof(transport.output_device));
  return true;
}

bool finish_render_transport(RenderTransport& transport) {
  if (!transport.input_device || !transport.output_device) return false;
  const uint32_t index = active_gpu_device_index();
  bool valid = directx::active() || (opencl::active()
      ? opencl::finish(index) == opencl::kSuccess : cuda_backend().synchronize());
  if (!valid) {
    if (directx::active()) directx::record_sync_failure();
    else if (opencl::active()) ++opencl_sync_failures;
    else ++cuda_sync_failures;
  }
  const std::size_t output_size = static_cast<std::size_t>(transport.output_width) *
      transport.output_height * 16;
  std::vector<float> bgra(output_size / sizeof(float));
  bool downloaded = false;
  if (valid && directx::active())
    downloaded = directx::copy_buffer(static_cast<ID3D12Resource*>(
        transport.output_device), bgra.data(), output_size, false);
  else if (valid && opencl::active())
    downloaded = opencl::enqueue_read(index, transport.output_device, 1, 0,
        output_size, bgra.data()) == opencl::kSuccess;
  else if (valid)
    downloaded = cuda_backend().copy_to_host(bgra.data(),
        static_cast<CudaDevicePointer>(reinterpret_cast<uintptr_t>(
        transport.output_device)), output_size);
  if (downloaded) {
    if (directx::active()) directx::record_download(output_size);
    else if (opencl::active()) opencl_download_bytes += output_size;
    else cuda_download_bytes += output_size;
    for (int32_t y = 0; y < transport.output_height; ++y) {
      auto* destination = static_cast<float*>(transport.output_host) +
          static_cast<std::size_t>(y) * transport.output_rowbytes / sizeof(float);
      const auto* source = bgra.data() + static_cast<std::size_t>(y) *
          transport.output_width * 4;
      for (int32_t x = 0; x < transport.output_width; ++x) {
        destination[x * 4] = source[x * 4 + 3];
        destination[x * 4 + 1] = source[x * 4 + 2];
        destination[x * 4 + 2] = source[x * 4 + 1];
        destination[x * 4 + 3] = source[x * 4];
      }
    }
  } else {
    valid = false;
  }
  std::memcpy(static_cast<std::byte*>(transport.input_world) + 24,
              &transport.input_host, sizeof(transport.input_host));
  std::memcpy(static_cast<std::byte*>(transport.output_world) + 24,
              &transport.output_host, sizeof(transport.output_host));
  valid = gpu_free_device_memory(nullptr, index, transport.output_device) == 0 && valid;
  valid = gpu_free_device_memory(nullptr, index, transport.input_device) == 0 && valid;
  transport.input_device = transport.output_device = nullptr;
  return valid;
}

int32_t __cdecl gpu_get_world_data(void*, void* world, void** pixels) {
  if (!is_active_world(world) || !pixels) return 4;
  std::memcpy(pixels, static_cast<std::byte*>(world) + 24, sizeof(void*));
  return *pixels ? 0 : 4;
}

int32_t __cdecl gpu_get_world_size(void*, void* world, std::size_t* size) {
  if (!is_active_world(world) || !size) return 4;
  int32_t rowbytes = 0, height = 0;
  std::memcpy(&rowbytes, static_cast<std::byte*>(world) + 32, sizeof(rowbytes));
  std::memcpy(&height, static_cast<std::byte*>(world) + 40, sizeof(height));
  if (rowbytes <= 0 || height <= 0 || rowbytes > kMaxWorldDimension * 16 ||
      height > kMaxWorldDimension) return 4;
  *size = static_cast<std::size_t>(rowbytes) * static_cast<std::size_t>(height);
  return 0;
}

int32_t __cdecl gpu_get_world_device_index(void*, void* world, uint32_t* index) {
  if (!is_active_world(world) || !index) return 4;
  const auto owned = g_created_worlds.find(world);
  *index = owned == g_created_worlds.end() ? active_gpu_device_index() : owned->second;
  return 0;
}

std::array<void*, 15> gpu_device_suite1{
    reinterpret_cast<void*>(&gpu_get_device_count),
    reinterpret_cast<void*>(&gpu_get_device_info),
    reinterpret_cast<void*>(&gpu_acquire_exclusive),
    reinterpret_cast<void*>(&gpu_release_exclusive),
    reinterpret_cast<void*>(&gpu_allocate_device_memory),
    reinterpret_cast<void*>(&gpu_free_device_memory),
    reinterpret_cast<void*>(&gpu_purge_memory),
    reinterpret_cast<void*>(&gpu_allocate_host_memory),
    reinterpret_cast<void*>(&gpu_free_host_memory),
    reinterpret_cast<void*>(&gpu_purge_memory),
    reinterpret_cast<void*>(&gpu_create_world),
    reinterpret_cast<void*>(&gpu_dispose_world),
    reinterpret_cast<void*>(&gpu_get_world_data),
    reinterpret_cast<void*>(&gpu_get_world_size),
    reinterpret_cast<void*>(&gpu_get_world_device_index)};

}  // namespace aexcompat::gpu_runtime::memory_world_transport
