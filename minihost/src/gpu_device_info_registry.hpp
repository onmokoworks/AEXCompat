#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::gpu_runtime {

inline constexpr uint32_t kMaxGpuDevices = 16;

class DeviceInfoRegistry {
 public:
  DeviceInfoRegistry() = default;

  uint32_t device_count() const;
  int32_t framework() const;
  bool set_device_count(uint32_t count);
  void set_framework(int32_t framework);
  bool set_device(uint32_t index, void* platform, void* device,
                  void* context, void* queue);
  void reset_devices();

  int32_t write_device_info(uint32_t index, void* info) const;

 private:
  struct DeviceHandles {
    void* platform{};
    void* device{};
    void* context{};
    void* queue{};
  };

  uint32_t device_count_{1};
  int32_t framework_{3};
  DeviceHandles devices_[kMaxGpuDevices]{};
};

DeviceInfoRegistry& device_info_registry();

int32_t __cdecl gpu_get_device_count(void* refcon, uint32_t* count);
int32_t __cdecl gpu_get_device_info(void* refcon, uint32_t index, void* info);

}  // namespace aexcompat::gpu_runtime
