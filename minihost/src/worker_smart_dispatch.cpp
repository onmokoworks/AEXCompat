#include "worker_smart_dispatch.hpp"

#include "gpu_memory_world_transport.hpp"
#include "render_subsystem.h"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cstring>
#include <iostream>
#include <memory>
#include <windows.h>

namespace aexcompat::worker_runtime::smart_dispatch {
namespace {

// What the empty-layer allocation needs, set by the dispatch and read by the
// hook it installs. Thread-local for the same reason the smart runtime's own
// state is: one worker can render on more than one thread.
struct EmptyLayerGeometry {
  int32_t width{};
  int32_t height{};
  int32_t pixel_format{};
};
thread_local EmptyLayerGeometry g_empty_layer;

class ModuleDirectoryScope {
 public:
  explicit ModuleDirectoryScope(HMODULE module) {
    const DWORD original_length = GetCurrentDirectoryW(
        static_cast<DWORD>(original_.size()), original_.data());
    const DWORD module_length = GetModuleFileNameW(
        module, module_path_.data(), static_cast<DWORD>(module_path_.size()));
    if (original_length == 0 || original_length >= original_.size() ||
        module_length == 0 || module_length >= module_path_.size())
      return;
    for (DWORD cursor = module_length; cursor > 0; --cursor) {
      if (module_path_[cursor - 1] == L'\\' || module_path_[cursor - 1] == L'/') {
        module_path_[cursor - 1] = L'\0';
        active_ = SetCurrentDirectoryW(module_path_.data()) != FALSE;
        break;
      }
    }
  }

  ~ModuleDirectoryScope() {
    if (active_) SetCurrentDirectoryW(original_.data());
  }

 private:
  bool active_{};
  std::array<wchar_t, 32768> original_{};
  std::array<wchar_t, 32768> module_path_{};
};

// `clear_pixels`, because an empty layer is transparent rather than absent.
// Failure leaves the caller without a world, and `checkout_pixels` fails closed
// on that rather than handing back an uninitialized struct.
bool allocate_empty_layer_world(void* world_storage) {
  return g_empty_layer.width > 0 && g_empty_layer.height > 0 &&
      world_registry::new_world(nullptr, g_empty_layer.width, g_empty_layer.height,
                                /*clear_pixels=*/1, g_empty_layer.pixel_format,
                                world_storage) == 0;
}

constexpr int32_t kSmartPreRender = 23;
constexpr int32_t kSmartRender = 24;
constexpr int32_t kSmartRenderGpu = 31;
constexpr int32_t kGpuDeviceSetup = 32;
constexpr int32_t kGpuDeviceSetdown = 33;

void* g_staged_cuda_module{};

void preload_staged_cuda_kernel() {
  using CuModuleLoad = int(__stdcall*)(void**, const char*);
  using CuModuleGetFunction = int(__stdcall*)(void**, void*, const char*);
  using CuModuleUnload = int(__stdcall*)(void*);
  const HMODULE cuda = GetModuleHandleW(L"nvcuda.dll");
  if (!cuda) return;
  auto module_load = reinterpret_cast<CuModuleLoad>(GetProcAddress(cuda, "cuModuleLoad"));
  auto get_function = reinterpret_cast<CuModuleGetFunction>(
      GetProcAddress(cuda, "cuModuleGetFunction"));
  auto module_unload = reinterpret_cast<CuModuleUnload>(
      GetProcAddress(cuda, "cuModuleUnload"));
  if (!module_load || !get_function || !module_unload) return;
  if (g_staged_cuda_module) {
    module_unload(g_staged_cuda_module);
    g_staged_cuda_module = nullptr;
  }
  std::array<wchar_t, 32768> path{};
  const DWORD length = GetModuleFileNameW(nullptr, path.data(),
                                          static_cast<DWORD>(path.size()));
  if (!length || length >= path.size()) return;
  std::wstring kernel_path(path.data(), length);
  const auto separator = kernel_path.find_last_of(L"\\/");
  if (separator == std::wstring::npos) return;
  kernel_path.resize(separator + 1);
  kernel_path += L"PTX\\CUDA\\ColorAndContrast.cubin";
  const int utf8_length = WideCharToMultiByte(CP_UTF8, 0, kernel_path.c_str(), -1,
                                               nullptr, 0, nullptr, nullptr);
  if (utf8_length <= 0) return;
  std::string utf8(static_cast<std::size_t>(utf8_length), '\0');
  WideCharToMultiByte(CP_UTF8, 0, kernel_path.c_str(), -1, utf8.data(),
                      utf8_length, nullptr, nullptr);
  void* module{};
  const int load_error = module_load(&module, utf8.c_str());
  void* function{};
  const int function_error = load_error == 0
      ? get_function(&function, module, "ColorAndContrastKernel")
      : -1;
  if (function) {
    g_staged_cuda_module = module;
  } else if (module) {
    module_unload(module);
  }
}
// `PF_RenderRequest` (AE_Effect.h) leads both selector inputs and is 44 bytes:
// rect at 0, `PF_Field` at 16, `PF_ChannelMask` at 20, then
// `preserve_rgb_of_zero_alpha`, padding, and reserved words. `bitdepth`
// follows it, and `PF_SmartRenderInput` continues with `pre_render_data`.
constexpr std::size_t kRenderRequestBytes = 44;
constexpr std::size_t kRenderRequestField = 16;
constexpr std::size_t kRenderRequestChannelMask = 20;
constexpr std::size_t kInputBitdepth = 44;
constexpr std::size_t kSmartInputPreRenderData = 48;
constexpr int32_t kFieldFrame = 0;
// Hypothesis, not an observation: AE is assumed to pass PF_ChannelMask_ARGB
// for an ordinary frame render, so 0xF is written. No probe has recorded what
// AE actually passes (the selector-timeline probe captures output_request.rect
// only). What is certain is that 0 reads as "no channel requested", and that
// PreRender and SmartRender must see the same value.
constexpr int32_t kChannelMaskArgb = 0xF;

// Adobe's own effects that depend on VideoFrame.dll expect a CPU SmartFX world
// to retain the private PPix backing created by that DLL.  The public
// PF_EffectWorld layout alone cannot manufacture that backing.  Keep this an
// optional dependency adapter: use only already-loaded Adobe modules, call
// their world lifecycle exports, and expose no guessed PPix layout here.
class VideoFrameCpuWorlds {
 public:
  using World = std::array<std::byte, 120>;

