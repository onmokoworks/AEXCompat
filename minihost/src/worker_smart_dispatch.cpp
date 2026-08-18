#include "worker_smart_dispatch.hpp"

#include "generated/aex_abi_contract.hpp"
#include "gpu_memory_world_transport.hpp"
#include "premiere_gpu_filter_abi.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "worker_active_plugin_context.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>
#include <vector>
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

// #1072:mirror AE's PTX cubins next to the staged
// worker exe so GPUFoundation's ExecutableDir()/PTX/CUDA finds Memory.cubin.
inline void ensure_ptx_at_executable_dir() {
  std::array<wchar_t, 4096> exe{};
  if (GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size())) == 0)
    return;
  std::wstring exe_dir(exe.data());
  auto slash = exe_dir.find_last_of(L"\\/");
  if (slash == std::wstring::npos) return;
  exe_dir.resize(slash);
  if (exe_dir.rfind(L"\\\\?\\", 0) == 0) exe_dir = exe_dir.substr(4);
  std::array<wchar_t, 4096> gf{};
  const DWORD gf_len = GetModuleFileNameW(GetModuleHandleW(L"GPUFoundation.dll"),
                                          gf.data(), static_cast<DWORD>(gf.size()));
  if (gf_len == 0) return;
  std::wstring ae_dir(gf.data(), gf_len);
  slash = ae_dir.find_last_of(L"\\/");
  if (slash == std::wstring::npos) return;
  ae_dir.resize(slash);
  if (ae_dir.rfind(L"\\\\?\\", 0) == 0) ae_dir = ae_dir.substr(4);
  CreateDirectoryW((exe_dir + L"\\PTX").c_str(), nullptr);
  for (const wchar_t* fw : {L"CUDA", L"CL", L"HLSL"}) {
    const std::wstring src = ae_dir + L"\\PTX\\" + fw;
    if (GetFileAttributesW(src.c_str()) == INVALID_FILE_ATTRIBUTES) continue;
    const std::wstring dst = exe_dir + L"\\PTX\\" + fw;
    CreateDirectoryW(dst.c_str(), nullptr);
    WIN32_FIND_DATAW fd{};
    const HANDLE h = FindFirstFileW((src + L"\\*").c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) continue;
    do {
      if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
      CopyFileW((src + L"\\" + fd.cFileName).c_str(),
                (dst + L"\\" + fd.cFileName).c_str(), TRUE);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
  }
}

// #1072:remove the PTX cubins ensure_ptx mirrored
// into the staged worker's ExecutableDir once GPUFoundation has preloaded every
// kernel into device memory (kernel_load_action=2). The staged exe dir is a
// trusted-worker stage slot root; leaving an unexpected PTX/ subtree there makes
// the broker skip that slot forever (trusted_worker_stage.rs stage_tree_has_
// unexpected_entry), which exhausts all slots after a handful of GPU renders.
// Deleting it right after the preload keeps the slot clean for reuse. Only the
// worker's own mirror is touched (its own ExecutableDir), so there is no foreign
// path here.
inline void remove_ptx_at_executable_dir() {
  std::array<wchar_t, 4096> exe{};
  if (GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size())) == 0)
    return;
  std::wstring exe_dir(exe.data());
  auto slash = exe_dir.find_last_of(L"\\/");
  if (slash == std::wstring::npos) return;
  exe_dir.resize(slash);
  if (exe_dir.rfind(L"\\\\?\\", 0) == 0) exe_dir = exe_dir.substr(4);
  const std::wstring ptx = exe_dir + L"\\PTX";
  for (const wchar_t* fw : {L"CUDA", L"CL", L"HLSL"}) {
    const std::wstring dir = ptx + L"\\" + fw;
    WIN32_FIND_DATAW fd{};
    const HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h != INVALID_HANDLE_VALUE) {
      do {
        if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) continue;
        DeleteFileW((dir + L"\\" + fd.cFileName).c_str());
      } while (FindNextFileW(h, &fd));
      FindClose(h);
    }
    RemoveDirectoryW(dir.c_str());
  }
  RemoveDirectoryW(ptx.c_str());
}

// #1072:query a GPU VideoFrame's typed memory-access
// interfaces (Contrast FUN_1800078c0/FUN_180007980). `mapping` is the object at
// InterfaceRef slot [8] of GetGPUIVFFromPFEffectWorld's 24-byte return, NOT the
// IVideoFrame at [0]. vtable[1] is the query-by-type-name entry.
// This dereferences a vtable on `mapping` whose layout was recovered from the
// RE of the color family; a GPU frame whose mapping object has a different shape
// than the one observed there could fault. That is crash-contained by the worker
// process floor (separate process + Job Object + minidump), not turned into a
// structured diagnostic here — the transport is only reached for effects that
// already advertise GPU F32 render and returned 14 on the CPU path.
inline bool gpu_frame_device_memory(void* mapping, void** out_ptr, int32_t* out_rowbytes) {
  if (!mapping) return false;
  struct TypedQuery { const char* name; std::uint64_t len; };
  auto* obj = static_cast<void***>(mapping);
  using QueryFn = void* (*)(void*, const TypedQuery*);
  const auto query = [&](const char* name, std::uint64_t len) -> void* {
    TypedQuery q{name, len};
    return reinterpret_cast<QueryFn>((*obj)[1])(mapping, &q);
  };
  void* gpu_access = query("VF::IGPUVideoFrameMemoryAccess>(void)", 30);
  void* pixel_access = query("VF::IVideoFrame2DPixelMemoryAccess>(void)", 34);
  if (!gpu_access || !pixel_access) return false;
  auto* g = static_cast<void***>(gpu_access);
  auto* p = static_cast<void***>(pixel_access);
  *out_ptr = reinterpret_cast<void* (*)(void*)>((*g)[0])(gpu_access);
  *out_rowbytes = reinterpret_cast<int32_t (*)(void*)>((*p)[1])(pixel_access);
  return *out_ptr != nullptr && *out_rowbytes > 0;
}

// Adobe's own effects that depend on VideoFrame.dll expect a CPU SmartFX world
// to retain the private PPix backing created by that DLL.  The public
// PF_EffectWorld layout alone cannot manufacture that backing.  Keep this an
// optional dependency adapter: use only already-loaded Adobe modules, call
// their world lifecycle exports, and expose no guessed PPix layout here.
class VideoFrameCpuWorlds {
 public:
  // VideoFrame owns these LayerDefs (VF::NewPF_WorldWithNewVideoFrame /
  // CreateGPUVideoFrame fill and free them); they are not the host's
  // EffectWorldStorage and carry no PF_World facade prefix. The host worlds
  // this adapter copies to/from are EffectWorldStorage (HostWorld).
  using World = std::array<std::byte, 120>;
  using HostWorld = aexcompat::world_safety::EffectWorldStorage;

