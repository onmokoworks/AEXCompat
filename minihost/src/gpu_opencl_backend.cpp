#include "gpu_opencl_backend.hpp"

#include <windows.h>

#include <array>
#include <cstdint>

#include "gpu_device_info_registry.hpp"

namespace aexcompat::gpu_runtime::opencl {
namespace {

using Platform = void*;
using Device = void*;
using Context = void*;
using Queue = void*;

constexpr Ulong kDeviceTypeGpu = 1u << 2;

struct Api {
  HMODULE module{};
  Int (__stdcall* get_platform_ids)(UInt, Platform*, UInt*){};
  Int (__stdcall* get_device_ids)(Platform, Ulong, UInt, Device*, UInt*){};
  Context (__stdcall* create_context)(const intptr_t*, UInt, const Device*,
      void (__stdcall*)(const char*, const void*, std::size_t, void*), void*, Int*){};
  Int (__stdcall* release_context)(Context){};
  Queue (__stdcall* create_queue)(Context, Device, Ulong, Int*){};
  Int (__stdcall* release_queue)(Queue){};
  Mem (__stdcall* create_buffer)(Context, Ulong, std::size_t, void*, Int*){};
  Int (__stdcall* release_mem)(Mem){};
  Int (__stdcall* enqueue_write)(Queue, Mem, UInt, std::size_t, std::size_t,
                                  const void*, UInt, const void*, void*){};
  Int (__stdcall* enqueue_read)(Queue, Mem, UInt, std::size_t, std::size_t,
                                 void*, UInt, const void*, void*){};
  Int (__stdcall* enqueue_fill)(Queue, Mem, const void*, std::size_t,
                                 std::size_t, std::size_t, UInt, const void*, void*){};
  Int (__stdcall* finish)(Queue){};
  std::array<Platform, kMaxGpuDevices> platforms{};
  std::array<Device, kMaxGpuDevices> devices{};
  std::array<Context, kMaxGpuDevices> contexts{};
  std::array<Queue, kMaxGpuDevices> queues{};
  uint32_t device_count{};
  uint32_t active_device_index{};
  bool active{};
};

Api g_api;
uint32_t g_last_device_count{};
uint32_t g_last_device_index{};

template <typename Function>
bool load_function(Function& function, const char* name) {
  function = reinterpret_cast<Function>(GetProcAddress(g_api.module, name));
  return function != nullptr;
}

void unload() {
  if (g_api.module) FreeLibrary(g_api.module);
  g_api = {};
}

bool valid_device_index(uint32_t index) {
  return g_api.active && index < g_api.device_count;
}

}  // namespace

bool begin_context(uint32_t active_device_index) {
  g_api.module = LoadLibraryExW(L"OpenCL.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!g_api.module ||
      !load_function(g_api.get_platform_ids, "clGetPlatformIDs") ||
      !load_function(g_api.get_device_ids, "clGetDeviceIDs") ||
      !load_function(g_api.create_context, "clCreateContext") ||
      !load_function(g_api.release_context, "clReleaseContext") ||
      !load_function(g_api.create_queue, "clCreateCommandQueue") ||
      !load_function(g_api.release_queue, "clReleaseCommandQueue") ||
      !load_function(g_api.create_buffer, "clCreateBuffer") ||
      !load_function(g_api.release_mem, "clReleaseMemObject") ||
      !load_function(g_api.enqueue_write, "clEnqueueWriteBuffer") ||
      !load_function(g_api.enqueue_read, "clEnqueueReadBuffer") ||
      !load_function(g_api.enqueue_fill, "clEnqueueFillBuffer") ||
      !load_function(g_api.finish, "clFinish")) {
    unload();
    return false;
  }

  std::array<Platform, kMaxGpuDevices> platforms{};
  UInt platform_count = 0;
  if (g_api.get_platform_ids(kMaxGpuDevices, platforms.data(), &platform_count) != kSuccess ||
      platform_count == 0 || platform_count > kMaxGpuDevices) {
    unload();
    return false;
  }
  for (UInt platform_index = 0; platform_index < platform_count; ++platform_index) {
    UInt count = 0;
    const Int count_error = g_api.get_device_ids(
        platforms[platform_index], kDeviceTypeGpu, 0, nullptr, &count);
    if (count_error != kSuccess || count == 0) continue;
    if (count > kMaxGpuDevices - g_api.device_count) {
      unload();
      return false;
    }
    std::array<Device, kMaxGpuDevices> devices{};
    if (g_api.get_device_ids(platforms[platform_index], kDeviceTypeGpu, count,
                             devices.data(), nullptr) != kSuccess) {
      unload();
      return false;
    }
    for (UInt index = 0; index < count; ++index) {
      const uint32_t destination = g_api.device_count++;
      g_api.platforms[destination] = platforms[platform_index];
      g_api.devices[destination] = devices[index];
    }
  }

  g_last_device_count = g_api.device_count;
  g_last_device_index = active_device_index;
  if (g_api.device_count == 0 || active_device_index >= g_api.device_count) {
    unload();
    return false;
  }
  for (uint32_t index = 0; index < g_api.device_count; ++index) {
    Int error = kSuccess;
    g_api.contexts[index] = g_api.create_context(
        nullptr, 1, &g_api.devices[index], nullptr, nullptr, &error);
    if (error == kSuccess && g_api.contexts[index]) {
      g_api.queues[index] = g_api.create_queue(
          g_api.contexts[index], g_api.devices[index], 0, &error);
    }
    if (error != kSuccess || !g_api.contexts[index] || !g_api.queues[index]) {
      for (uint32_t cleanup = 0; cleanup <= index; ++cleanup) {
        if (g_api.queues[cleanup]) g_api.release_queue(g_api.queues[cleanup]);
        if (g_api.contexts[cleanup]) g_api.release_context(g_api.contexts[cleanup]);
      }
      unload();
      return false;
    }
  }

  g_api.active_device_index = active_device_index;
  g_api.active = true;
  auto& registry = device_info_registry();
  if (!registry.set_device_count(g_api.device_count)) {
    end_context();
    return false;
  }
  registry.set_framework(1);
  for (uint32_t index = 0; index < g_api.device_count; ++index) {
    if (!registry.set_device(index, g_api.platforms[index], g_api.devices[index],
                             g_api.contexts[index], g_api.queues[index])) {
      end_context();
      return false;
    }
  }
  return true;
}

bool end_context() {
  bool valid = true;
  for (uint32_t index = 0; index < g_api.device_count; ++index) {
    if (g_api.queues[index])
      valid = g_api.release_queue(g_api.queues[index]) == kSuccess && valid;
    if (g_api.contexts[index])
      valid = g_api.release_context(g_api.contexts[index]) == kSuccess && valid;
  }
  unload();
  device_info_registry().reset_devices();
  return valid;
}

bool active() { return g_api.active; }
uint32_t active_device_index() { return g_api.active_device_index; }
uint32_t device_count() { return g_api.device_count; }
uint32_t last_device_count() { return g_last_device_count; }
uint32_t last_device_index() { return g_last_device_index; }

Mem create_buffer(uint32_t index, Ulong flags, std::size_t size,
                  void* host_pointer, Int* error) {
  if (!valid_device_index(index)) {
    if (error) *error = -1;
    return nullptr;
  }
  return g_api.create_buffer(g_api.contexts[index], flags, size, host_pointer, error);
}

Int release_mem(Mem memory) { return g_api.active && memory ? g_api.release_mem(memory) : -1; }

Int enqueue_write(uint32_t index, Mem memory, UInt blocking, std::size_t offset,
                  std::size_t size, const void* source) {
  if (!valid_device_index(index)) return -1;
  return g_api.enqueue_write(g_api.queues[index], memory, blocking, offset, size,
                             source, 0, nullptr, nullptr);
}

Int enqueue_read(uint32_t index, Mem memory, UInt blocking, std::size_t offset,
                 std::size_t size, void* destination) {
  if (!valid_device_index(index)) return -1;
  return g_api.enqueue_read(g_api.queues[index], memory, blocking, offset, size,
                            destination, 0, nullptr, nullptr);
}

Int enqueue_fill(uint32_t index, Mem memory, const void* pattern,
                 std::size_t pattern_size, std::size_t offset, std::size_t size) {
  if (!valid_device_index(index)) return -1;
  return g_api.enqueue_fill(g_api.queues[index], memory, pattern, pattern_size,
                            offset, size, 0, nullptr, nullptr);
}

Int finish(uint32_t index) {
  return valid_device_index(index) ? g_api.finish(g_api.queues[index]) : -1;
}

}  // namespace aexcompat::gpu_runtime::opencl