  bool available() {
    if (resolved_)
      return create_ && dispose_ && par_ctor_ && get_mapping_ && new_ppix_ &&
          get_frame_ && init_gpu_ && dispose_gpu_ && get_primary_device_ &&
          create_gpu_frame_ && ppix_lock_ && ppix_unlock_ && ppix_pixels_ &&
          ppix_rowbytes_ && dispose_ppix_;
    resolved_ = true;
    const HMODULE video_frame = GetModuleHandleW(L"VideoFrame.dll");
    const HMODULE dva_media_types = GetModuleHandleW(L"dvamediatypes.dll");
    const HMODULE gpu_foundation = GetModuleHandleW(L"GPUFoundation.dll");
    const HMODULE asl_foundation = GetModuleHandleW(L"ASLFoundation.dll");
    if (!video_frame || !dva_media_types || !gpu_foundation || !asl_foundation)
      return false;
    // GPUFoundation retains VideoFrame-backed device/frame objects for the
    // worker lifetime. Some effects release their own final VideoFrame import
    // during SEQUENCE_SETDOWN; without a host lifetime pin that unloads the DLL
    // while GPUFoundation still holds those objects and its next release calls
    // through unmapped VideoFrame code.
    HMODULE pinned_video_frame{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_PIN, L"VideoFrame.dll",
                            &pinned_video_frame))
      return false;
    create_ = reinterpret_cast<CreateWorld>(GetProcAddress(video_frame,
        "?NewPF_WorldWithNewVideoFrame@VF@@YA_NIIUPixelFormat@dvamediatypes@@"
        "AEBVPixelAspectRatio@3@AEAUPF_LayerDef@@@Z"));
    dispose_ = reinterpret_cast<DisposeWorld>(GetProcAddress(video_frame,
        "?DisposePF_WorldFromVideoFrame@VF@@YAXPEAUPF_LayerDef@@@Z"));
    par_ctor_ = reinterpret_cast<ParConstructor>(GetProcAddress(
        dva_media_types, "??0PixelAspectRatio@dvamediatypes@@QEAA@II@Z"));
    format_rowbytes_ = reinterpret_cast<FormatRowbytes>(GetProcAddress(
        dva_media_types,
        "?UncompressedPixelFormatRowBytes@dvamediatypes@@YA_KUPixelFormat@1@H@Z"));
    get_mapping_ = reinterpret_cast<GetWorldMapping>(GetProcAddress(video_frame,
        "?GetPFWorldFrameMapping@VF@@YA?AV?$InterfaceRef@"
        "UIVideoFrameMemoryAccess@VF@@@classref@dvacore@@PEAUPF_LayerDef@@@Z"));
    new_ppix_ = reinterpret_cast<NewPPixFromMapping>(GetProcAddress(video_frame,
        "?NewPPixFromVideoMemoryAccess@VF@@YAPEAPEAUPPix@@V?$InterfaceRef@"
        "UIVideoFrameMemoryAccess@VF@@@classref@dvacore@@@Z"));
    get_frame_ = reinterpret_cast<GetFrameFromPPix>(GetProcAddress(video_frame,
        "?GetVideoFrameFromPPix@VF@@YA?AV?$InterfaceRef@UIVideoFrame@MF@@@"
        "classref@dvacore@@PEAPEAUPPix@@@Z"));
    init_gpu_ = reinterpret_cast<InitWorldGpu>(GetProcAddress(video_frame,
        "?InitEffectWorldGPU@VF@@YAHV?$InterfaceRef@UIVideoFrame@MF@@@"
        "classref@dvacore@@AEAUPF_LayerDef@@@Z"));
    dispose_gpu_ = reinterpret_cast<DisposeWorldGpu>(GetProcAddress(video_frame,
        "?DispposeEffectWorldGPU@VF@@YAXAEAUPF_LayerDef@@@Z"));
    initialize_video_frame_ = reinterpret_cast<InitializeVideoFrame>(
        GetProcAddress(video_frame, "?Initialize@VF@@YA_NXZ"));
    shutdown_video_frame_ = reinterpret_cast<ShutdownVideoFrame>(
        GetProcAddress(video_frame, "?Shutdown@VF@@YAXXZ"));
    initialize_asl_foundation_ = reinterpret_cast<InitializeAslFoundation>(
        GetProcAddress(asl_foundation, "?Initialize@Foundation@ASL@@YAXXZ"));
    terminate_asl_foundation_ = reinterpret_cast<TerminateAslFoundation>(
        GetProcAddress(asl_foundation, "?Terminate@Foundation@ASL@@YAXXZ"));
    initialize_gpu_foundation_ = reinterpret_cast<InitializeGpuFoundation>(
        GetProcAddress(gpu_foundation,
            "?Initialize@GF@@YAX_N0000W4KernelLoadAction@1@PEAX22@Z"));
    terminate_gpu_foundation_ = reinterpret_cast<TerminateGpuFoundation>(
        GetProcAddress(gpu_foundation, "?Terminate@GF@@YAXXZ"));
    is_gpu_foundation_initialized_ =
        reinterpret_cast<IsGpuFoundationInitialized>(GetProcAddress(
            gpu_foundation, "?IsInitialized@GF@@YA_NXZ"));
    get_primary_device_ = reinterpret_cast<GetPrimaryDevice>(GetProcAddress(
        gpu_foundation,
        "?GetPrimaryDevice@Detail@GF@@YA?AV?$shared_ptr@VDevice@GF@@@std@@"
        "W4DeviceFramework@2@@Z"));
    get_device_count_ = reinterpret_cast<GetDeviceCount>(GetProcAddress(
        gpu_foundation, "?GetDeviceCount@Detail@GF@@YAIXZ"));
    get_device_ = reinterpret_cast<GetDevice>(GetProcAddress(
        gpu_foundation,
        "?GetDevice@Detail@GF@@YA?AV?$shared_ptr@VDevice@GF@@@std@@I@Z"));
    create_gpu_frame_ = reinterpret_cast<CreateGpuFrame>(GetProcAddress(video_frame,
        "?CreateGPUVideoFrame@VF@@YAHAEAV?$InterfaceRef@UIVideoFrame@MF@@@"
        "classref@dvacore@@AEBV?$shared_ptr@VDevice@GF@@@std@@UPixelFormat@"
        "dvamediatypes@@IIVPixelAspectRatio@8@W4FieldType@8@V?$PointT@H@geom@4@"
        "_K66AEBV?$shared_ptr@$$CBVRecycledMemory@allocator@dvacore@@@6@AEBV234@@Z"));
    ensure_gpu_transfer_ = reinterpret_cast<EnsureGpuTransfer>(GetProcAddress(video_frame,
        "?EnsureGPUDeviceTransferCurrent@VF@@YAHAEAV?$shared_ptr@VGPUDeviceTransfer@VF@@@"
        "std@@AEBV?$shared_ptr@VDevice@GF@@@3@@Z"));
    create_gpu_frame_from_handle_ = reinterpret_cast<CreateGpuFrameFromHandle>(
        GetProcAddress(video_frame,
            "?CreateGPUVideoFrameFromHardwareHandle@VF@@YAHAEAV?$InterfaceRef@"
            "UIVideoFrame@MF@@@classref@dvacore@@AEBV?$shared_ptr@VDevice@GF@@@std@@"
            "AEBV?$shared_ptr@VGPUDeviceTransfer@VF@@@6@UPixelFormat@dvamediatypes@@II_K@Z"));
    get_gpu_ppix_ = reinterpret_cast<GetGpuPpix>(GetProcAddress(video_frame,
        "?GetGPUPPixFromPFEffectWorld@VF@@YAPEAPEAUPPix@@PEBUPF_LayerDef@@@Z"));
    get_gpu_frame_ = reinterpret_cast<GetGpuFrame>(GetProcAddress(video_frame,
        "?GetGPUIVFFromPFEffectWorld@VF@@YA?AV?$InterfaceRef@UIVideoFrame@MF@@@"
        "classref@dvacore@@PEBUPF_LayerDef@@@Z"));
    convert_to_gpu_ = reinterpret_cast<ConvertToGpu>(GetProcAddress(video_frame,
        "?ConvertToGPUVideoFrame@HardwareResidentVideoFrame@VF@@UEAAHAEBV?$InterfaceRef@"
        "UIVideoFrame@MF@@@classref@dvacore@@W4RenderQuality@dvamediatypes@@V?$shared_ptr@"
        "UColorConvertParams@GF@@@std@@AEBVGuid@utility@5@@Z"));
    convert_to_cpu_ = reinterpret_cast<ConvertToCpu>(GetProcAddress(video_frame,
        "?ConvertToCPUVideoFrame@HardwareResidentVideoFrame@VF@@UEAAHAEAV?$InterfaceRef@"
        "UIVideoFrame@MF@@@classref@dvacore@@@Z"));
    new_ppix_from_frame_ = reinterpret_cast<NewPPixFromFrame>(GetProcAddress(video_frame,
        "?NewPPixFromVideoFrame@VF@@YAPEAPEAUPPix@@V?$InterfaceRef@UIVideoFrame@MF@@@"
        "classref@dvacore@@@Z"));
    ppix_lock_ = reinterpret_cast<PpixLock>(
        GetProcAddress(video_frame, "ppixLockPixels"));
    ppix_unlock_ = reinterpret_cast<PpixUnlock>(
        GetProcAddress(video_frame, "ppixUnlockPixels"));
    ppix_pixels_ = reinterpret_cast<PpixPixels>(
        GetProcAddress(video_frame, "ppixGetPixels"));
    ppix_rowbytes_ = reinterpret_cast<PpixRowbytes>(
        GetProcAddress(video_frame, "ppixGetRowbytes"));
    ppix_copy_ = reinterpret_cast<PpixCopy>(
        GetProcAddress(video_frame, "ppixCopy"));
    dispose_ppix_ = reinterpret_cast<DisposePpix>(GetProcAddress(video_frame,
        "?DisposePPixFromVideoFrame@VF@@YAXPEAPEAUPPix@@@Z"));
    return create_ && dispose_ && par_ctor_ && format_rowbytes_ && get_mapping_ && new_ppix_ &&
        get_frame_ && init_gpu_ && dispose_gpu_ && initialize_asl_foundation_ &&
        initialize_video_frame_ && shutdown_video_frame_ &&
        terminate_asl_foundation_ && initialize_gpu_foundation_ &&
        terminate_gpu_foundation_ && is_gpu_foundation_initialized_ &&
        get_primary_device_ && get_device_count_ && get_device_ &&
        create_gpu_frame_ && ensure_gpu_transfer_ && create_gpu_frame_from_handle_ &&
        get_gpu_ppix_ && get_gpu_frame_ && convert_to_gpu_ &&
        convert_to_cpu_ && new_ppix_from_frame_ && ppix_lock_ && ppix_unlock_ && ppix_pixels_ &&
        ppix_rowbytes_ && ppix_copy_ && dispose_ppix_;
  }

