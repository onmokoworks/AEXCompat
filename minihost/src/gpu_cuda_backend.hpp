#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::gpu_runtime {

using CudaDevicePointer = uint64_t;

class CudaBackend {
 public:
  CudaBackend() = default;
  CudaBackend(const CudaBackend&) = delete;
  CudaBackend& operator=(const CudaBackend&) = delete;

  bool begin(uint32_t active_device_index);
  bool end();

  bool active() const;
  uint32_t device_count() const;
  uint32_t active_device_index() const;
  void* active_context() const;

  bool push(uint32_t device_index);
  bool pop(uint32_t device_index);
  bool allocate(CudaDevicePointer* pointer, std::size_t size) const;
  bool free(CudaDevicePointer pointer) const;
  bool allocate_host(void** pointer, std::size_t size) const;
  bool free_host(void* pointer) const;
  bool copy_to_device(CudaDevicePointer destination, const void* source,
                      std::size_t size) const;
  bool copy_to_host(void* destination, CudaDevicePointer source,
                    std::size_t size) const;
  bool memset(CudaDevicePointer destination, unsigned char value,
              std::size_t size) const;
  bool synchronize() const;

 private:
  struct State;
  State* state_{};
  uint32_t last_discovered_device_count_{};
};

CudaBackend& cuda_backend();

}  // namespace aexcompat::gpu_runtime
