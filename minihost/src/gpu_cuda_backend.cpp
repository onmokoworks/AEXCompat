#include "gpu_cuda_backend.hpp"

#include "gpu_device_info_registry.hpp"

#include <array>
#include <new>
#include <windows.h>

namespace aexcompat::gpu_runtime {
namespace {

using CuResult = int32_t;
using CuDevice = int32_t;
using CuContext = void*;
constexpr CuResult kCudaSuccess = 0;
static_assert(kMaxGpuDevices == 16);

template <typename Function>
bool load_function(HMODULE module, Function& function, const char* name) {
  function = reinterpret_cast<Function>(GetProcAddress(module, name));
  return function != nullptr;
}

CudaBackend g_cuda_backend;

}  // namespace

struct CudaBackend::State {
  HMODULE module{};
  CuResult (__stdcall* init)(uint32_t){};
  CuResult (__stdcall* device_get_count)(int32_t*){};
  CuResult (__stdcall* device_get)(CuDevice*, int32_t){};
  CuResult (__stdcall* primary_retain)(CuContext*, CuDevice){};
  CuResult (__stdcall* primary_release)(CuDevice){};
  CuResult (__stdcall* context_push)(CuContext){};
  CuResult (__stdcall* context_pop)(CuContext*){};
  CuResult (__stdcall* synchronize)(){};
  CuResult (__stdcall* mem_alloc)(CudaDevicePointer*, std::size_t){};
  CuResult (__stdcall* mem_free)(CudaDevicePointer){};
  CuResult (__stdcall* copy_host_to_device)(CudaDevicePointer, const void*, std::size_t){};
  CuResult (__stdcall* copy_device_to_host)(void*, CudaDevicePointer, std::size_t){};
  CuResult (__stdcall* memset_device)(CudaDevicePointer, unsigned char, std::size_t){};
  CuResult (__stdcall* host_alloc)(void**, std::size_t, uint32_t){};
  CuResult (__stdcall* host_free)(void*){};
  std::array<CuDevice, kMaxGpuDevices> devices{};
  std::array<CuContext, kMaxGpuDevices> contexts{};
  uint32_t device_count{};
  uint32_t active_device_index{};
  uint32_t retained_count{};
  bool pushed{};
};

bool CudaBackend::begin(uint32_t active_device_index) {
  if (active()) return true;
  State* state = new (std::nothrow) State;
  if (!state) return false;
  state_ = state;
  state->module = LoadLibraryExW(L"nvcuda.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!state->module ||
      !load_function(state->module, state->init, "cuInit") ||
      !load_function(state->module, state->device_get_count, "cuDeviceGetCount") ||
      !load_function(state->module, state->device_get, "cuDeviceGet") ||
      !load_function(state->module, state->primary_retain, "cuDevicePrimaryCtxRetain") ||
      !load_function(state->module, state->primary_release, "cuDevicePrimaryCtxRelease_v2") ||
      !load_function(state->module, state->context_push, "cuCtxPushCurrent_v2") ||
      !load_function(state->module, state->context_pop, "cuCtxPopCurrent_v2") ||
      !load_function(state->module, state->synchronize, "cuCtxSynchronize") ||
      !load_function(state->module, state->mem_alloc, "cuMemAlloc_v2") ||
      !load_function(state->module, state->mem_free, "cuMemFree_v2") ||
      !load_function(state->module, state->copy_host_to_device, "cuMemcpyHtoD_v2") ||
      !load_function(state->module, state->copy_device_to_host, "cuMemcpyDtoH_v2") ||
      !load_function(state->module, state->memset_device, "cuMemsetD8_v2") ||
      !load_function(state->module, state->host_alloc, "cuMemHostAlloc") ||
      !load_function(state->module, state->host_free, "cuMemFreeHost") ||
      state->init(0) != kCudaSuccess) {
    end();
    return false;
  }
  int32_t discovered_count = 0;
  if (state->device_get_count(&discovered_count) != kCudaSuccess ||
      discovered_count <= 0 ||
      discovered_count > static_cast<int32_t>(kMaxGpuDevices)) {
    end();
    return false;
  }
  last_discovered_device_count_ = static_cast<uint32_t>(discovered_count);
  if (active_device_index >= last_discovered_device_count_) {
    end();
    return false;
  }
  state->device_count = static_cast<uint32_t>(discovered_count);
  state->active_device_index = active_device_index;
  for (uint32_t index = 0; index < state->device_count; ++index) {
    if (state->device_get(&state->devices[index], static_cast<int32_t>(index)) != kCudaSuccess ||
        state->primary_retain(&state->contexts[index], state->devices[index]) != kCudaSuccess) {
      end();
      return false;
    }
    ++state->retained_count;
  }
  if (state->context_push(state->contexts[active_device_index]) != kCudaSuccess) {
    end();
    return false;
  }
  state->pushed = true;
  auto& registry = device_info_registry();
  registry.set_device_count(state->device_count);
  for (uint32_t index = 0; index < state->device_count; ++index) {
    registry.set_device(index, nullptr,
        reinterpret_cast<void*>(static_cast<uintptr_t>(state->devices[index])),
        state->contexts[index], nullptr);
  }
  registry.set_framework(3);
  return true;
}

bool CudaBackend::end() {
  if (!state_) return true;
  State* state = state_;
  bool valid = true;
  if (state->pushed) {
    CuContext popped{};
    valid = state->context_pop(&popped) == kCudaSuccess &&
        popped == state->contexts[state->active_device_index];
    state->pushed = false;
  }
  while (state->retained_count > 0) {
    --state->retained_count;
    valid = state->primary_release(state->devices[state->retained_count]) == kCudaSuccess && valid;
  }
  if (state->module) FreeLibrary(state->module);
  delete state;
  state_ = nullptr;
  device_info_registry().reset_devices();
  return valid;
}

bool CudaBackend::active() const { return state_ && state_->pushed; }
uint32_t CudaBackend::device_count() const {
  return state_ ? state_->device_count : last_discovered_device_count_;
}
uint32_t CudaBackend::active_device_index() const {
  return state_ ? state_->active_device_index : 0;
}
void* CudaBackend::active_context() const {
  return active() ? state_->contexts[state_->active_device_index] : nullptr;
}
bool CudaBackend::push(uint32_t index) {
  return active() && index < state_->device_count &&
      state_->context_push(state_->contexts[index]) == kCudaSuccess;
}
bool CudaBackend::pop(uint32_t index) {
  if (!active() || index >= state_->device_count) return false;
  CuContext popped{};
  return state_->context_pop(&popped) == kCudaSuccess && popped == state_->contexts[index];
}
bool CudaBackend::allocate(CudaDevicePointer* pointer, std::size_t size) const {
  return active() && state_->mem_alloc(pointer, size) == kCudaSuccess;
}
bool CudaBackend::free(CudaDevicePointer pointer) const {
  return active() && state_->mem_free(pointer) == kCudaSuccess;
}
bool CudaBackend::allocate_host(void** pointer, std::size_t size) const {
  return active() && state_->host_alloc(pointer, size, 0) == kCudaSuccess;
}
bool CudaBackend::free_host(void* pointer) const {
  return active() && state_->host_free(pointer) == kCudaSuccess;
}
bool CudaBackend::copy_to_device(CudaDevicePointer destination, const void* source,
                                std::size_t size) const {
  return active() && state_->copy_host_to_device(destination, source, size) == kCudaSuccess;
}
bool CudaBackend::copy_to_host(void* destination, CudaDevicePointer source,
                              std::size_t size) const {
  return active() && state_->copy_device_to_host(destination, source, size) == kCudaSuccess;
}
bool CudaBackend::memset(CudaDevicePointer destination, unsigned char value,
                        std::size_t size) const {
  return active() && state_->memset_device(destination, value, size) == kCudaSuccess;
}
bool CudaBackend::synchronize() const {
  return active() && state_->synchronize() == kCudaSuccess;
}

CudaBackend& cuda_backend() { return g_cuda_backend; }

}  // namespace aexcompat::gpu_runtime