  bool create_input(int32_t width, int32_t height, const World& source,
    bool gpu, int32_t framework) {
    if (gpu) {
      if (!create_cpu(input_cpu_staging_, input_cpu_staging_live_, width,
                      height) ||
          !copy_pixels(source, input_cpu_staging_))
        return false;
      input_gpu_ = create_gpu(input_, input_live_, width, height, framework);
      if (!input_gpu_) return false;
      void** source_ppix{};
      void** destination_ppix{};
      std::memcpy(&source_ppix, input_cpu_staging_.data() + 64,
                  sizeof(source_ppix));
      std::memcpy(&destination_ppix, input_.data() + 64,
                  sizeof(destination_ppix));
      const std::array<int32_t, 4> bounds{0, 0, width, height};
      const int32_t copy_error = ppix_copy_(
          source_ppix, destination_ppix, bounds.data(), bounds.data(), 0);
      return copy_error == 0;
    }
    return create_cpu(input_, input_live_, width, height) &&
        copy_pixels(source, input_);
  }

  bool create_output(int32_t width, int32_t height, bool gpu,
                     int32_t framework) {
    if (gpu) {
      output_gpu_ = create_gpu(output_, output_live_, width, height, framework);
      return output_gpu_;
    }
    return create_cpu(output_, output_live_, width, height);
  }

  bool prepare_gpu_output_staging(int32_t width, int32_t height) {
    return output_cpu_staging_live_ ||
        create_cpu(output_cpu_staging_, output_cpu_staging_live_, width,
                   height);
  }

  World& input() { return input_; }
  World& output() { return output_; }

  bool initialize_gpu_host() {
    return initialize_gpu_foundation_at_module_root();
  }

  void* cuda_context() const { return gf_cuda_context_; }

  bool copy_output_to(World& destination) {
    if (!output_live_) return false;
    if (!output_gpu_) return copy_pixels(output_, destination);
    if (!output_cpu_staging_live_) return false;
    using CuCtxPushCurrent = int(__stdcall*)(void*);
    using CuCtxPopCurrent = int(__stdcall*)(void**);
    const HMODULE cuda = GetModuleHandleW(L"nvcuda.dll");
    const auto push_current = cuda ? reinterpret_cast<CuCtxPushCurrent>(
        GetProcAddress(cuda, "cuCtxPushCurrent_v2")) : nullptr;
    const auto pop_current = cuda ? reinterpret_cast<CuCtxPopCurrent>(
        GetProcAddress(cuda, "cuCtxPopCurrent_v2")) : nullptr;
    const bool pushed = gf_cuda_context_ && push_current && pop_current &&
        push_current(gf_cuda_context_) == 0;
    const bool copied = pushed && transfer_gpu_to_cpu(output_, destination);
    void* popped{};
    const bool popped_ok = pushed && pop_current(&popped) == 0 &&
        popped == gf_cuda_context_;
    return copied && popped_ok;
  }

  ~VideoFrameCpuWorlds() {
    if (output_live_) {
      if (output_gpu_)
        dispose_gpu_(output_.data());
      else
        dispose_(output_.data());
      output_live_ = false;
    }
    if (input_live_) {
      if (input_gpu_)
        dispose_gpu_(input_.data());
      else
        dispose_(input_.data());
      input_live_ = false;
    }
    if (output_cpu_staging_live_) {
      dispose_(output_cpu_staging_.data());
      output_cpu_staging_live_ = false;
    }
    if (input_cpu_staging_live_) {
      dispose_(input_cpu_staging_.data());
      input_cpu_staging_live_ = false;
    }
  }

 private:
  using CreateWorld = bool(__cdecl*)(uint32_t, uint32_t, uint64_t,
                                      const void*, void*);
  using DisposeWorld = void(__cdecl*)(void*);
  using ParConstructor = void*(__cdecl*)(void*, uint32_t, uint32_t);
  using FormatRowbytes = uint64_t(__cdecl*)(uint64_t, int32_t);
  using GetWorldMapping = void*(__cdecl*)(void*, void*);
  using NewPPixFromMapping = void**(__cdecl*)(void*);
  using GetFrameFromPPix = void*(__cdecl*)(void*, void**);
  using InitWorldGpu = int32_t(__cdecl*)(void*, void*);
  using DisposeWorldGpu = void(__cdecl*)(void*);
  using InitializeVideoFrame = bool(__cdecl*)();
  using ShutdownVideoFrame = void(__cdecl*)();
  using InitializeAslFoundation = void(__cdecl*)();
  using TerminateAslFoundation = void(__cdecl*)();
  using InitializeGpuFoundation = void(__cdecl*)(
      bool, bool, bool, bool, bool, int32_t, void*, void*, void*);
  using TerminateGpuFoundation = void(__cdecl*)();
  using IsGpuFoundationInitialized = bool(__cdecl*)();
  using GetPrimaryDevice = void*(__cdecl*)(void*, int32_t);
  using GetDeviceCount = uint32_t(__cdecl*)();
  using GetDevice = void*(__cdecl*)(void*, uint32_t);
  using CreateGpuFrame = int32_t(__cdecl*)(
      void*, const void*, uint64_t, uint32_t, uint32_t, const void*, int32_t,
      uint64_t, uint64_t, uint64_t, uint64_t, const void*, const void*);
  using EnsureGpuTransfer = int32_t(__cdecl*)(void*, const void*);
  using CreateGpuFrameFromHandle = int32_t(__cdecl*)(
      void*, const void*, const void*, uint64_t, uint32_t, uint32_t, uint64_t);
  using GetGpuPpix = void**(__cdecl*)(const void*);
  using GetGpuFrame = void*(__cdecl*)(void*, const void*);
  using ConvertToGpu = int32_t(__cdecl*)(
      void*, const void*, int32_t, const void*, const void*);
  using ConvertToCpu = int32_t(__cdecl*)(void*, void*);
  using NewPPixFromFrame = void**(__cdecl*)(void*);
  using PpixLock = int32_t(__cdecl*)(void**);
  using PpixUnlock = void(__cdecl*)(void**);
  using PpixPixels = char*(__cdecl*)(void**);
  using PpixRowbytes = int32_t(__cdecl*)(void**);
  using PpixCopy = int32_t(__cdecl*)(void**, void**, const int32_t*,
                                     const int32_t*, int32_t);
  using DisposePpix = void(__cdecl*)(void**);

