#include "gpu_directx_backend.hpp"

#include <array>
#include <cstring>
#include <utility>

#include <dxgi1_6.h>
#include <windows.h>

#include "gpu_device_info_registry.hpp"

namespace aexcompat::gpu_runtime::directx_backend {
namespace {

struct DirectXApi {
  HMODULE dxgi_module{};
  HMODULE d3d12_module{};
  HRESULT(WINAPI* create_factory)(REFIID, void**){};
  HRESULT(WINAPI* create_device)(IUnknown*, D3D_FEATURE_LEVEL, REFIID, void**){};
  IDXGIFactory1* factory{};
  std::array<IDXGIAdapter1*, kMaxGpuDevices> adapters{};
  std::array<ID3D12Device*, kMaxGpuDevices> devices{};
  std::array<ID3D12CommandQueue*, kMaxGpuDevices> queues{};
  uint32_t device_count{};
  uint32_t active_device_index{};
  bool active{};
};

DirectXApi g_directx;
Diagnostics g_diagnostics;

}  // namespace

bool end_context() {
  for (uint32_t index = 0; index < g_directx.device_count; ++index) {
    if (g_directx.queues[index]) g_directx.queues[index]->Release();
    if (g_directx.devices[index]) g_directx.devices[index]->Release();
    if (g_directx.adapters[index]) g_directx.adapters[index]->Release();
  }
  if (g_directx.factory) g_directx.factory->Release();
  if (g_directx.d3d12_module) FreeLibrary(g_directx.d3d12_module);
  if (g_directx.dxgi_module) FreeLibrary(g_directx.dxgi_module);
  g_directx = {};
  device_info_registry().reset_devices();
  return true;
}

bool begin_context(uint32_t active_device_index) {
  g_directx.dxgi_module =
      LoadLibraryExW(L"dxgi.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
  g_directx.d3d12_module =
      LoadLibraryExW(L"d3d12.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!g_directx.dxgi_module || !g_directx.d3d12_module) {
    end_context();
    return false;
  }
  g_directx.create_factory = reinterpret_cast<decltype(g_directx.create_factory)>(
      GetProcAddress(g_directx.dxgi_module, "CreateDXGIFactory1"));
  g_directx.create_device = reinterpret_cast<decltype(g_directx.create_device)>(
      GetProcAddress(g_directx.d3d12_module, "D3D12CreateDevice"));
  if (!g_directx.create_factory || !g_directx.create_device ||
      FAILED(g_directx.create_factory(IID_PPV_ARGS(&g_directx.factory)))) {
    end_context();
    return false;
  }
  for (uint32_t ordinal = 0; ordinal < kMaxGpuDevices; ++ordinal) {
    IDXGIAdapter1* adapter{};
    if (g_directx.factory->EnumAdapters1(ordinal, &adapter) ==
        DXGI_ERROR_NOT_FOUND)
      break;
    if (!adapter) continue;
    DXGI_ADAPTER_DESC1 description{};
    const bool usable = SUCCEEDED(adapter->GetDesc1(&description)) &&
                        (description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) == 0;
    ID3D12Device* device{};
    if (!usable || FAILED(g_directx.create_device(
                       adapter, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) {
      adapter->Release();
      continue;
    }
    D3D12_COMMAND_QUEUE_DESC queue_description{};
    queue_description.Type = D3D12_COMMAND_LIST_TYPE_COMPUTE;
    ID3D12CommandQueue* queue{};
    if (FAILED(device->CreateCommandQueue(&queue_description,
                                          IID_PPV_ARGS(&queue)))) {
      device->Release();
      adapter->Release();
      continue;
    }
    const uint32_t index = g_directx.device_count++;
    g_directx.adapters[index] = adapter;
    g_directx.devices[index] = device;
    g_directx.queues[index] = queue;
  }
  g_diagnostics.device_count = g_directx.device_count;
  g_diagnostics.device_index = active_device_index;
  if (g_directx.device_count == 0 ||
      active_device_index >= g_directx.device_count) {
    end_context();
    return false;
  }
  g_directx.active_device_index = active_device_index;
  g_directx.active = true;
  g_diagnostics.context_used = true;
  device_info_registry().set_device_count(g_directx.device_count);
  device_info_registry().set_framework(4);
  for (uint32_t index = 0; index < g_directx.device_count; ++index) {
    device_info_registry().set_device(index, nullptr, g_directx.devices[index],
                                      nullptr, g_directx.queues[index]);
  }
  return true;
}

bool active() { return g_directx.active; }

uint32_t active_device_index() { return g_directx.active_device_index; }

ID3D12Device* device(uint32_t index) {
  return index < g_directx.device_count ? g_directx.devices[index] : nullptr;
}

bool copy_buffer(ID3D12Resource* device_buffer, void* host_data,
                 std::size_t size, bool upload) {
  if (!g_directx.active || !device_buffer || !host_data || size == 0)
    return false;
  const uint32_t index = g_directx.active_device_index;
  ID3D12Device* device = g_directx.devices[index];
  ID3D12CommandAllocator* allocator{};
  ID3D12GraphicsCommandList* list{};
  ID3D12Resource* staging{};
  ID3D12Fence* fence{};
  HANDLE event = nullptr;
  bool valid = SUCCEEDED(device->CreateCommandAllocator(
      D3D12_COMMAND_LIST_TYPE_COMPUTE, IID_PPV_ARGS(&allocator)));
  if (valid)
    valid = SUCCEEDED(device->CreateCommandList(
        0, D3D12_COMMAND_LIST_TYPE_COMPUTE, allocator, nullptr,
        IID_PPV_ARGS(&list)));
  D3D12_HEAP_PROPERTIES heap{};
  heap.Type = upload ? D3D12_HEAP_TYPE_UPLOAD : D3D12_HEAP_TYPE_READBACK;
  D3D12_RESOURCE_DESC description{};
  description.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
  description.Width = size;
  description.Height = 1;
  description.DepthOrArraySize = 1;
  description.MipLevels = 1;
  description.SampleDesc.Count = 1;
  description.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
  if (valid)
    valid = SUCCEEDED(device->CreateCommittedResource(
        &heap, D3D12_HEAP_FLAG_NONE, &description,
        upload ? D3D12_RESOURCE_STATE_GENERIC_READ
               : D3D12_RESOURCE_STATE_COPY_DEST,
        nullptr, IID_PPV_ARGS(&staging)));
  if (valid && upload) {
    void* mapped{};
    valid = SUCCEEDED(staging->Map(0, nullptr, &mapped)) && mapped;
    if (valid) std::memcpy(mapped, host_data, size);
    if (mapped) staging->Unmap(0, nullptr);
  }
  const D3D12_RESOURCE_STATES copy_state =
      upload ? D3D12_RESOURCE_STATE_COPY_DEST
             : D3D12_RESOURCE_STATE_COPY_SOURCE;
  if (valid) {
    D3D12_RESOURCE_BARRIER before{};
    before.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    before.Transition.pResource = device_buffer;
    before.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    before.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
    before.Transition.StateAfter = copy_state;
    list->ResourceBarrier(1, &before);
    if (upload)
      list->CopyBufferRegion(device_buffer, 0, staging, 0, size);
    else
      list->CopyBufferRegion(staging, 0, device_buffer, 0, size);
    std::swap(before.Transition.StateBefore, before.Transition.StateAfter);
    list->ResourceBarrier(1, &before);
    valid = SUCCEEDED(list->Close());
  }
  if (valid) {
    ID3D12CommandList* lists[]{list};
    g_directx.queues[index]->ExecuteCommandLists(1, lists);
    valid = SUCCEEDED(
        device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)));
    event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    valid = valid && event &&
            SUCCEEDED(g_directx.queues[index]->Signal(fence, 1)) &&
            SUCCEEDED(fence->SetEventOnCompletion(1, event)) &&
            WaitForSingleObject(event, 30'000) == WAIT_OBJECT_0;
  }
  if (valid && !upload) {
    void* mapped{};
    D3D12_RANGE read_range{0, size};
    valid = SUCCEEDED(staging->Map(0, &read_range, &mapped)) && mapped;
    if (valid) std::memcpy(host_data, mapped, size);
    if (mapped) {
      D3D12_RANGE written_range{0, 0};
      staging->Unmap(0, &written_range);
    }
  }
  if (event) CloseHandle(event);
  if (fence) fence->Release();
  if (staging) staging->Release();
  if (list) list->Release();
  if (allocator) allocator->Release();
  return valid;
}

void record_upload(std::size_t size) { g_diagnostics.upload_bytes += size; }
void record_download(std::size_t size) { g_diagnostics.download_bytes += size; }
void record_sync_failure() { ++g_diagnostics.sync_failures; }
const Diagnostics& diagnostics() { return g_diagnostics; }

}  // namespace aexcompat::gpu_runtime::directx_backend
