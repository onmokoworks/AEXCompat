#include "gpu_device_info_registry.hpp"

#include <array>
#include <cstddef>
#include <cstring>
#include <iostream>

namespace aexcompat::gpu_runtime {
namespace {

struct DeviceInfoAbi {
  int32_t framework{};
  uint8_t compatible{};
  std::array<std::byte, 3> compatible_padding{};
  void* platform{};
  void* device{};
  void* context{};
  void* queue{};
  std::array<std::byte, 16> reserved{};
};

static_assert(sizeof(void*) == 8);
static_assert(sizeof(DeviceInfoAbi) == 56);
static_assert(offsetof(DeviceInfoAbi, framework) == 0);
static_assert(offsetof(DeviceInfoAbi, compatible) == 4);
static_assert(offsetof(DeviceInfoAbi, platform) == 8);
static_assert(offsetof(DeviceInfoAbi, device) == 16);
static_assert(offsetof(DeviceInfoAbi, context) == 24);
static_assert(offsetof(DeviceInfoAbi, queue) == 32);

DeviceInfoRegistry g_device_info_registry;

}  // namespace

uint32_t DeviceInfoRegistry::device_count() const { return device_count_; }
int32_t DeviceInfoRegistry::framework() const { return framework_; }

bool DeviceInfoRegistry::set_device_count(uint32_t count) {
  if (count == 0 || count > kMaxGpuDevices) return false;
  device_count_ = count;
  return true;
}

void DeviceInfoRegistry::set_framework(int32_t framework) {
  framework_ = framework;
}

bool DeviceInfoRegistry::set_device(uint32_t index, void* platform, void* device,
                                    void* context, void* queue) {
  if (index >= kMaxGpuDevices) return false;
  devices_[index] = {platform, device, context, queue};
  return true;
}

void DeviceInfoRegistry::reset_devices() {
  device_count_ = 1;
  for (auto& device : devices_) device = {};
}

int32_t DeviceInfoRegistry::write_device_info(uint32_t index, void* info) const {
  if (index >= device_count_ || !info) return 4;
  const auto& handles = devices_[index];
  DeviceInfoAbi output{};
  output.framework = framework_;
  output.compatible = 1;
  output.platform = handles.platform;
  output.device = handles.device;
  output.context = handles.context;
  output.queue = handles.queue;
  std::memcpy(info, &output, sizeof(output));
  return 0;
}

DeviceInfoRegistry& device_info_registry() { return g_device_info_registry; }

int32_t __cdecl gpu_get_device_count(void*, uint32_t* count) {
  if (!count) return 4;
  *count = device_info_registry().device_count();
  return 0;
}

int32_t __cdecl gpu_get_device_info(void*, uint32_t index, void* info) {
  std::cerr << "stage:gpu_device_info_begin\n" << std::flush;
  const int32_t error = device_info_registry().write_device_info(index, info);
  if (error != 0) return error;
  std::cerr << "stage:gpu_device_info_end error=0\n" << std::flush;
  return 0;
}

}  // namespace aexcompat::gpu_runtime