  static int32_t world_i32(const World& world, std::size_t offset) {
    int32_t value{};
    std::memcpy(&value, world.data() + offset, sizeof(value));
    return value;
  }

  static void* world_pixels(const World& world) {
    void* value{};
    std::memcpy(&value, world.data() + 24, sizeof(value));
    return value;
  }

  static void release_interface_ref(std::array<std::byte, 24>& ref) {
    void* control{};
    std::memcpy(&control, ref.data() + 16, sizeof(control));
    if (control) {
      auto* counts = static_cast<volatile long*>(control);
      if (InterlockedDecrement(counts + 2) == 0) {
        void** vtable{};
        std::memcpy(&vtable, control, sizeof(vtable));
        reinterpret_cast<void(__cdecl*)(void*)>(vtable[0])(control);
        if (InterlockedDecrement(counts + 3) == 0)
          reinterpret_cast<void(__cdecl*)(void*)>(vtable[1])(control);
      }
    }
    ref.fill({});
  }

  static void* interface_pointer(const std::array<std::byte, 24>& ref) {
    void* pointer{};
    std::memcpy(&pointer, ref.data(), sizeof(pointer));
    return pointer;
  }

  static bool copy_pixels(const World& source, World& destination) {
    const int32_t width = world_i32(source, 36);
    const int32_t height = world_i32(source, 40);
    const int32_t source_rowbytes = world_i32(source, 32);
    const int32_t destination_rowbytes = world_i32(destination, 32);
    if (width <= 0 || height <= 0 || width != world_i32(destination, 36) ||
        height != world_i32(destination, 40) || source_rowbytes < width * 16 ||
        destination_rowbytes < width * 16) return false;
    const auto* source_pixels = static_cast<const std::byte*>(world_pixels(source));
    auto* destination_pixels = static_cast<std::byte*>(world_pixels(destination));
    if (!source_pixels || !destination_pixels) return false;
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(destination_pixels + static_cast<std::size_t>(y) * destination_rowbytes,
                  source_pixels + static_cast<std::size_t>(y) * source_rowbytes,
                  static_cast<std::size_t>(width) * 16);
    return true;
  }

  bool copy_pixels_to_ppix(const World& source, World& destination) const {
    void** ppix{};
    std::memcpy(&ppix, destination.data() + 64, sizeof(ppix));
    if (!ppix || ppix_lock_(ppix) != 0) return false;
    const auto* source_pixels = static_cast<const std::byte*>(world_pixels(source));
    auto* destination_pixels = reinterpret_cast<std::byte*>(ppix_pixels_(ppix));
    const int32_t width = world_i32(source, 36);
    const int32_t height = world_i32(source, 40);
    const int32_t source_rowbytes = world_i32(source, 32);
    const int32_t destination_rowbytes = source_rowbytes;
    const bool valid = source_pixels && destination_pixels && width > 0 && height > 0 &&
        source_rowbytes >= width * 16 && destination_rowbytes >= width * 16;
    if (valid)
      for (int32_t y = 0; y < height; ++y)
        std::memcpy(destination_pixels + static_cast<std::size_t>(y) * destination_rowbytes,
                    source_pixels + static_cast<std::size_t>(y) * source_rowbytes,
                    static_cast<std::size_t>(width) * 16);
    ppix_unlock_(ppix);
    return valid;
  }

  bool create_cpu(World& world, bool& live, int32_t width, int32_t height,
                  uint64_t pixel_format = 0x0008213A62675241ULL) {
    if (!available() || width <= 0 || height <= 0) return false;
    // The constructor's storage is deliberately opaque.  Only the constructor
    // and the const-reference consumer touch it.
    alignas(8) std::array<std::byte, 32> square_par{};
    par_ctor_(square_par.data(), 1, 1);
    world.fill({});
    // DVA's enumerated 128-bit ARGB format (`ARgb`), verified through
    // UncompressedPixelFormatRowBytes as 16 bytes per pixel.
    live = create_(static_cast<uint32_t>(width), static_cast<uint32_t>(height),
                   pixel_format, square_par.data(), world.data());
    if (!live || !world_pixels(world) || world_i32(world, 36) != width ||
        world_i32(world, 40) != height || world_i32(world, 32) < width * 16)
      return false;
    // ColorAndContrast reads PF_LayerDef::platform_ref (+64) as the PPix
    // handle. Build that handle from the VideoFrame memory mapping rather than
    // inventing PPix storage or treating the public pixel pointer as one.
    alignas(8) std::array<std::byte, 24> mapping{};
    get_mapping_(mapping.data(), world.data());
    void** ppix = new_ppix_(mapping.data());
    if (!ppix) return false;
    std::memcpy(world.data() + 64, &ppix, sizeof(ppix));
    return true;
  }

  bool initialize_gpu_foundation_at_module_root() {
    if (is_gpu_foundation_initialized_()) return true;
    initialize_asl_foundation_();
    owns_asl_foundation_ = true;
    std::array<wchar_t, 32768> original_directory{};
    std::array<wchar_t, 32768> module_path{};
    const DWORD original_length = GetCurrentDirectoryW(
        static_cast<DWORD>(original_directory.size()), original_directory.data());
    const DWORD module_length = GetModuleFileNameW(
        GetModuleHandleW(L"GPUFoundation.dll"), module_path.data(),
        static_cast<DWORD>(module_path.size()));
    bool changed_directory = false;
    if (original_length > 0 && original_length < original_directory.size() &&
        module_length > 0 && module_length < module_path.size()) {
      for (DWORD cursor = module_length; cursor > 0; --cursor) {
        if (module_path[cursor - 1] == L'\\' || module_path[cursor - 1] == L'/') {
          module_path[cursor - 1] = L'\0';
          changed_directory = SetCurrentDirectoryW(module_path.data()) != FALSE;
          break;
        }
      }
    }
    initialize_gpu_foundation_(true, true, true, true, true,
                               /*kernel_load_action=*/1,
                               nullptr, nullptr, nullptr);
    // GF creates and leaves its private CUDA context current. The transport
    // starts from the primary context immediately after this adapter returns;
    // keep the two owners separate by removing GF's context from this thread's
    // stack without destroying it.
    if (const HMODULE cuda = GetModuleHandleW(L"nvcuda.dll")) {
      using CuCtxGetCurrent = int(__stdcall*)(void**);
      using CuCtxPopCurrent = int(__stdcall*)(void**);
      const auto get_current = reinterpret_cast<CuCtxGetCurrent>(
          GetProcAddress(cuda, "cuCtxGetCurrent"));
      const auto pop_current = reinterpret_cast<CuCtxPopCurrent>(
          GetProcAddress(cuda, "cuCtxPopCurrent_v2"));
      void* current{};
      if (get_current && pop_current && get_current(&current) == 0 && current) {
        void* popped{};
        const int32_t pop_error = pop_current(&popped);
        if (pop_error == 0 && popped == current) gf_cuda_context_ = current;
      }
    }
    if (changed_directory)
      SetCurrentDirectoryW(original_directory.data());
    owns_gpu_foundation_ = is_gpu_foundation_initialized_();
    return owns_gpu_foundation_;
  }

