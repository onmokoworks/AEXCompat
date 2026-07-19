#pragma once

#include <cstddef>
#include <cstdint>

#include <windows.h>
#include <d3d12.h>

namespace aexcompat::gpu_runtime::directx_backend {

struct Diagnostics {
  bool context_used{};
  uint32_t device_count{};
  uint32_t device_index{};
  uint64_t upload_bytes{};
  uint64_t download_bytes{};
  uint32_t sync_failures{};
};

bool begin_context(uint32_t active_device_index);
bool end_context();

bool active();
uint32_t active_device_index();
ID3D12Device* device(uint32_t index);

bool copy_buffer(ID3D12Resource* device_buffer, void* host_data,
                 std::size_t size, bool upload);

void record_upload(std::size_t size);
void record_download(std::size_t size);
void record_sync_failure();
const Diagnostics& diagnostics();

}  // namespace aexcompat::gpu_runtime::directx_backend