  bool available() {
    // Cache the full resolution result, not a re-derived subset: an earlier
    // version recomputed a 15-of-30 proc AND on the resolved_ fast path, so if a
    // proc that only the slow path checks failed to resolve, the first call
    // returned false while every later call returned true. That split masked the
    // two AE-2026 export renames this adapter needs (format_rowbytes and
    // GF::Initialize) and made video_frame_worlds look ready when it was not.
    if (resolved_) return available_;
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
        "?UncompressedPixelFormatRowBytes@dvamediatypes@@YA_KAEBUPixelFormat@1@H@Z"));
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
            "?Initialize@GF@@YAX_N0000W4KernelLoadAction@1@@Z"));
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
    available_ =
        create_ && dispose_ && par_ctor_ && format_rowbytes_ && get_mapping_ && new_ppix_ &&
        get_frame_ && init_gpu_ && dispose_gpu_ && initialize_asl_foundation_ &&
        initialize_video_frame_ && shutdown_video_frame_ &&
        terminate_asl_foundation_ && initialize_gpu_foundation_ &&
        terminate_gpu_foundation_ && is_gpu_foundation_initialized_ &&
        get_primary_device_ && get_device_count_ && get_device_ &&
        create_gpu_frame_ && ensure_gpu_transfer_ && create_gpu_frame_from_handle_ &&
        get_gpu_ppix_ && get_gpu_frame_ && convert_to_gpu_ &&
        convert_to_cpu_ && new_ppix_from_frame_ && ppix_lock_ && ppix_unlock_ && ppix_pixels_ &&
        ppix_rowbytes_ && ppix_copy_ && dispose_ppix_;
    return available_;
  }

  bool create_input(int32_t width, int32_t height, const HostWorld& source,
    bool gpu, int32_t framework) {
    if (gpu) {
      input_gpu_ = create_gpu(input_, input_live_, width, height, framework);
      if (!input_gpu_) return false;
      // #1072:upload float32 input straight into the
      // GPU frame's CUDA device memory (ppixCopy does not cross CPU/GPU).
      return gpu_memcpy_frame(input_, world_pixels(source), world_i32(source, 32),
                              width, height, /*to_gpu=*/true);
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

  World& input() { return input_; }
  World& output() { return output_; }

  bool initialize_gpu_host() {
    return initialize_gpu_foundation_at_module_root();
  }

  void* cuda_context() const { return gf_cuda_context_; }

  // --- Premiere GPU-filter host primitives (issue #1058) ---------------------
  // A standalone GPU PPix factory the Premiere GPU-filter host drives on demand
  // (the fixed input()/output() slots above are for the PF SmartFX GPU path).
  // The PPix handle lives at world+64: a VF-backed PPix that
  // VF::GetPPixHandFrameMapping maps, which is exactly what the plug-in reads.
  bool pr_gpu_ready() {
    if (!(available() && initialize_gpu_foundation_at_module_root() &&
          (owns_video_frame_ || (owns_video_frame_ = initialize_video_frame_()))))
      return false;
    // A Premiere GPU filter fetches its device itself via GF::Detail::GetDevice
    // (unlike the PF SmartFX GPU route, which is handed a device through the PF
    // GPU suite). GetDevice indexes GF's enumerated device registry, which is
    // only populated once the device list is walked - GetPrimaryDevice alone
    // does not fill it. Warm the registry so the plug-in's own GetDevice call
    // returns a live device instead of an empty shared_ptr (issue #1058).
    const uint32_t device_count = get_device_count_();
    for (uint32_t index = 0; index < device_count; ++index) {
      std::shared_ptr<void> device;
      get_device_(&device, index);
    }
    return device_count > 0;
  }
  bool pr_make_gpu_ppix(World& world, bool& live, int32_t width, int32_t height) {
    return create_gpu(world, live, width, height, /*framework=*/3);
  }
  bool pr_upload(World& world, const void* cpu, int32_t cpu_rowbytes,
                 int32_t width, int32_t height) {
    return gpu_memcpy_frame(world, const_cast<void*>(cpu), cpu_rowbytes, width,
                            height, /*to_gpu=*/true);
  }
  bool pr_download(World& world, void* cpu, int32_t cpu_rowbytes, int32_t width,
                   int32_t height) {
    return gpu_memcpy_frame(world, cpu, cpu_rowbytes, width, height,
                            /*to_gpu=*/false);
  }
  void pr_dispose(World& world, bool& live) {
    if (live) {
      dispose_gpu_(world.data());
      live = false;
    }
  }
  void* pr_ppix(const World& world) const {
    void* ppix{};
    std::memcpy(&ppix, world.data() + 64, sizeof(ppix));
    return ppix;
  }
  // The frame's real dimensions as stored in its VF GPU world. create_gpu only
  // returns a live world whose width/height fields (offsets 36/40) equal the
  // requested size, so these are authoritative - unlike the FrameRecord scalar
  // members, which the world build corrupts (issue #1058).
  void pr_world_dims(const World& world, int32_t& width, int32_t& height) const {
    width = world_i32(world, 36);
    height = world_i32(world, 40);
  }
  // Raw CUDA device pointer backing a GPU frame, via the same VF frame mapping
  // the plug-in itself reads (borrowed, not released - mirrors gpu_memcpy_frame).
  void* pr_gpu_device_ptr(World& world) {
    std::array<std::byte, 24> ivf{};
    get_gpu_frame_(ivf.data(), world.data());
    void* mapping{};
    std::memcpy(&mapping, ivf.data() + 8, sizeof(mapping));
    void* dev_ptr{};
    int32_t gpu_rowbytes{};
    return gpu_frame_device_memory(mapping, &dev_ptr, &gpu_rowbytes) ? dev_ptr
                                                                      : nullptr;
  }

  bool copy_output_to(HostWorld& destination) {
    if (!output_live_) return false;
    if (!output_gpu_) return copy_pixels(output_, destination);
    // #1072:transfer_gpu_to_cpu does the CUDA
    // context push and the device->host copy itself.
    return transfer_gpu_to_cpu(output_, destination);
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
  }

 private:
  using CreateWorld = bool(__cdecl*)(uint32_t, uint32_t, uint64_t,
                                      const void*, void*);
  using DisposeWorld = void(__cdecl*)(void*);
  using ParConstructor = void*(__cdecl*)(void*, uint32_t, uint32_t);
  // AE 2026 passes the PixelFormat by const reference (a pointer), not by value.
  using FormatRowbytes = uint64_t(__cdecl*)(const void*, int32_t);
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
      bool, bool, bool, bool, bool, int32_t);
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

  template <typename AnyWorld>
  static int32_t world_i32(const AnyWorld& world, std::size_t offset) {
    int32_t value{};
    std::memcpy(&value, world.data() + offset, sizeof(value));
    return value;
  }

  template <typename AnyWorld>
  static void* world_pixels(const AnyWorld& world) {
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

  template <typename SourceWorld, typename DestinationWorld>
  static bool copy_pixels(const SourceWorld& source, DestinationWorld& destination) {
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
    ensure_ptx_at_executable_dir();  // #1072
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
                               /*kernel_load_action=*/2);  // #1072
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
    // Kernels are resident in device memory now (full preload above), so the
    // mirrored cubins on disk are no longer needed. Remove them so this worker's
    // stage slot is not left poisoned for the next effect (#1072). The "preload
    // covers every kernel the render dispatches" assumption is validated
    // empirically: the color family renders correctly with the mirror already
    // deleted here (full corpus sweep, #1072). A new gate-matched effect that
    // lazy-faults a kernel not covered by the preload would fail its GPU render;
    // re-verify the sweep when widening the gate, or defer removal to worker exit.
    remove_ptx_at_executable_dir();
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

  // #1072:copy float32 ARGB rows between a GPU
  // frame's CUDA device memory and a CPU buffer inside gf_cuda_context_.
  bool gpu_memcpy_frame(const World& gpu_world, void* cpu_buf, int32_t cpu_rowbytes,
                        int32_t width, int32_t height, bool to_gpu) {
    if (!cpu_buf || width <= 0 || height <= 0) return false;
    const size_t row = static_cast<size_t>(width) * 16;  // float32 ARGB
    // Fail closed rather than over-read/over-write a malformed world (mirrors
    // copy_pixels' rowbytes check): the per-row payload is `row` bytes on both
    // sides, so a stride shorter than that is a rejected world, not a clamp. The
    // negative check matters because cpu_rowbytes is signed and the offset math
    // below is unsigned: a negative stride cast to size_t would pass a bare
    // `< row` test and then wrap the pointer.
    if (cpu_rowbytes < 0 || static_cast<size_t>(cpu_rowbytes) < row) return false;
    std::array<std::byte, 24> ivf{};
    get_gpu_frame_(ivf.data(), gpu_world.data());
    // KNOWN LEAK (follow-up): GetGPUIVFFromPFEffectWorld returns an owned
    // InterfaceRef<IVideoFrame> (RE of VF::GetGPUIVFFromPFEffectWorld @18001a0c0:
    // it stores AddRef'd pointers into the 24-byte return and the caller's
    // InterfaceRef destructor is what releases them), and the typed query in
    // gpu_frame_device_memory returns AddRef'd sub-interfaces too. This borrows
    // both without releasing, matching the pre-existing get_mapping_ convention.
    // Per-frame (2x: input upload + output copy) it does not accumulate for the
    // one-frame-per-worker render paths validated here, but a resident multi-frame
    // GPU session would leak. A correct fix releases the frame ref AND the sub-
    // interfaces (release_interface_ref handles the 24-byte ref; the sub-interface
    // release ABI needs its own RE) and must be verified with a multi-frame leak
    // test before landing, since a wrong release double-frees.
    void* mapping{};
    std::memcpy(&mapping, ivf.data() + 8, sizeof(mapping));  // slot [8]
    void* dev_ptr{};
    int32_t gpu_rowbytes{};
    if (!gpu_frame_device_memory(mapping, &dev_ptr, &gpu_rowbytes) ||
        static_cast<size_t>(gpu_rowbytes) < row)
      return false;
    const HMODULE cuda = GetModuleHandleW(L"nvcuda.dll");
    if (!cuda) return false;
    using CuMemcpyHtoD = int(__stdcall*)(unsigned long long, const void*, size_t);
    using CuMemcpyDtoH = int(__stdcall*)(void*, unsigned long long, size_t);
    using CuCtxPushCurrent = int(__stdcall*)(void*);
    using CuCtxPopCurrent = int(__stdcall*)(void**);
    using CuCtxSynchronize = int(__stdcall*)();
    const auto h2d = reinterpret_cast<CuMemcpyHtoD>(GetProcAddress(cuda, "cuMemcpyHtoD_v2"));
    const auto d2h = reinterpret_cast<CuMemcpyDtoH>(GetProcAddress(cuda, "cuMemcpyDtoH_v2"));
    const auto push = reinterpret_cast<CuCtxPushCurrent>(GetProcAddress(cuda, "cuCtxPushCurrent_v2"));
    const auto pop = reinterpret_cast<CuCtxPopCurrent>(GetProcAddress(cuda, "cuCtxPopCurrent_v2"));
    const auto sync = reinterpret_cast<CuCtxSynchronize>(GetProcAddress(cuda, "cuCtxSynchronize"));
    if (!h2d || !d2h || !push || !pop) return false;
    // dev_ptr is only valid inside gf_cuda_context_; refuse to issue the copy
    // against whatever context happens to be current if the push failed, rather
    // than reading/writing the wrong device memory.
    if (!gf_cuda_context_ || push(gf_cuda_context_) != 0) return false;
    // Wait for any pending device work before touching the frame's memory. A
    // Premiere GPU filter launches its CUDA kernel on its own stream, and a
    // plain cuMemcpyDtoH only orders against the default stream, so a download
    // issued right after the plug-in's Render returned would otherwise read the
    // frame before the kernel finished and copy back zeros (issue #1058).
    if (sync) sync();
    bool ok = true;
    for (int32_t y = 0; y < height && ok; ++y) {
      const unsigned long long dev = reinterpret_cast<unsigned long long>(dev_ptr) +
          static_cast<unsigned long long>(y) * gpu_rowbytes;
      auto* cpu = static_cast<std::byte*>(cpu_buf) + static_cast<size_t>(y) * cpu_rowbytes;
      ok = to_gpu ? (h2d(dev, cpu, row) == 0) : (d2h(cpu, dev, row) == 0);
    }
    if (ok && sync) sync();
    // The context we pushed must be the one that comes back off the stack; a
    // mismatch means the stack was corrupted under us, so fail closed.
    void* popped{};
    const bool popped_ok = pop(&popped) == 0 && popped == gf_cuda_context_;
    return ok && popped_ok;
  }

  template <typename DestinationWorld>
  bool transfer_gpu_to_cpu(const World& gpu, DestinationWorld& destination) {
    return gpu_memcpy_frame(gpu, world_pixels(destination), world_i32(destination, 32),
                            world_i32(destination, 36), world_i32(destination, 40),
                            /*to_gpu=*/false);
  }

  bool resolved_{};
  bool available_{};
  bool input_live_{};
  bool output_live_{};
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

// The same accessors on a world storage (the LayerDef part).
template <typename T>
T read(const aexcompat::world_safety::EffectWorldStorage& world, std::size_t offset) {
  return read<T>(world.layer_def, offset);
}
template <typename T>
void write(aexcompat::world_safety::EffectWorldStorage& world, std::size_t offset, T value) {
  write(world.layer_def, offset, value);
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

// ===========================================================================
// Premiere GPU-filter host (issue #1058)
//
// The VR / Immersive effect family (VRGaussianBlur, ...) are GPU-only Premiere
// GPU filters. Their AE SmartFX CPU path only draws a "requires GPU" warning
// and returns 512, so the only way to render them is to drive their private
// `xGPUFilterEntry` export exactly as Premiere / AE's bridge does: hand the
// plug-in a `piSuites` host plus the Premiere GPU suites, then call
// CreateInstance / Render / DisposeInstance. Input/output frames are the GPU
// VideoFrame PPix handles VideoFrameCpuWorlds already builds for #1158; the
// plug-in reads their device memory through VF::GetPPixHandFrameMapping, the
// same path the PF SmartFX GPU route uses.
// ===========================================================================
namespace pr_host {
namespace abi = ::aexcompat::worker_runtime::pr_gpu;
namespace params_rt = ::aexcompat::worker_runtime::parameters;
using World = VideoFrameCpuWorlds::World;

struct FrameRecord {
  World world{};
  bool live{};
  int32_t width{};
  int32_t height{};
  abi::PrPixelFormat format{abi::kPrPixelFormat_GPU_BGRA_4444_32f};
  // True for a frame the plug-in allocated through CreateGPUPPix. A Premiere GPU
  // filter renders into a frame it creates itself and hands back through
  // outFrame; it does not touch a host-provided destination. That returned frame
  // is what the host must read (issue #1058).
  bool plugin_created{};
  // Set when the plug-in released the frame through PPixSuite::Dispose. The
  // actual free is deferred to the HostContext destructor (the released frame
  // may still be the render output the host has not read yet), but recording the
  // release keeps handle ownership fail-closed: a second Dispose of the same
  // handle, or a Dispose of a handle this host never issued, is rejected.
  bool dispose_requested{};
};

struct HostContext {
  VideoFrameCpuWorlds* frames{};
  std::vector<std::unique_ptr<FrameRecord>> gpu_frames;
  const params_rt::State* param_state{};
  int32_t node_id{};
  int32_t width{};
  int32_t height{};
  void* cuda_context{};
  std::vector<char*> scratch;

  ~HostContext() {
    // Every GPU frame the render allocated - host input/output and each frame
    // the plug-in created through CreateGPUPPix - is freed exactly once here,
    // since PPixSuite::Dispose only defers. A plug-in that allocates frames in a
    // loop therefore holds them all until the render ends rather than freeing a
    // disposed frame immediately; peak device memory is bounded by the worker's
    // kill-on-close Job Object memory limit, which contains a runaway allocator
    // (issue #1058 follow-up if a legitimate effect ever needs a per-render cap).
    for (auto& record : gpu_frames)
      if (record && record->live) frames->pr_dispose(record->world, record->live);
    for (char* pointer : scratch) std::free(pointer);
  }
};

// Suite methods are stateless and reach the active render through this pointer,
// set for the duration of one run_pr_gpu_filter() call on the render thread.
thread_local HostContext* g_ctx{};

FrameRecord* find_frame(abi::PPixHand ppix) {
  if (!g_ctx || !ppix) return nullptr;
  for (auto& record : g_ctx->gpu_frames)
    if (record->live &&
        g_ctx->frames->pr_ppix(record->world) == static_cast<void*>(ppix))
      return record.get();
  return nullptr;
}

// ---- GPU Device Suite -----------------------------------------------------
abi::prSuiteError GPUDev_GetDeviceCount(abi::csSDK_uint32* out) {
  if (out) *out = 1;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError GPUDev_GetDeviceInfo(abi::csSDK_uint32, abi::csSDK_uint32,
                                       abi::PrGPUDeviceInfo* out) {
  if (!out) return -1;
  std::memset(out, 0, sizeof(*out));
  out->outDeviceFramework = 0;  // CUDA
  out->outMeetsMinReq = 1;
  out->outContextHandle = g_ctx ? g_ctx->cuda_context : nullptr;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError GPUDev_CreateGPUPPix(abi::csSDK_uint32, abi::PrPixelFormat fmt,
                                       int32_t w, int32_t h, int32_t, int32_t,
                                       abi::prFieldType, abi::PPixHand* out) {
  if (!out || !g_ctx || w <= 0 || h <= 0) return -1;
  auto record = std::make_unique<FrameRecord>();
  record->width = w;
  record->height = h;
  record->format = fmt;
  if (!g_ctx->frames->pr_make_gpu_ppix(record->world, record->live, w, h))
    return -1;
  // Note: building the VF GPU world leaves record->height at h+1 (the world
  // build writes an allocated-height value onto this adjacent member; the
  // mechanism is an InitEffectWorldGPU ABI quirk tracked separately). The
  // authoritative requested size lives in the world at offsets 36/40, which
  // pr_world_dims reads; the output readback uses those, not this member. The
  // suite queries below (GetGPUPPixSize / PPix_GetBounds) still report the
  // member, which the in-corpus plug-ins render correctly against.
  record->plugin_created = true;
  *out = reinterpret_cast<abi::PPixHand>(g_ctx->frames->pr_ppix(record->world));
  g_ctx->gpu_frames.push_back(std::move(record));
  return abi::kSuiteError_NoError;
}
abi::prSuiteError GPUDev_GetGPUPPixData(abi::PPixHand ppix, void** out) {
  FrameRecord* record = find_frame(ppix);
  if (!record || !out) return -1;
  *out = g_ctx->frames->pr_gpu_device_ptr(record->world);
  return *out ? abi::kSuiteError_NoError : -1;
}
abi::prSuiteError GPUDev_GetGPUPPixDeviceIndex(abi::PPixHand,
                                               abi::csSDK_uint32* out) {
  if (out) *out = 0;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError GPUDev_GetGPUPPixSize(abi::PPixHand ppix, size_t* out) {
  FrameRecord* record = find_frame(ppix);
  if (!record || !out) return -1;
  *out = static_cast<size_t>(record->width) * record->height * 16;
  return abi::kSuiteError_NoError;
}

// ---- PPix Suite / PPix2 Suite ---------------------------------------------
abi::prSuiteError PPix_Dispose(abi::PPixHand ppix) {
  // A Premiere GPU filter calls Dispose on the frame it returns through outFrame
  // as a reference release, expecting the host to still hold its own reference.
  // This host does not reference-count frames, so freeing here would destroy the
  // rendered output before it is read back; the free is deferred to the
  // HostContext destructor (issue #1058). Ownership stays fail-closed though: a
  // Dispose of a handle this host never issued, or a second Dispose of the same
  // handle, is rejected rather than silently accepted.
  FrameRecord* record = find_frame(ppix);
  if (!record || record->dispose_requested) return -1;
  record->dispose_requested = true;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix_GetBounds(abi::PPixHand ppix, abi::prRect* out) {
  FrameRecord* record = find_frame(ppix);
  if (!record || !out) return -1;
  *out = {0, 0, record->width, record->height};
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix_GetRowBytes(abi::PPixHand ppix, abi::csSDK_int32* out) {
  FrameRecord* record = find_frame(ppix);
  if (!record || !out) return -1;
  *out = record->width * 16;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix_GetPixelAspectRatio(abi::PPixHand, abi::csSDK_uint32* num,
                                           abi::csSDK_uint32* den) {
  if (num) *num = 1;
  if (den) *den = 1;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix_GetPixelFormat(abi::PPixHand ppix,
                                      abi::PrPixelFormat* out) {
  FrameRecord* record = find_frame(ppix);
  if (!record || !out) return -1;
  *out = record->format;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix2_GetSize(abi::PPixHand ppix, size_t* out) {
  return GPUDev_GetGPUPPixSize(ppix, out);
}
abi::prSuiteError PPix2_GetOrigin(abi::PPixHand, abi::csSDK_int32* x,
                                  abi::csSDK_int32* y) {
  if (x) *x = 0;
  if (y) *y = 0;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError PPix2_GetFieldOrder(abi::PPixHand, abi::prFieldType* out) {
  if (out) *out = 0;  // progressive
  return abi::kSuiteError_NoError;
}

// ---- Video Segment Suite: GetParam bridges to the worker's param records ---
abi::prSuiteError VS_GetNodeProperty(abi::csSDK_int32, const char*,
                                     abi::PrMemoryPtr* out) {
  // Failing this is intentional: the plug-in falls back to reading its blur
  // amount through GetParam (observed in VRGaussianBlur::Render) when the
  // "EffectNode::RuntimeInstanceID" property lookup fails, which is the value
  // path this host actually serves.
  if (out) *out = nullptr;
  return -1;
}
abi::prSuiteError VS_GetParamCount(abi::csSDK_int32, abi::csSDK_int32* out) {
  if (!out || !g_ctx || !g_ctx->param_state) return -1;
  *out = static_cast<abi::csSDK_int32>(g_ctx->param_state->records.size());
  return abi::kSuiteError_NoError;
}
abi::prSuiteError VS_GetParam(abi::csSDK_int32, abi::csSDK_int32 index,
                              abi::PrTime, abi::PrParam* out) {
  if (!out || !g_ctx || !g_ctx->param_state) return -1;
  std::memset(out, 0, sizeof(*out));
  const auto& records = g_ctx->param_state->records;
  // Suite param index i addresses the effect's i-th real parameter (the input
  // layer is not a node param), i.e. worker records[i].
  if (index < 0 || static_cast<std::size_t>(index) >= records.size()) return -1;
  const auto& record = records[static_cast<std::size_t>(index)];
  const double value = record.has_current ? record.current_value
                                          : record.default_value;
  switch (record.type) {
    case 4:  // PF_Param_CHECKBOX
      out->mType = abi::kPrParamType_Bool;
      out->mBool = value != 0.0 ? 1 : 0;
      return abi::kSuiteError_NoError;
    case 7: {  // PF_Param_POPUP
      // PF stores popup selections 1-based; the Premiere VideoSegment convention
      // these VR effects read against is 0-based. Delivering the raw PF value
      // shifts every menu by one - for VRColorGradient that lands the default
      // "Blending Mode" (PF 2 = "Normal") on the plug-in's 0-based index 2,
      // which is the "(-" separator entry in its BlendingModeList, disabling the
      // blend and reading back as an exact input passthrough (issue #1206).
      // Convert to 0-based; a valid PF popup value is >= 1.
      out->mType = abi::kPrParamType_Int32;
      const int32_t pf_value = static_cast<int32_t>(value);
      out->mInt32 = pf_value > 0 ? pf_value - 1 : 0;
      return abi::kSuiteError_NoError;
    }
    case 5: {  // PF_Param_COLOR
      // The VR effects read the color out of the PrParam value union as three
      // 16-bit little-endian channels R,G,B (bytes +1/+3/+5 from the union
      // start), each divided by 255 - i.e. they take the high byte of each
      // 16-bit channel as the 8-bit value (observed in VRColorGradient::Render
      // FUN_180007f10: R=uStack_330>>8&0xff, G=uStack_330>>0x18, B=uStack_32c>>8
      // &0xff). Deliver the discovered 8-bit color in each channel's high byte.
      out->mType = abi::kPrParamType_Int64;
      const auto& color =
          record.has_current ? record.current_color : record.default_color;
      std::array<uint8_t, 8> packed{};
      packed[1] = color[1];  // R high byte (record color is ARGB: [1]=R)
      packed[3] = color[2];  // G high byte
      packed[5] = color[3];  // B high byte
      std::memcpy(&out->mInt64, packed.data(), packed.size());
      return abi::kSuiteError_NoError;
    }
    case 6:   // PF_Param_POINT
    case 18: {  // PF_Param_POINT_3D
      // Discovery stores a POINT default as a percentage of the layer (SDK
      // PF_PointDef x_dephault, 0..100). The Premiere VideoSegment convention
      // these VR effects read against is a 0..1 fraction of the frame with the
      // frame center at 0.5 (VRColorGradient's point->direction math uses a
      // hard-coded center of {0.5, 0.5}: FUN_1800103e0). Delivering the raw
      // percentage placed every point ~100x outside the frame, collapsing the
      // per-point view directions and corrupting the gradient. Convert to the
      // 0..1 fraction.
      out->mType = abi::kPrParamType_Point;
      const auto& components =
          record.has_current ? record.current_components : record.default_components;
      out->mPoint.x = record.component_count > 0 ? components[0] / 100.0 : 0.0;
      out->mPoint.y = record.component_count > 1 ? components[1] / 100.0 : 0.0;
      return abi::kSuiteError_NoError;
    }
    case 1:   // PF_Param_SLIDER
    case 2:   // PF_Param_FIX_SLIDER
    case 3:   // PF_Param_ANGLE (worker stores the decoded value)
    case 10:  // PF_Param_FLOAT_SLIDER
    default:
      out->mType = abi::kPrParamType_Float64;
      out->mFloat64 = value;
      return abi::kSuiteError_NoError;
  }
}

// Video Segment node-graph walk (PrGPUFilterBase_VR::GetMediaDimensions). This
// host has no Premiere node graph, so the owner-node query returns "no node":
// the plug-in's walk then terminates on its first iteration and falls back to
// SequenceInfoSuite::GetFrameRect for the frame dimensions (observed in
// VRGaussianBlur::Initialize).
abi::prSuiteError VS_AcquireOperatorOwnerNodeID(abi::csSDK_int32,
                                                abi::csSDK_int32* out) {
  if (out) *out = 0;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError VS_AcquireInputNodeID(abi::csSDK_int32, abi::csSDK_int32,
                                        abi::PrTime*, abi::csSDK_int32* out) {
  if (out) *out = 0;
  return abi::kSuiteError_NoError;
}
abi::prSuiteError VS_ReleaseVideoNodeID(abi::csSDK_int32) {
  return abi::kSuiteError_NoError;
}
abi::prSuiteError VS_GetNodeInfo(abi::csSDK_int32, char* outType,
                                 void* /*outHash*/, abi::csSDK_int32* outFlags) {
  if (outType) outType[0] = '\0';
  if (outFlags) *outFlags = 0;
  return abi::kSuiteError_NoError;
}

// ---- Memory Manager Suite (only the pointer helpers are ever driven) -------
abi::PrMemoryPtr MM_NewPtrClear(abi::csSDK_uint32 bytes) {
  void* pointer = std::calloc(bytes ? bytes : 1, 1);
  if (pointer && g_ctx) g_ctx->scratch.push_back(static_cast<char*>(pointer));
  return static_cast<abi::PrMemoryPtr>(pointer);
}
abi::PrMemoryPtr MM_NewPtr(abi::csSDK_uint32 bytes) {
  return MM_NewPtrClear(bytes);
}
void MM_PrDisposePtr(abi::PrMemoryPtr pointer) {
  if (!pointer || !g_ctx) return;
  auto& scratch = g_ctx->scratch;
  for (auto it = scratch.begin(); it != scratch.end(); ++it)
    if (*it == pointer) {
      std::free(pointer);
      scratch.erase(it);
      return;
    }
}

// ---- Sequence Info Suite ---------------------------------------------------
abi::prSuiteError SI_GetFrameRect(abi::PrTimelineID, abi::prRect* out) {
  if (!out || !g_ctx) return -1;
  *out = {0, 0, g_ctx->width, g_ctx->height};
  return abi::kSuiteError_NoError;
}
// Report a flat (non-VR) projection so a VR effect renders as an ordinary 2D
// layer rather than warping to an equirectangular sphere.
abi::prSuiteError SI_GetImmersiveVideoVRConfiguration(abi::PrTimelineID,
    int32_t* proj, int32_t* layout, abi::csSDK_uint32* h, abi::csSDK_uint32* v) {
  if (proj) *proj = 0;
  if (layout) *layout = 0;
  if (h) *h = 360;
  if (v) *v = 180;
  return abi::kSuiteError_NoError;
}

// Static suite tables. Any slot a plug-in reaches that this host does not
// serve stays null: that surfaces as a diagnosable crash, never a silent wrong
// pixel (the fail-closed stance in CLAUDE.md).
abi::PrSDKGPUDeviceSuite g_gpu_device_suite{};
abi::PrSDKPPixSuite g_ppix_suite{};
abi::PrSDKPPix2Suite g_ppix2_suite{};
abi::PrSDKVideoSegmentSuite g_video_segment_suite{};
abi::PrSDKMemoryManagerSuite g_memory_suite{};
abi::PrSDKSequenceInfoSuite g_sequence_info_suite{};
// Acquired-but-unused-by-the-effects-in-scope tables (null slots).
struct EmptySuite { void* slots[32]{}; };
EmptySuite g_gpu_image_processing_suite{};
EmptySuite g_transition_suite{};
EmptySuite g_opaque_effect_data_suite{};

bool g_suite_tables_ready{};
void ensure_suite_tables() {
  if (g_suite_tables_ready) return;
  g_gpu_device_suite.GetDeviceCount = &GPUDev_GetDeviceCount;
  g_gpu_device_suite.GetDeviceInfo = &GPUDev_GetDeviceInfo;
  g_gpu_device_suite.CreateGPUPPix = &GPUDev_CreateGPUPPix;
  g_gpu_device_suite.GetGPUPPixData = &GPUDev_GetGPUPPixData;
  g_gpu_device_suite.GetGPUPPixDeviceIndex = &GPUDev_GetGPUPPixDeviceIndex;
  g_gpu_device_suite.GetGPUPPixSize = &GPUDev_GetGPUPPixSize;

  g_ppix_suite.Dispose = &PPix_Dispose;
  g_ppix_suite.GetBounds = &PPix_GetBounds;
  g_ppix_suite.GetRowBytes = &PPix_GetRowBytes;
  g_ppix_suite.GetPixelAspectRatio = &PPix_GetPixelAspectRatio;
  g_ppix_suite.GetPixelFormat = &PPix_GetPixelFormat;

  g_ppix2_suite.GetSize = &PPix2_GetSize;
  g_ppix2_suite.GetOrigin = &PPix2_GetOrigin;
  g_ppix2_suite.GetFieldOrder = &PPix2_GetFieldOrder;

  g_video_segment_suite.slot8_ReleaseVideoNodeID =
      reinterpret_cast<void*>(&VS_ReleaseVideoNodeID);
  g_video_segment_suite.slot9_GetNodeInfo =
      reinterpret_cast<void*>(&VS_GetNodeInfo);
  g_video_segment_suite.slot11_AcquireInputNodeID =
      reinterpret_cast<void*>(&VS_AcquireInputNodeID);
  g_video_segment_suite.GetNodeProperty = &VS_GetNodeProperty;
  g_video_segment_suite.GetParamCount = &VS_GetParamCount;
  g_video_segment_suite.GetParam = &VS_GetParam;
  // idx 26 (+0xd0) AcquireOperatorOwnerNodeID: tail[8] past GetParam (idx 17).
  g_video_segment_suite.tail[8] =
      reinterpret_cast<void*>(&VS_AcquireOperatorOwnerNodeID);

  g_sequence_info_suite.slot0_GetFrameRect =
      reinterpret_cast<void*>(&SI_GetFrameRect);

  g_memory_suite.NewPtrClear = &MM_NewPtrClear;
  g_memory_suite.NewPtr = &MM_NewPtr;
  g_memory_suite.PrDisposePtr = &MM_PrDisposePtr;

  g_sequence_info_suite.GetImmersiveVideoVRConfiguration =
      &SI_GetImmersiveVideoVRConfiguration;
  g_suite_tables_ready = true;
}

bool name_is(const char* a, const char* b) {
  return a && b && std::strcmp(a, b) == 0;
}
abi::prSuiteError SP_AcquireSuite(const char* name, int32_t, const void** out) {
  if (!out) return -1;
  *out = nullptr;
  if (name_is(name, abi::kGPUDeviceSuite)) *out = &g_gpu_device_suite;
  else if (name_is(name, abi::kPPixSuite)) *out = &g_ppix_suite;
  else if (name_is(name, abi::kPPix2Suite)) *out = &g_ppix2_suite;
  else if (name_is(name, abi::kVideoSegmentSuite)) *out = &g_video_segment_suite;
  else if (name_is(name, abi::kMemoryManagerSuite)) *out = &g_memory_suite;
  else if (name_is(name, abi::kSequenceInfoSuite)) *out = &g_sequence_info_suite;
  else if (name_is(name, abi::kGPUImageProcessingSuite))
    *out = &g_gpu_image_processing_suite;
  else if (name_is(name, abi::kTransitionSuite)) *out = &g_transition_suite;
  else if (name_is(name, abi::kOpaqueEffectDataSuite))
    *out = &g_opaque_effect_data_suite;
  return *out ? abi::kSuiteError_NoError : -1;
}
abi::prSuiteError SP_ReleaseSuite(const char*, int32_t) {
  return abi::kSuiteError_NoError;
}
abi::SPBasicSuite g_sp_basic{&SP_AcquireSuite, &SP_ReleaseSuite, nullptr,
                             nullptr, nullptr, nullptr, nullptr};
abi::SPBasicSuite* get_sp_basic() { return &g_sp_basic; }
abi::PlugUtilFuncs g_util_funcs{nullptr, nullptr, nullptr,       nullptr,
                                nullptr, nullptr, nullptr,       &get_sp_basic,
                                nullptr};

// The plug-in's own GPU compute (CUDA kernel launch, GF device work) runs
// outside the PF selector dispatch's SEH guard, so wrap the filter lifecycle
// calls here: a crash inside the plug-in is contained (the render declines and
// falls through) instead of taking the worker down, and its faulting address is
// recorded for diagnosis. Kept in its own POD-only function because __try/__except
// cannot share a frame with C++ objects that need unwinding.
struct PrRenderCrash {
  uint32_t code{};
  void* address{};
  void* access{};
  bool crashed{};
  // A C++ exception that escaped the plug-in call. Caught at a C++ boundary
  // inside the SEH leaf (the selector dispatch's invoke_audited_effect_call_seh
  // shape) so the exception object is destroyed and the trace tells a throw
  // apart from a fault; either one declines the route.
  bool cpp_exception{};
};
bool pr_faulted(const PrRenderCrash& crash) {
  return crash.crashed || crash.cpp_exception;
}
void report_pr_fault(const char* phase, const PrRenderCrash& crash,
                     HMODULE module) {
  if (crash.cpp_exception) {
    std::cerr << "stage:pr_gpu_crash phase=" << phase
              << " kind=cpp_exception base=" << static_cast<void*>(module)
              << "\n" << std::flush;
    return;
  }
  std::cerr << "stage:pr_gpu_crash phase=" << phase << " kind=seh code="
            << std::hex << crash.code << " addr=" << crash.address
            << " access=" << crash.access << std::dec
            << " base=" << static_cast<void*>(module) << "\n"
            << std::flush;
}
abi::prSuiteError entry_cpp_boundary(abi::PrGPUFilterEntryFn entry,
                                     abi::csSDK_uint32 version,
                                     abi::csSDK_int32* index,
                                     abi::prBool in_startup,
                                     abi::piSuites* suites,
                                     abi::PrGPUFilter* filter,
                                     abi::PrGPUFilterInfo* info,
                                     PrRenderCrash* crash) {
  try {
    return entry(version, index, in_startup, suites, filter, info);
  } catch (...) {
    crash->cpp_exception = true;
    return -1;
  }
}
abi::prSuiteError create_instance_cpp_boundary(abi::PrGPUFilter* filter,
                                               abi::PrGPUFilterInstance* instance,
                                               PrRenderCrash* crash) {
  try {
    return filter->CreateInstance(instance);
  } catch (...) {
    crash->cpp_exception = true;
    return -1;
  }
}
abi::prSuiteError dispose_instance_cpp_boundary(abi::PrGPUFilter* filter,
                                                abi::PrGPUFilterInstance* instance,
                                                PrRenderCrash* crash) {
  try {
    return filter->DisposeInstance(instance);
  } catch (...) {
    crash->cpp_exception = true;
    return -1;
  }
}
abi::prSuiteError render_cpp_boundary(
    abi::PrGPUFilter* filter, abi::PrGPUFilterInstance* instance,
    const abi::PrGPUFilterRenderParams* render_params,
    const abi::PPixHand* in_frames, abi::PPixHand* out_frame,
    PrRenderCrash* crash) {
  try {
    return filter->Render(instance, render_params, in_frames, 1, out_frame);
  } catch (...) {
    crash->cpp_exception = true;
    return -1;
  }
}
LONG pr_seh_filter(uint32_t code, _EXCEPTION_POINTERS* info, PrRenderCrash* crash) {
  crash->code = code;
  crash->address = info ? info->ExceptionRecord->ExceptionAddress : nullptr;
  crash->access =
      (info && info->ExceptionRecord->NumberParameters >= 2)
          ? reinterpret_cast<void*>(info->ExceptionRecord->ExceptionInformation[1])
          : nullptr;
  crash->crashed = true;
  return EXCEPTION_EXECUTE_HANDLER;
}
// The plug-in's xGPUFilterEntry startup/shutdown runs plug-in code too (its
// static filter registration and GF device lookups), so it gets the same
// containment as CreateInstance / Render: a crash there declines the route
// with its fault site on stderr instead of taking the worker down.
abi::prSuiteError guarded_entry(abi::PrGPUFilterEntryFn entry,
                                abi::csSDK_uint32 version, abi::csSDK_int32* index,
                                abi::prBool in_startup,
                                abi::piSuites* suites, abi::PrGPUFilter* filter,
                                abi::PrGPUFilterInfo* info, PrRenderCrash* crash) {
  __try {
    return entry_cpp_boundary(entry, version, index, in_startup, suites, filter,
                              info, crash);
  } __except (pr_seh_filter(GetExceptionCode(), GetExceptionInformation(),
                            crash)) {
    return -1;
  }
}
abi::prSuiteError guarded_create_instance(abi::PrGPUFilter* filter,
                                          abi::PrGPUFilterInstance* instance,
                                          PrRenderCrash* crash) {
  __try {
    return create_instance_cpp_boundary(filter, instance, crash);
  } __except (pr_seh_filter(GetExceptionCode(), GetExceptionInformation(),
                            crash)) {
    return -1;
  }
}
abi::prSuiteError guarded_dispose_instance(abi::PrGPUFilter* filter,
                                           abi::PrGPUFilterInstance* instance,
                                           PrRenderCrash* crash) {
  __try {
    return dispose_instance_cpp_boundary(filter, instance, crash);
  } __except (pr_seh_filter(GetExceptionCode(), GetExceptionInformation(),
                            crash)) {
    return -1;
  }
}
abi::prSuiteError guarded_render(abi::PrGPUFilter* filter,
                                 abi::PrGPUFilterInstance* instance,
                                 const abi::PrGPUFilterRenderParams* render_params,
                                 const abi::PPixHand* in_frames,
                                 abi::PPixHand* out_frame, PrRenderCrash* crash) {
  __try {
    return render_cpp_boundary(filter, instance, render_params, in_frames,
                               out_frame, crash);
  } __except (pr_seh_filter(GetExceptionCode(), GetExceptionInformation(),
                            crash)) {
    return -1;
  }
}

// A Premiere GPU filter gates its whole GPU render on MF::IsGPUAccelerationAvailable():
// when it returns false the plug-in takes a path that uses a null device and
// crashes (observed in VRGaussianBlur::Render, faulting on null+0x40 in the GPU
// scratch allocation). In a full Premiere/AE host that flag is set by the
// application's Mercury-GPU enable; this worker stands up the same GPU stack
// (GPUFoundation + CUDA, and GF::Detail::GetDevice returns a live device here)
// but never runs that app-level enable, so the flag reads false. Since the host
// genuinely provides GPU acceleration, redirect the plug-in's own import of that
// query to a stub that reports true, so it takes its normal device path.
bool pr_gpu_available_stub() { return true; }

void force_gpu_acceleration_available(HMODULE module) {
  auto* base = reinterpret_cast<std::byte*>(module);
  const auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
  if (dos->e_magic != IMAGE_DOS_SIGNATURE) return;
  const auto* nt =
      reinterpret_cast<const IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
  if (nt->Signature != IMAGE_NT_SIGNATURE) return;
  const auto& import_dir =
      nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
  if (!import_dir.VirtualAddress) return;
  auto* descriptor = reinterpret_cast<const IMAGE_IMPORT_DESCRIPTOR*>(
      base + import_dir.VirtualAddress);
  for (; descriptor->Name; ++descriptor) {
    if (!descriptor->OriginalFirstThunk || !descriptor->FirstThunk) continue;
    const auto* names = reinterpret_cast<const IMAGE_THUNK_DATA*>(
        base + descriptor->OriginalFirstThunk);
    auto* iat = reinterpret_cast<IMAGE_THUNK_DATA*>(base + descriptor->FirstThunk);
    for (; names->u1.AddressOfData; ++names, ++iat) {
      if (names->u1.Ordinal & IMAGE_ORDINAL_FLAG64) continue;
      const auto* by_name = reinterpret_cast<const IMAGE_IMPORT_BY_NAME*>(
          base + names->u1.AddressOfData);
      if (std::strcmp(by_name->Name, "?IsGPUAccelerationAvailable@MF@@YA_NXZ") !=
          0)
        continue;
      DWORD old_protect{};
      if (VirtualProtect(&iat->u1.Function, sizeof(void*), PAGE_READWRITE,
                         &old_protect)) {
        iat->u1.Function =
            reinterpret_cast<ULONGLONG>(&pr_gpu_available_stub);
        VirtualProtect(&iat->u1.Function, sizeof(void*), old_protect,
                       &old_protect);
      }
      return;
    }
  }
}

// Drive one Premiere GPU-filter render. Returns true only when it produced a
// valid output frame into the guarded buffer; any failure returns false so the
// caller falls through to the ordinary PF SmartFX path (which, for a genuine
// GPU-only VR effect, surfaces its own 512 diagnostic - never worse than before
// this route existed).
bool run_pr_gpu_filter(const Request& request, VideoFrameCpuWorlds& frames,
                       smart_execution::Result& result) {
  // Every exit from this function names itself on stderr as a stage pair the
  // broker records without AEXCOMPAT_EXTENDED_DIAG (issue #1271 review). The
  // lifecycle containment above turned faults that used to take the worker
  // down - a loud record with a classification, an exit code and a minidump -
  // into a quiet fall-through to the PF path, and a plug-in that lands in
  // `rendered` with nothing saying the GPU route was tried and declined is the
  // silent success this project does not allow. Each `reason` is a fixed
  // lower-case identifier chosen here, which is the shape the broker's parser
  // admits - that check is what keeps plug-in authored text out of a report,
  // since the plug-in shares this stderr and can print `stage:` lines too.
  const char* outcome = "entered";
  // The `_begin` line names why the route was entered when the PF CPU path
  // was turned away first (issue #1283). Without it a `rendered` record whose
  // `_end` says `committed` cannot be told from one whose CPU path worked and
  // never needed the route, so a host callback refusal that the GPU route then
  // papered over would leave no trace in the sweep record a corpus comparison
  // reads - which needs no `AEXCOMPAT_EXTENDED_DIAG`, so this cannot be a
  // trace-only line. `render_sweep` lifts it to `worker.pr_gpu_route_entered_from`.
  // Same lower-case identifier shape as the `_end` reason, for the same parser.
  const char* const retry_cause = [] {
    switch (smart_setup::pr_gpu_retry_cause()) {
      case 512: return "cpu_internal_struct_damaged";
      case 516: return "cpu_bad_callback_param";
      default: return static_cast<const char*>(nullptr);
    }
  }();
  std::cerr << "stage:pr_gpu_route_begin";
  if (retry_cause) std::cerr << " reason=" << retry_cause;
  std::cerr << "\n" << std::flush;
  struct RouteOutcome {
    const char** reason;
    ~RouteOutcome() {
      std::cerr << "stage:pr_gpu_route_end reason=" << *reason << "\n"
                << std::flush;
    }
  } route_outcome{&outcome};
  const auto decline = [&outcome](const char* reason) {
    outcome = reason;
    return false;
  };
  const auto& plan = *request.plan;
  const int32_t width = plan.width;
  const int32_t height = plan.height;
  if (width <= 0 || height <= 0) return decline("bad_extent");
  const HMODULE module = active_plugin::effect_module;
  if (!module) return decline("no_module");
  const auto entry = reinterpret_cast<abi::PrGPUFilterEntryFn>(
      GetProcAddress(module, abi::kGPUFilterEntryExport));
  if (!entry) {
    return decline("no_entry_export");
  }
  if (!frames.pr_gpu_ready()) {
    return decline("gpu_unavailable");
  }
  ensure_suite_tables();

  // CreateGPUVideoFrame and the plug-in's own device work need the GPU backend
  // context current on this thread. The PF SmartFX GPU path establishes it via
  // begin_backend_context before its create_gpu; this route reaches create_gpu
  // outside that flow (VR effects do not GPU-negotiate), so establish it here.
  namespace transport = gpu_runtime::memory_world_transport;
  if (!transport::begin_backend_context(/*framework=*/3, /*device_index=*/0,
                                        frames.cuda_context())) {
    return decline("no_backend_context");
  }

  force_gpu_acceleration_available(module);

  HostContext context;
  context.frames = &frames;
  context.param_state = &params_rt::state();
  context.node_id = 1;
  context.width = width;
  context.height = height;
  context.cuda_context = frames.cuda_context();
  g_ctx = &context;
  struct ContextGuard {
    ~ContextGuard() {
      g_ctx = nullptr;
      gpu_runtime::memory_world_transport::end_backend_context(/*framework=*/3);
    }
  } context_guard;

  abi::piSuites suites{};
  suites.piInterfaceVer = 9;
  suites.utilFuncs = &g_util_funcs;

  abi::PrGPUFilter filter{};
  abi::PrGPUFilterInfo info{};
  int32_t startup_index = 0;
  PrRenderCrash crash;
  const abi::prSuiteError startup_error = guarded_entry(
      entry, abi::kPrSDKGPUFilterInterfaceVersion, &startup_index,
      /*inStartup=*/1, &suites, &filter, &info, &crash);
  if (pr_faulted(crash)) {
    report_pr_fault("startup", crash, module);
    return decline("startup_fault");
  }
  std::cerr << "stage:pr_gpu_startup_end error=" << startup_error << "\n"
            << std::flush;
  if (!abi::suite_ok(startup_error)) return decline("startup_error");
  bool shutdown_done = false;
  const auto shutdown = [&] {
    if (shutdown_done) return;
    shutdown_done = true;
    int32_t shutdown_index = 0;
    PrRenderCrash shutdown_crash;
    guarded_entry(entry, abi::kPrSDKGPUFilterInterfaceVersion, &shutdown_index,
                  /*inStartup=*/0, &suites, &filter, &info, &shutdown_crash);
    if (pr_faulted(shutdown_crash))
      report_pr_fault("shutdown", shutdown_crash, module);
  };
  // Startup is balanced by shutdown on every exit. Each one below calls it
  // explicitly - the success path has to shut the filter down before the
  // readback, so the call cannot simply be deferred to scope exit - and this
  // guard is the net for an exit that forgets to: with the lambda idempotent
  // it fires only when nothing else did. It is a maintenance net, not a
  // throw-safety property: no C++ handler exists above this frame, so MSVC
  // terminates at the throw point without unwinding, and an allocation failure
  // in the region below would take the worker rather than reach this
  // destructor. Declared after `context_guard`, so it runs first and the
  // filter shuts down while the GPU backend context is still current.
  struct ShutdownGuard {
    const decltype(shutdown)* run;
    ~ShutdownGuard() { (*run)(); }
  } shutdown_guard{&shutdown};
  // Startup succeeded, so it is balanced by shutdown from here on, including
  // when the filter table it filled in is unusable.
  if (!filter.CreateInstance || !filter.Render) {
    shutdown();
    return decline("no_filter_table");
  }

  // Build the input GPU PPix and upload the host's input pixels into it as
  // float32 ARGB. A float32 session's world uploads as-is; an 8/16bpc session
  // (issue #1271: every VR effect was 512 at depth 8/16 because this route was
  // float32-only and the PF CPU path is GPU-only) widens its world into a
  // float32 staging copy first, so the plug-in sees the 32f frame it renders.
  const int32_t session_pixel_bytes = plan.pixel_bytes;
  if (session_pixel_bytes != 4 && session_pixel_bytes != 8 &&
      session_pixel_bytes != 16) {
    shutdown();
    return decline("unsupported_depth");
  }
  void* input_pixels{};
  std::memcpy(&input_pixels, request.input_world->data() + 24,
              sizeof(input_pixels));
  const int32_t input_rowbytes = read<int32_t>(*request.input_world, 32);
  const int32_t float_rowbytes = width * 16;
  // Fail closed on a stride that cannot hold the row (mirrors
  // gpu_memcpy_frame's own check on the float32 side) rather than over-read.
  // Checked before any GPU frame exists so nothing is left to dispose.
  if (input_pixels &&
      (input_rowbytes < 0 ||
       static_cast<std::size_t>(input_rowbytes) <
           static_cast<std::size_t>(width) * session_pixel_bytes)) {
    shutdown();
    return decline("bad_input_stride");
  }
  std::vector<float> input_float32;
  const void* upload_pixels = input_pixels;
  int32_t upload_rowbytes = input_rowbytes;
  if (input_pixels && session_pixel_bytes != 16) {
    try {
      input_float32.resize(static_cast<std::size_t>(width) * height * 4);
    } catch (const std::exception&) {
      shutdown();
      return decline("input_staging_alloc");
    }
    for (int32_t y = 0; y < height; ++y) {
      const auto* row = static_cast<const unsigned char*>(input_pixels) +
                        static_cast<std::size_t>(y) * input_rowbytes;
      float* out_row = input_float32.data() + static_cast<std::size_t>(y) * width * 4;
      for (int32_t x = 0; x < width; ++x)
        render_pixel_transport::argb_to_argb32f(
            out_row + static_cast<std::size_t>(x) * 4,
            row + static_cast<std::size_t>(x) * session_pixel_bytes,
            session_pixel_bytes);
    }
    upload_pixels = input_float32.data();
    upload_rowbytes = float_rowbytes;
  }
  auto input_record = std::make_unique<FrameRecord>();
  input_record->width = width;
  input_record->height = height;
  if (!frames.pr_make_gpu_ppix(input_record->world, input_record->live, width,
                               height)) {
    shutdown();
    return decline("input_frame_alloc");
  }
  if (upload_pixels &&
      !frames.pr_upload(input_record->world, upload_pixels, upload_rowbytes,
                        width, height)) {
    shutdown();
    return decline("input_upload");
  }
  const abi::PPixHand input_ppix =
      reinterpret_cast<abi::PPixHand>(frames.pr_ppix(input_record->world));
  context.gpu_frames.push_back(std::move(input_record));

  abi::PrGPUFilterInstance instance{};
  instance.piSuitesP = &suites;
  instance.inDeviceIndex = 0;
  instance.inTimelineID = 1;
  instance.inNodeID = context.node_id;
  instance.ioPrivatePluginData = nullptr;
  // DisposeInstance is plug-in code as well; a fault there is contained and
  // recorded like the other three lifecycle calls, and never stops the shutdown
  // that follows it.
  const auto dispose_instance = [&] {
    if (!filter.DisposeInstance) return;
    PrRenderCrash dispose_crash;
    guarded_dispose_instance(&filter, &instance, &dispose_crash);
    if (pr_faulted(dispose_crash))
      report_pr_fault("dispose_instance", dispose_crash, module);
  };
  const abi::prSuiteError create_error =
      guarded_create_instance(&filter, &instance, &crash);
  if (pr_faulted(crash)) {
    report_pr_fault("create_instance", crash, module);
    shutdown();
    return decline("create_instance_fault");
  }
  if (!abi::suite_ok(create_error)) {
    // Deliberately not disposed: the plug-in reported that it did not create
    // the instance, so handing it back for disposal would be a call it never
    // agreed to take. Anything it allocated before deciding to fail is its own
    // to release at shutdown.
    shutdown();
    return decline("create_instance_error");
  }

  abi::PrGPUFilterRenderParams render_params{};
  render_params.inQuality = 4;  // Max
  render_params.inDownsampleFactorX = 1.0f;
  render_params.inDownsampleFactorY = 1.0f;
  render_params.inRenderWidth = static_cast<abi::csSDK_uint32>(width);
  render_params.inRenderHeight = static_cast<abi::csSDK_uint32>(height);
  render_params.inRenderPARNum = 1;
  render_params.inRenderPARDen = 1;
  render_params.inRenderField = 2;  // both fields

  // Hand the plug-in a valid outFrame destination (some effects check it before
  // rendering), but recover the rendered pixels from the CreateGPUPPix frame the
  // plug-in actually renders into - the VR filters ignore this destination and
  // return their own frame.
  auto output_record = std::make_unique<FrameRecord>();
  output_record->width = width;
  output_record->height = height;
  if (!frames.pr_make_gpu_ppix(output_record->world, output_record->live, width,
                               height)) {
    dispose_instance();
    shutdown();
    return decline("output_frame_alloc");
  }
  abi::PPixHand host_output_ppix =
      reinterpret_cast<abi::PPixHand>(frames.pr_ppix(output_record->world));
  context.gpu_frames.push_back(std::move(output_record));

  abi::PPixHand in_frames[1] = {input_ppix};
  abi::PPixHand out_frame = host_output_ppix;
  std::cerr << "stage:pr_gpu_render_begin\n" << std::flush;
  const abi::prSuiteError render_error = guarded_render(
      &filter, &instance, &render_params, in_frames, &out_frame, &crash);
  std::cerr << "stage:pr_gpu_render_end error=" << render_error << "\n"
            << std::flush;
  if (pr_faulted(crash)) {
    report_pr_fault("render", crash, module);
    dispose_instance();
    shutdown();
    return decline("render_fault");
  }

  // Locate the rendered frame, in order of how reliably it names the output:
  // (1) a plug-in frame named through outFrame, (2) the most recent plug-in
  // CreateGPUPPix frame (VR filters render there without writing outFrame),
  // (3) the host-provided output frame (plug-ins that render into *outFrame and
  // create nothing, e.g. VRColorGradient). Deferred disposal keeps every frame
  // live until after this readback.
  //
  // Limitation (issue #1058 follow-up): step 2 is a heuristic. A plug-in that
  // keeps several CreateGPUPPix frames alive (a ping-pong / multi-pass design),
  // or that renders its final result into the host outFrame while leaving an
  // undisposed scratch frame behind, can leave the real output in a frame this
  // loop does not choose - step 2 would pick the scratch frame before step 3
  // could reach the host frame. And step 3 trusts that a success-returning
  // plug-in fully wrote the host frame: one that reports success, allocates no
  // frame, and writes only a sub-rect (assuming the host pre-cleared or
  // pre-copied the input) would have its unwritten region read back as the
  // frame's recycled device memory, not zero. The dimension cross-check below
  // only rejects a *size* mismatch; a wrong or partially-written frame of the
  // right size is read as if it were the output. Every effect in the current
  // corpus produces a single fully-written output frame, so this holds for them;
  // widening the corpus needs a real output-frame signal.
  FrameRecord* out_record = nullptr;
  if (abi::suite_ok(render_error)) {
    // 1. The plug-in created its own frame and handed it back through outFrame.
    if (out_frame) {
      FrameRecord* named = find_frame(out_frame);
      if (named && named->plugin_created) out_record = named;
    }
    // 2. It created its own frame but did not repoint outFrame (the VR filters
    //    render into their CreateGPUPPix frame and leave outFrame alone).
    if (!out_record)
      for (auto it = context.gpu_frames.rbegin(); it != context.gpu_frames.rend();
           ++it)
        if ((*it)->plugin_created) {
          out_record = it->get();
          break;
        }
    // 3. It created no frame of its own and rendered into the host-provided
    //    output frame (the plain PF/Premiere contract: render into *outFrame).
    //    Only reachable when no plug-in frame exists, so this never shadows the
    //    #828 case (plug-in renders into its own frame, leaving the host frame
    //    zeroed) - that case always has a plugin_created frame above.
    if (!out_record && out_frame == host_output_ppix)
      out_record = find_frame(host_output_ppix);
  }

  dispose_instance();
  shutdown();

  // Read back the plug-in's own output extent, not the requested plan size. A
  // Premiere GPU filter may legitimately produce a frame of a different size
  // than requested - VRConverter, driven by its "Output Frame Ratio" popup,
  // reprojects a 256x256 equirect input to a 256x128 2:1 frame (Project
  // Direction 3: verify variable output size). Read the frame's size from its VF
  // world (offsets 36/40), not the FrameRecord scalar members: building the
  // world leaves FrameRecord::height at height+1 (see GPUDev_CreateGPUPPix), so
  // those members are unreliable (issue #1058). The world width/height are what
  // create_gpu validated the allocation against, so reading exactly that many
  // rows of that stride never over-reads the device allocation - the fail-closed
  // property the earlier plan-size cross-check provided. Reject only a
  // degenerate (non-positive) extent.
  int32_t out_frame_w = 0, out_frame_h = 0;
  if (out_record)
    frames.pr_world_dims(out_record->world, out_frame_w, out_frame_h);
  if (!abi::suite_ok(render_error)) return decline("render_error");
  if (!out_record || out_frame_w <= 0 || out_frame_h <= 0)
    return decline("no_output_frame");

  // The plug-in's frame is float32 ARGB. A float32 session downloads it
  // straight into the guarded output; an 8/16bpc session downloads into a
  // float32 staging copy and narrows it into the guarded output at the session
  // depth, so what leaves this route is a world of the depth the session
  // registered (the finalize copy, the hash and the pixel validation all read
  // it at `plan.pixel_bytes`). The output extent stays the plug-in's own.
  const int32_t out_float_rowbytes = out_frame_w * 16;
  const int32_t rowbytes = out_frame_w * session_pixel_bytes;
  if (!request.guarded->reset(static_cast<std::size_t>(rowbytes) * out_frame_h))
    return decline("output_buffer_alloc");
  *request.destination = request.guarded->data();
  std::vector<float> output_float32;
  void* download_pixels = request.guarded->data();
  if (session_pixel_bytes != 16) {
    // The extent is the plug-in's own; a staging copy the host cannot allocate
    // is a refused frame, the same shape as `guarded->reset` failing above.
    try {
      output_float32.resize(static_cast<std::size_t>(out_frame_w) * out_frame_h * 4);
    } catch (const std::exception&) {
      return decline("output_staging_alloc");
    }
    download_pixels = output_float32.data();
  }
  if (!frames.pr_download(out_record->world, download_pixels, out_float_rowbytes,
                          out_frame_w, out_frame_h))
    return decline("download_failed");
  // The float32 session's finalize rejects a non-finite output
  // (output_pixels_valid false, -6). Narrowing would silently turn NaN into 0
  // and +-inf into the bounds, so the check is taken here on the float frame
  // the plug-in produced, before the narrowing, and handed to finalize as
  // `output_non_finite` so it lands in the same verdict: the depth a session
  // renders at must not decide whether a broken output is a diagnostic.
  // Held locally until the route commits: every field this function publishes
  // into `result` is written on the success path below, because a declined
  // route falls through to the ordinary PF render and must not colour its
  // verdict with what the GPU frame contained. That applies to `result` only -
  // the guarded buffer, `*request.destination` and the output world are
  // rewritten before the last failure returns and are not restored. The PF
  // path re-establishes all three before it renders, so the values a declined
  // route leaves behind describe this session's own depth and the plug-in's
  // extent rather than a foreign layout (before issue #1271 they described a
  // 16-byte float32 pixel in an 8-bit session).
  bool output_non_finite = false;
  if (session_pixel_bytes != 16) {
    output_non_finite =
        !std::all_of(output_float32.begin(), output_float32.end(),
                     [](float value) { return std::isfinite(value); });
    if (output_non_finite)
      std::cerr << "stage:pr_gpu_output_non_finite\n" << std::flush;
    for (int32_t y = 0; y < out_frame_h; ++y) {
      const float* row =
          output_float32.data() + static_cast<std::size_t>(y) * out_frame_w * 4;
      auto* out_row = request.guarded->data() + static_cast<std::size_t>(y) * rowbytes;
      for (int32_t x = 0; x < out_frame_w; ++x)
        render_pixel_transport::argb32f_to_argb(
            out_row + static_cast<std::size_t>(x) * session_pixel_bytes,
            row + static_cast<std::size_t>(x) * 4, session_pixel_bytes);
    }
  }

  // TEMP (#1058 correctness): dump the input and rendered output as raw float32
  // ARGB (the frames the plug-in saw and produced, before any session-depth
  // narrowing) so an AE-oracle comparison can settle the channel order.
  // Env-gated.
  if (const char* dump_path = std::getenv("AEXCOMPAT_PR_GPU_DUMP")) {
    const auto write_raw = [&](const std::string& path, const void* pixels,
                               int32_t w, int32_t h, int32_t src_rowbytes) {
      if (!pixels) return;
      std::ofstream file(path, std::ios::binary);
      const int32_t header[2] = {w, h};
      file.write(reinterpret_cast<const char*>(header), sizeof(header));
      for (int32_t y = 0; y < h; ++y)
        file.write(static_cast<const char*>(pixels) +
                       static_cast<std::size_t>(y) * src_rowbytes,
                   static_cast<std::size_t>(w) * 16);
    };
    write_raw(std::string(dump_path) + ".in", upload_pixels, width, height,
              upload_rowbytes);
    write_raw(std::string(dump_path) + ".out", download_pixels, out_frame_w,
              out_frame_h, out_float_rowbytes);
  }

  if (!render::prepare_world_layout(
          *request.output_world,
          {session_pixel_bytes == 4 ? 0 : 1, session_pixel_bytes, out_frame_w,
           out_frame_h, rowbytes},
          *request.destination) ||
      !request.formats->register_world(request.output_world->data(),
                                       request.dispatch_pixel_format))
    return decline("world_publish_failed");

  result.gpu_setup_error = 0;
  result.pre_error = 0;
  result.selector_error = 0;
  result.render_error = 0;
  result.output_non_finite = output_non_finite;
  result.rects_valid = true;
  result.empty_result_rect = false;
  result.result_rect = {0, 0, out_frame_w, out_frame_h};
  result.max_result_rect = {0, 0, out_frame_w, out_frame_h};
  result.result_within_request = true;
  result.output_width = out_frame_w;
  result.output_height = out_frame_h;
  result.output_rowbytes = rowbytes;
  outcome = output_non_finite ? "committed_non_finite" : "committed";
  return true;
}

}  // namespace pr_host

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

bool pr_gpu_filter_route_available() {
  return active_plugin::effect_module &&
      GetProcAddress(active_plugin::effect_module,
                     pr_gpu::kGPUFilterEntryExport) != nullptr;
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

  // Premiere GPU-filter route (issue #1058): the VR / Immersive effect family
  // exports xGPUFilterEntry and is GPU-only - its PF SmartFX CPU path only draws
  // a "requires GPU acceleration" warning and returns 512. Drive the Premiere
  // GPU filter directly instead. The filter renders 32f frames: a float32
  // session takes this route first, as before. An 8/16bpc session takes it
  // only on the frame loop's retry after the PF CPU path answered 512 (issue
  // #1271; the input is widened to 32f and the output narrowed back inside
  // run_pr_gpu_filter). Not export-first at 8/16: other AE effects export
  // xGPUFilterEntry with a working PF CPU path (Levels2, Box_Blur, Lumetri,
  // DirectionalBlur, ...) and routing them here first at the default depth
  // measured as crashes and changed pixels against their PF renders. Any
  // failure falls through to the ordinary PF path below, so a genuine
  // GPU-only effect is never made worse than its pre-existing 512.
  if (pr_gpu_filter_route_available() &&
      (plan.float32 || smart_setup::force_pr_gpu_retry_requested())) {
    result.pr_gpu_route_attempted = true;
    VideoFrameCpuWorlds pr_filter_frames;
    if (pr_host::run_pr_gpu_filter(request, pr_filter_frames, result))
      return true;
  }

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
    write<int32_t>(*request.input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_X_OFFSET,
                   -smart_bounds.result_rect[0]);
    write<int32_t>(*request.input, aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_Y_OFFSET,
                   -smart_bounds.result_rect[1]);
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
    // Snapshot around this one call so `selector_failure_substituted` names
    // the Smart Render selector and nothing else in the frame (issue #1271).
    const uint64_t substitutions_before =
        selector_dispatch_telemetry().substituted_selector_failures;
    result.selector_error = hooks.guarded_call(request.entry, render_selector,
        request.input->data(), request.output->data(), params.data(), nullptr,
        smart_extra.data());
    result.selector_failure_substituted =
        selector_dispatch_telemetry().substituted_selector_failures !=
        substitutions_before;
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