  bool create_gpu(World& world, bool& live, int32_t width, int32_t height,
                  int32_t framework) {
    if (!available() || width <= 0 || height <= 0) return false;
    if (!initialize_gpu_foundation_at_module_root()) return false;
    if (!owns_video_frame_)
      owns_video_frame_ = initialize_video_frame_();
    if (!owns_video_frame_) return false;
    std::shared_ptr<void> device;
    // PF_GPU_Framework uses CUDA=3 and DirectX=4, while GPUFoundation's
    // DeviceFramework uses CUDA=0 and DirectX=3. Passing PF's value through
    // selected the DirectX module manager for a CUDA render, so it searched
    // PTX/HLSL and could never discover the plug-in's CUDA module.
    const int32_t gf_framework =
        framework == 3 ? 0 : framework == 4 ? 3 : framework;
    get_primary_device_(&device, gf_framework);
    if (!device) {
      const int32_t expected_internal_framework =
          framework == 3 ? 0 : framework == 1 ? 1 : framework == 4 ? 3 : -1;
      const uint32_t device_count = get_device_count_();
      for (uint32_t index = 0; index < device_count && !device; ++index) {
        std::shared_ptr<void> candidate_device;
        get_device_(&candidate_device, index);
        if (!candidate_device) continue;
        int32_t candidate_framework{};
        std::memcpy(&candidate_framework,
                    static_cast<const std::byte*>(candidate_device.get()) + 0x74,
                    sizeof(candidate_framework));
        if (candidate_framework == expected_internal_framework)
          device = std::move(candidate_device);
      }
    }
    if (!device) return false;
    alignas(8) std::array<std::byte, 32> square_par{};
    par_ctor_(square_par.data(), 1, 1);
    const std::shared_ptr<const void> no_recycled_memory;
    alignas(8) std::array<std::byte, 24> frame{};
    constexpr uint64_t kDvaArgbFloat = 0x0008213A62675241ULL;
    int32_t create_error{};
    try {
      create_error = create_gpu_frame_(
          frame.data(), &device, kDvaArgbFloat, static_cast<uint32_t>(width),
          static_cast<uint32_t>(height), square_par.data(), /*field_type=*/0,
          /*origin=*/0, /*render_time=*/0, /*reserved_size_1=*/0,
          /*reserved_size_2=*/0, &no_recycled_memory, &no_recycled_memory);
    } catch (const std::exception&) {
      return false;
    } catch (...) {
      return false;
    }
    if (create_error != 0) return false;
    void** ppix = new_ppix_from_frame_(frame.data());
    world.fill({});
    const int32_t init_error = init_gpu_(frame.data(), world.data());
    live = init_error == 0;
    if (live) {
      if (!ppix) ppix = get_gpu_ppix_(world.data());
      std::memcpy(world.data() + 64, &ppix, sizeof(ppix));
    }
    return live && !world_pixels(world) && world_i32(world, 36) == width &&
        world_i32(world, 40) == height;
  }

  bool transfer_gpu_to_cpu(const World& gpu, World& destination) const {
    void** source_ppix{};
    void** staging_ppix{};
    std::memcpy(&source_ppix, gpu.data() + 64, sizeof(source_ppix));
    std::memcpy(&staging_ppix, output_cpu_staging_.data() + 64,
                sizeof(staging_ppix));
    const int32_t width = world_i32(destination, 36);
    const int32_t height = world_i32(destination, 40);
    const std::array<int32_t, 4> bounds{0, 0, width, height};
    const int32_t error = source_ppix && staging_ppix
        ? ppix_copy_(source_ppix, staging_ppix, bounds.data(), bounds.data(), 0)
        : -1;
    return error == 0 && copy_pixels(output_cpu_staging_, destination);
  }

  bool resolved_{};
  bool input_live_{};
  bool output_live_{};
  bool input_cpu_staging_live_{};
  bool output_cpu_staging_live_{};
  CreateWorld create_{};
  DisposeWorld dispose_{};
  ParConstructor par_ctor_{};
  FormatRowbytes format_rowbytes_{};
  GetWorldMapping get_mapping_{};
  NewPPixFromMapping new_ppix_{};
  GetFrameFromPPix get_frame_{};
  InitWorldGpu init_gpu_{};
  DisposeWorldGpu dispose_gpu_{};
  InitializeVideoFrame initialize_video_frame_{};
  ShutdownVideoFrame shutdown_video_frame_{};
  InitializeAslFoundation initialize_asl_foundation_{};
  TerminateAslFoundation terminate_asl_foundation_{};
  InitializeGpuFoundation initialize_gpu_foundation_{};
  TerminateGpuFoundation terminate_gpu_foundation_{};
  IsGpuFoundationInitialized is_gpu_foundation_initialized_{};
  GetPrimaryDevice get_primary_device_{};
  GetDeviceCount get_device_count_{};
  GetDevice get_device_{};
  CreateGpuFrame create_gpu_frame_{};
  EnsureGpuTransfer ensure_gpu_transfer_{};
  CreateGpuFrameFromHandle create_gpu_frame_from_handle_{};
  GetGpuPpix get_gpu_ppix_{};
  GetGpuFrame get_gpu_frame_{};
  ConvertToGpu convert_to_gpu_{};
  ConvertToCpu convert_to_cpu_{};
  NewPPixFromFrame new_ppix_from_frame_{};
  PpixLock ppix_lock_{};
  PpixUnlock ppix_unlock_{};
  PpixPixels ppix_pixels_{};
  PpixRowbytes ppix_rowbytes_{};
  PpixCopy ppix_copy_{};
  DisposePpix dispose_ppix_{};
  bool input_gpu_{};
  bool output_gpu_{};
  bool owns_gpu_foundation_{};
  bool owns_asl_foundation_{};
  bool owns_video_frame_{};
  void* gf_cuda_context_{};
  World input_{};
  World output_{};
  World input_cpu_staging_{};
  World output_cpu_staging_{};
};

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& buffer, std::size_t offset, T value) {
  std::memcpy(buffer.data() + offset, &value, sizeof(value));
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& buffer, std::size_t offset) {
  T value{};
  std::memcpy(&value, buffer.data() + offset, sizeof(value));
  return value;
}

void write_render_request(std::byte* destination,
                          const std::array<int32_t, 4>& rect) {
  std::memset(destination, 0, kRenderRequestBytes);
  std::memcpy(destination, rect.data(), sizeof(rect));
  const int32_t field = kFieldFrame;
  const int32_t channel_mask = kChannelMaskArgb;
  std::memcpy(destination + kRenderRequestField, &field, sizeof(field));
  std::memcpy(destination + kRenderRequestChannelMask, &channel_mask,
              sizeof(channel_mask));
}

}  // namespace

SelectorInputs build_selector_inputs(const std::array<int32_t, 4>& request_rect,
                                     int16_t bitdepth, void* pre_render_data) {
  SelectorInputs inputs{};
  write_render_request(inputs.pre_render.data(), request_rect);
  write<int16_t>(inputs.pre_render, kInputBitdepth, bitdepth);
  write_render_request(inputs.smart_render.data(), request_rect);
  write<int16_t>(inputs.smart_render, kInputBitdepth, bitdepth);
  write<void*>(inputs.smart_render, kSmartInputPreRenderData, pre_render_data);
  return inputs;
}

SelectorInputLayout selector_input_layout() {
  return {static_cast<int32_t>(kRenderRequestBytes),
          static_cast<int32_t>(kRenderRequestField),
          static_cast<int32_t>(kRenderRequestChannelMask),
          static_cast<int32_t>(kInputBitdepth),
          static_cast<int32_t>(kSmartInputPreRenderData)};
}

bool verify_selector_inputs() {
  const std::array<int32_t, 4> rect{3, 2, 11, 8};
  int32_t pre_render_marker = 0;
  for (const int16_t bitdepth : {int16_t{8}, int16_t{16}, int16_t{32}}) {
    const SelectorInputs inputs =
        build_selector_inputs(rect, bitdepth, &pre_render_marker);
    // The regression this guards is asymmetry: SmartRender used to get a zeroed
    // prefix while PreRender got the real one. Compare the two byte for byte
    // over the shared prefix rather than re-reading each field with the same
    // constants that wrote it.
    if (!std::equal(inputs.pre_render.begin(),
                    inputs.pre_render.begin() + kInputBitdepth + sizeof(bitdepth),
                    inputs.smart_render.begin()))
      return false;
    std::array<int32_t, 4> observed{};
    std::memcpy(observed.data(), inputs.smart_render.data(), sizeof(observed));
    if (observed != rect) return false;
    if (read<int32_t>(inputs.smart_render, kRenderRequestField) != kFieldFrame)
      return false;
    if (read<int32_t>(inputs.smart_render, kRenderRequestChannelMask) !=
        kChannelMaskArgb)
      return false;
    if (read<int16_t>(inputs.smart_render, kInputBitdepth) != bitdepth) return false;
    // The request prefix must stop before `pre_render_data`: PreRender's pointer
    // has to reach SmartRender intact.
    if (read<void*>(inputs.smart_render, kSmartInputPreRenderData) !=
        static_cast<void*>(&pre_render_marker))
      return false;
    // Nothing past the pointer belongs to this builder.
    if (!std::all_of(inputs.smart_render.begin() + kSmartInputPreRenderData +
                         sizeof(void*),
                     inputs.smart_render.end(),
                     [](std::byte value) { return value == std::byte{}; }))
      return false;
  }
  return true;
}

bool dispatch(const Request& request, const Hooks& hooks,
              smart_execution::Result& result, State& dispatch_state) {
  if (!request.entry || !request.input || !request.output || !request.plan ||
      !request.parameters || !request.input_world || !request.output_world ||
      !request.formats || !request.guarded || !request.destination ||
      !hooks.guarded_call || !hooks.capture_module_audit ||
      !hooks.guid_mix_in_callback ||
      !hooks.automatic_checkin)
    return false;

  const auto& plan = *request.plan;
  auto& runtime = smart::state();
  auto& params = request.parameters->params;
  namespace transport = gpu_runtime::memory_world_transport;

  std::array<std::byte, 8> gpu_setup_input{}, gpu_setup_output{};
  std::array<std::byte, 16> gpu_setup_extra{};
  const int32_t gpu_framework =
      (plan.fixture_gpu_negotiation || plan.directx_gpu_negotiation)
          ? 4
          : (plan.opencl_gpu_negotiation ? 1 : 3);
  const bool video_frame_modules_loaded =
      GetModuleHandleW(L"VideoFrame.dll") && GetModuleHandleW(L"dvamediatypes.dll");
  const bool use_video_frame_worlds = plan.float32 && !plan.missing_input &&
      video_frame_modules_loaded;
  const bool use_transport = plan.gpu_negotiation &&
      (gpu_framework == 1 || gpu_framework == 3 || gpu_framework == 4);
  VideoFrameCpuWorlds video_frame_worlds;
  const bool video_frame_adapter_ready =
      use_video_frame_worlds && video_frame_worlds.available();
  const bool gpu_host_ready = !plan.gpu_negotiation ||
      !video_frame_adapter_ready || video_frame_worlds.initialize_gpu_host();
  if (plan.gpu_negotiation) hooks.capture_module_audit();
  // CPU renders must not touch any GPU backend: an unconditional context
  // start loads the CUDA runtime (nvcuda.dll plus NVIDIA driver-store
  // DLLs) that the module audit cannot classify, failing every sealed
  // smart dispatch on NVIDIA machines even for pure CPU plans (issue #185).
  const bool gpu_context_started = plan.gpu_negotiation && gpu_host_ready &&
      transport::begin_backend_context(
          gpu_framework, plan.gpu_device_index,
          gpu_framework == 3 ? video_frame_worlds.cuda_context() : nullptr);
  if (plan.gpu_negotiation) {
    write<int32_t>(gpu_setup_input, 0, gpu_framework);
    write<uint32_t>(gpu_setup_input, 4, plan.gpu_device_index);
    write<void*>(gpu_setup_extra, 0, gpu_setup_input.data());
    write<void*>(gpu_setup_extra, 8, gpu_setup_output.data());
    std::cerr << "stage:gpu_device_setup_begin\n" << std::flush;
    {
      ModuleDirectoryScope gpu_module_directory(
          GetModuleHandleW(L"GPUFoundation.dll"));
      result.gpu_setup_error = gpu_context_started
          ? hooks.guarded_call(request.entry, kGpuDeviceSetup,
                               request.input->data(), request.output->data(),
                               params.data(), nullptr, gpu_setup_extra.data())
          : -6;
    }
    std::cerr << "stage:gpu_device_setup_end error=" << result.gpu_setup_error
              << "\n" << std::flush;
  }

  std::array<std::byte, 16> pre_callbacks{};
  std::array<std::byte, 24> pre_extra{};
  const std::array<int32_t, 4> expected_request = plan.partial_output_request
      ? std::array<int32_t, 4>{3, 2, 11, 8}
      : std::array<int32_t, 4>{0, 0, plan.width, plan.height};
  const int16_t render_bitdepth = plan.float32 ? 32 : (plan.deep16 ? 16 : 8);
  // Both selector inputs come from one builder, so SmartRender cannot be handed
  // a different request or bitdepth than PreRender was (issue #699).
  // `pre_render_data` is only known after PreRender ran; it is written into the
  // SmartRender copy below.
  SelectorInputs selector_inputs =
      build_selector_inputs(expected_request, render_bitdepth, nullptr);
  std::array<std::byte, 64>& pre_input = selector_inputs.pre_render;
  if (plan.gpu_negotiation) {
    write<void*>(pre_input, 48, read<void*>(gpu_setup_output, 0));
    write<int32_t>(pre_input, 56, gpu_framework);
    write<uint32_t>(pre_input, 60, plan.gpu_device_index);
  }
  write<void*>(pre_callbacks, 0, reinterpret_cast<void*>(&smart::pre_checkout_layer));
  write<void*>(pre_callbacks, 8, hooks.guid_mix_in_callback);
  write<void*>(pre_extra, 0, pre_input.data());
  write<void*>(pre_extra, 8, dispatch_state.pre_output.data());
  write<void*>(pre_extra, 16, pre_callbacks.data());
  runtime.input_checkout_request.fill(-1);
  runtime.map_checkout_request.fill(-1);
  runtime.secondary_checkout_id = -1;
  runtime.width = plan.width;
  runtime.height = plan.height;
  runtime.rowbytes = plan.rowbytes;
  // `params` holds the input plus one entry per declared parameter, matching
  // the SDK's "0 = input, 1..n = param" indexing for checkout_layer.
  runtime.param_count = params.empty()
      ? 0 : static_cast<int32_t>(params.size() - 1);
  runtime.pixel_format = plan.float32 ? "argb32f" : (plan.deep16 ? "argb16" : "argb8");
  if (video_frame_adapter_ready && !plan.gpu_negotiation &&
      !video_frame_worlds.create_input(plan.width, plan.height,
                                       *request.input_world,
                                       plan.gpu_negotiation, gpu_framework))
    return false;
  // Before the selector, not after it: PreRender is where a SmartFX plug-in
  // checks its input out, and the registration that checkout leaves behind is
  // what SmartRender later hands pixels from. Assigning this only in the render
  // preamble below registered a null world for the whole negotiation (issue
  // #675). The secondary and hosted-layer worlds are already set up before
  // dispatch, so this brings the primary input in line with them.
  //
  // `output_world` deliberately stays below: nothing in PreRender reads it, and
  // publishing it early would also widen the GPU world registry's view of it
  // before the transport that backs it is prepared.
  runtime.input_world = plan.missing_input ? nullptr :
      (video_frame_adapter_ready && !plan.gpu_negotiation
          ? video_frame_worlds.input().data()
                              : request.input_world->data());
  if (video_frame_adapter_ready && !plan.gpu_negotiation) {
    // VideoFrame owns more than the public struct contents. Return the exact
    // PF_LayerDef address it created instead of the ordinary checkout-view
    // copy; its private world registry may key the PPix association by world.
    runtime.input_checkout_view_world = nullptr;
    if (!request.parameters->definitions.empty() &&
        request.parameters->definitions[0].size() >= 56 +
            video_frame_worlds.input().size())
      std::memcpy(request.parameters->definitions[0].data() + 56,
                  video_frame_worlds.input().data(),
                  video_frame_worlds.input().size());
  }
  // The layer a plug-in gets when it checks out a layer parameter this host has
  // no layer for, as a hook the runtime calls on the first checkout that needs
  // one. Through the host's own new-world path, so the world registry owns it
  // and the host's own callbacks resolve it: copying or sampling an empty layer
  // is an ordinary thing for a plug-in to do, and `PF_COPY` resolves its
  // arguments through that registry (issue #962).
  //
  // On first need rather than here, because that registry is bounded (256 MB,
  // 64 worlds) and shared with the plug-in's own PF_NEW_WORLD: a full frame
  // taken on every dispatch is a full frame taken from an effect building a
  // scratch pyramid, on frames where nothing asks for an empty layer at all.
  g_empty_layer.width = plan.width;
  g_empty_layer.height = plan.height;
  g_empty_layer.pixel_format = plan.float32 ? world_registry::kPixelFormatArgb128
      : (plan.deep16 ? world_registry::kPixelFormatArgb64
                     : world_registry::kPixelFormatArgb32);
  runtime.allocate_empty_layer = &allocate_empty_layer_world;
  std::cerr << "stage:smart_pre_render_begin\n" << std::flush;
  result.pre_error = (!plan.gpu_negotiation || result.gpu_setup_error == 0)
      ? hooks.guarded_call(request.entry, kSmartPreRender, request.input->data(),
                           request.output->data(), params.data(), nullptr,
                           pre_extra.data())
      : -1;
  std::cerr << "stage:smart_pre_render_end error=" << result.pre_error << "\n"
            << std::flush;
  hooks.automatic_checkin();

  const render::SmartOutputBounds smart_bounds = render::prepare_smart_output_bounds(
      dispatch_state.pre_output.data(), dispatch_state.pre_output.size(), plan.pixel_bytes);
  result.result_rect = smart_bounds.result_rect;
  result.max_result_rect = smart_bounds.max_result_rect;
  result.rects_valid = result.pre_error == 0 && smart_bounds.valid;
  result.empty_result_rect = result.rects_valid && smart_bounds.empty_result;
  result.returns_extra_pixels =
      (read<uint16_t>(dispatch_state.pre_output, 34) & 0x1u) != 0;
  // Without RETURNS_EXTRA_PIXELS the SDK does not admit result > request. The
  // overrun is surfaced as an explicit diagnostic rather than a render
  // failure: AE silently clips, and blocking here would turn an observable
  // compatibility gap into a dead end for real-AEX observation.
  result.result_within_request =
      render::smart_rect_contained(smart_bounds.result_rect, expected_request);
  result.extra_pixels_contract_violation = result.rects_valid &&
      !result.returns_extra_pixels && !result.result_within_request;
  if (result.rects_valid && !result.empty_result_rect) {
    if (!request.guarded->reset(
            static_cast<std::size_t>(smart_bounds.rowbytes) * smart_bounds.height)) {
      result.rects_valid = false;
      result.pre_error = -3;
    }
    *request.destination = request.guarded->data();
    if (!render::prepare_world_layout(
            *request.output_world,
            {(plan.deep16 || plan.float32) ? 1 : 0, plan.pixel_bytes,
             smart_bounds.width, smart_bounds.height, smart_bounds.rowbytes},
            *request.destination) ||
        !request.formats->register_world(request.output_world->data(),
                                         request.dispatch_pixel_format))
      result.rects_valid = false;
    // AE 25.3 observation (issue #102): the output world carries the
    // result_rect top-left as PF_LayerDef::origin_x/origin_y (offset 104/108),
    // and in_data.output_origin (276/280) is the position of the layer origin
    // inside that buffer, i.e. the negated result_rect top-left.
    write<int32_t>(*request.output_world, 104, smart_bounds.origin_x);
    write<int32_t>(*request.output_world, 108, smart_bounds.origin_y);
    write<int32_t>(*request.input, 276, -smart_bounds.result_rect[0]);
    write<int32_t>(*request.input, 280, -smart_bounds.result_rect[1]);
    result.output_width = smart_bounds.width;
    result.output_height = smart_bounds.height;
    result.output_rowbytes = smart_bounds.rowbytes;
    if (video_frame_adapter_ready && !plan.gpu_negotiation) {
      if (!video_frame_worlds.create_output(smart_bounds.width,
                                            smart_bounds.height,
                                            plan.gpu_negotiation,
                                            gpu_framework) ||
          !(plan.gpu_negotiation
                ? request.formats->register_gpu_world(
                      video_frame_worlds.output().data(),
                      world_registry::kPixelFormatGpuBgra128)
                : request.formats->register_world(
                      video_frame_worlds.output().data(),
                      world_registry::kPixelFormatArgb128))) {
        result.rects_valid = false;
      } else {
        write<int32_t>(video_frame_worlds.output(), 104, smart_bounds.origin_x);
        write<int32_t>(video_frame_worlds.output(), 108, smart_bounds.origin_y);
      }
    }
  }
  result.roi_contract_valid = !plan.partial_output_request ||
      (runtime.input_checkout_request == expected_request &&
       runtime.map_checkout_request == expected_request &&
       result.result_rect == expected_request &&
       result.max_result_rect == expected_request);
  result.gpu_render_possible =
      (read<uint16_t>(dispatch_state.pre_output, 34) & 0x2u) != 0;
  result.checkout_time = runtime.checkout_time;
  result.checkout_time_step = runtime.checkout_time_step;
  result.checkout_time_scale = runtime.checkout_time_scale;

  std::array<std::byte, 24> callbacks{};
  std::array<std::byte, 16> smart_extra{};
  std::array<std::byte, 72>& smart_input = selector_inputs.smart_render;
  write<void*>(smart_input, kSmartInputPreRenderData,
               read<void*>(dispatch_state.pre_output, 40));
  if (plan.gpu_negotiation) {
    write<void*>(smart_input, 56, read<void*>(gpu_setup_output, 0));
    write<int32_t>(smart_input, 64, gpu_framework);
    write<uint32_t>(smart_input, 68, plan.gpu_device_index);
  }
  write<void*>(callbacks, 0, reinterpret_cast<void*>(&smart::checkout_pixels));
  write<void*>(callbacks, 8, reinterpret_cast<void*>(&smart::checkin_pixels));
  write<void*>(callbacks, 16, reinterpret_cast<void*>(&smart::checkout_output));
  write<void*>(smart_extra, 0, smart_input.data());
  write<void*>(smart_extra, 8, callbacks.data());
  runtime.output_world = video_frame_adapter_ready && !plan.gpu_negotiation
      ? video_frame_worlds.output().data() : request.output_world->data();
  if (plan.gpu_negotiation && !video_frame_adapter_ready &&
      ((!plan.missing_input && !request.formats->register_world(
            runtime.input_world, world_registry::kPixelFormatGpuBgra128)) ||
       !request.formats->register_world(
            runtime.output_world, world_registry::kPixelFormatGpuBgra128)))
    result.pre_error = 4;

  const int32_t render_selector = plan.gpu_negotiation && result.gpu_render_possible
      ? kSmartRenderGpu : kSmartRender;
  // One predicate drives the selector call, the GPU transport, and the
  // dispatch reporting, so a skipped render (empty result or rejected
  // geometry) never prepares device transport or claims a GPU dispatch.
  const bool will_dispatch = result.pre_error == 0 && result.rects_valid &&
      !result.empty_result_rect;
  result.gpu_render_dispatched = render_selector == kSmartRenderGpu && will_dispatch;
  transport::RenderTransport render_transport;
  bool transport_ready = !result.gpu_render_dispatched || !use_transport;
  bool transport_prepared = false;
  bool video_frame_gpu_ready = false;
  if (result.gpu_render_dispatched && video_frame_adapter_ready) {
    video_frame_gpu_ready =
        video_frame_worlds.prepare_gpu_output_staging(
            smart_bounds.width, smart_bounds.height) &&
        video_frame_worlds.create_input(plan.width, plan.height,
                                        *request.input_world, true,
                                        gpu_framework) &&
        video_frame_worlds.create_output(smart_bounds.width,
                                         smart_bounds.height, true,
                                         gpu_framework);
    transport_ready = video_frame_gpu_ready;
    if (video_frame_gpu_ready) {
      runtime.input_world = video_frame_worlds.input().data();
      runtime.output_world = video_frame_worlds.output().data();
      runtime.input_checkout_view_world = nullptr;
      for (auto& checkout : runtime.pixel_checkouts) {
        if (checkout.world == request.input_world->data()) {
          checkout.world = runtime.input_world;
          checkout.view_world = nullptr;
        }
      }
      write<int32_t>(video_frame_worlds.output(), 104,
                     smart_bounds.origin_x);
      write<int32_t>(video_frame_worlds.output(), 108,
                     smart_bounds.origin_y);
      if (!request.parameters->definitions.empty() &&
          request.parameters->definitions[0].size() >=
              56 + video_frame_worlds.input().size())
        std::memcpy(request.parameters->definitions[0].data() + 56,
                    video_frame_worlds.input().data(),
                    video_frame_worlds.input().size());
      transport_ready =
          request.formats->register_gpu_world(
              runtime.input_world, world_registry::kPixelFormatGpuBgra128) &&
          request.formats->register_gpu_world(
              runtime.output_world, world_registry::kPixelFormatGpuBgra128);
    }
  } else if (result.gpu_render_dispatched && use_transport) {
    transport_ready = transport::prepare_render_transport(
        request.input_world->data(), request.output_world->data(), render_transport);
    transport_prepared = transport_ready;
    // prepare_render_transport swaps each world's +24 pixel pointer to the GPU
    // device buffer (so PF_GPUDeviceSuite1::GetGPUWorldData returns it). The
    // pre-transport register_world above captured the host pointer, so re-register
    // with the device-pointer layout the plug-in actually observes during
    // dispatch; otherwise PF_GetPixelFormat's dispatch-format resolve rejects the
    // layout mismatch and the SmartRenderGPU selector fails with
    // PF_Err_OUT_OF_MEMORY (issue #305).
    if (transport_ready &&
        ((!plan.missing_input && !request.formats->register_world(
                     runtime.input_world, world_registry::kPixelFormatGpuBgra128)) ||
                !request.formats->register_world(
                     runtime.output_world, world_registry::kPixelFormatGpuBgra128))) {
      transport_ready = false;
    }
  }
  std::cerr << "stage:"
            << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_begin\n" << std::flush;
  if (result.empty_result_rect && result.pre_error == 0) {
    // A legally empty result_rect renders nothing; the selector is skipped.
    result.render_error = 0;
  } else if (will_dispatch && transport_ready) {
    if (plan.gpu_negotiation) hooks.capture_module_audit();
    runtime.gpu_render_dispatched = result.gpu_render_dispatched;
    result.selector_dispatched = true;
    if (gpu_framework == 3 && result.gpu_render_dispatched)
      preload_staged_cuda_kernel();
    result.selector_error = hooks.guarded_call(request.entry, render_selector,
        request.input->data(), request.output->data(), params.data(), nullptr,
        smart_extra.data());
    // A plug-in may use PF_CHECKOUT_PARAM from SMART_RENDER as well as from
    // SMART_PRE_RENDER. Those selector-local values are host-owned and must be
    // checked back in when the selector returns, just like the pre-render set.
    hooks.automatic_checkin();
    runtime.gpu_render_dispatched = false;
    result.render_error = result.selector_error;
  } else {
    // Invalid geometry (rects_valid false with a successful pre-render) lands
    // here too: dispatching into the stale full-frame output world would turn
    // the rejected rects into a silent render, so the run fails explicitly.
    result.render_error = result.pre_error == 0 ? -6 : -1;
  }
  if (transport_prepared) {
    // Free the device allocations and restore each world's +24 host pointer
    // whenever the transport was prepared, even if the device-pointer re-register
    // above failed and suppressed the selector, so a prepared transport never
    // leaks its allocations (#305 review).
    if (!transport::finish_render_transport(render_transport) && result.render_error == 0)
      result.render_error = -6;
    // finish_render_transport restored +24 to the host pointer; re-register the
    // host layout so a post-dispatch PF_GetPixelFormat (e.g. a plug-in that
    // queries a render world during GPU device setdown) resolves the current
    // layout instead of the now-stale device pointer (mirror of #305).
    if (!plan.missing_input)
      request.formats->register_world(request.input_world->data(),
                                      world_registry::kPixelFormatGpuBgra128);
    request.formats->register_world(request.output_world->data(),
                                    world_registry::kPixelFormatGpuBgra128);
  }
  if (video_frame_gpu_ready && result.render_error == 0) {
    bool output_copied = false;
    try {
      output_copied = video_frame_worlds.copy_output_to(*request.output_world);
    } catch (const std::exception& error) {
    } catch (...) {
    }
    if (!output_copied) result.render_error = -6;
  }
  if (video_frame_adapter_ready && result.selector_dispatched &&
      !plan.gpu_negotiation && result.render_error == 0 &&
      !video_frame_worlds.copy_output_to(*request.output_world))
    result.render_error = -6;
  std::cerr << "stage:"
            << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_end error=" << result.render_error << "\n" << std::flush;
  if (plan.gpu_negotiation && result.gpu_setup_error == 0) {
    std::array<std::byte, 16> setdown_input{};
    std::array<std::byte, 8> setdown_extra{};
    write<void*>(setdown_input, 0, read<void*>(gpu_setup_output, 0));
    write<int32_t>(setdown_input, 8, gpu_framework);
    write<uint32_t>(setdown_input, 12, plan.gpu_device_index);
    write<void*>(setdown_extra, 0, setdown_input.data());
    hooks.capture_module_audit();
    std::cerr << "stage:gpu_device_setdown_begin\n" << std::flush;
    result.gpu_setdown_error = invoke_entry_seh(request.entry, kGpuDeviceSetdown,
        request.input->data(), request.output->data(), params.data(), nullptr,
        setdown_extra.data(), &result.gpu_setdown_exception_code);
    std::cerr << "stage:gpu_device_setdown_end error=" << result.gpu_setdown_error
              << "\n" << std::flush;
  }
  if (plan.gpu_negotiation) hooks.capture_module_audit();
  if (plan.gpu_negotiation && gpu_context_started &&
      !transport::end_backend_context(gpu_framework) && result.gpu_setdown_error == 0)
    result.gpu_setdown_error = -6;
  if (video_frame_gpu_ready) {
    void* pre_render_data = read<void*>(dispatch_state.pre_output, 40);
    if (auto cleanup =
            read<void(__cdecl*)(void*)>(dispatch_state.pre_output, 48)) {
      invoke_smart_pre_render_cleanup_seh(cleanup, pre_render_data);
      write<void*>(dispatch_state.pre_output, 40, nullptr);
      write<void(__cdecl*)(void*)>(dispatch_state.pre_output, 48, nullptr);
    }
  }
  result.input_checkout_result_rect = runtime.input_checkout_result_rect;
  result.map_checkout_result_rect = runtime.map_checkout_result_rect;
  result.malformed_checkout_requests = runtime.malformed_checkout_requests;
  result.empty_checkout_pixel_denials = runtime.empty_checkout_pixel_denials;
  result.empty_layer_param_checkouts = runtime.empty_layer_param_checkouts;
  result.empty_layer_param_pixel_checkouts =
      runtime.empty_layer_param_pixel_checkouts;
  return true;
}

}  // namespace aexcompat::worker_runtime::smart_dispatch
