#pragma once

// Clean-room ABI for hosting Adobe Premiere Pro "GPU filter" plug-ins
// (`xGPUFilterEntry`), authored from observed call sites in the plug-in binaries
// plus the publicly documented Premiere Pro SDK suite contracts. This is NOT a
// copy of any Adobe SDK header: only the structures and suite vtables the host
// actually has to satisfy are declared here, with the field/method offsets the
// plug-ins depend on.
//
// Why this exists: several bundled AE effects (the VR / Immersive family:
// VRGaussianBlur, VRSharpen, ...) are GPU-only Premiere-style filters. Their AE
// SmartFX CPU path only draws a "requires GPU acceleration" warning, so the only
// way to render them is to drive their private `xGPUFilterEntry` export the way
// Premiere Pro (and AE's own bridge) does: hand the plug-in a `piSuites` host and
// the Premiere GPU suites, then call CreateInstance / Render / DisposeInstance.
//
// ABI notes verified against the plug-in binaries and the SDK headers:
//   * The real SDK packs these structs with #pragma pack(1); the offsets below
//     bake that in (e.g. PrParam value at +4, ioPrivatePluginData at +0x14).
//   * Every suite is a flat array of 8-byte function pointers, so a method the
//     plug-in reaches at struct offset +0xNN is function-pointer index 0xNN/8.

#include <cstddef>
#include <cstdint>

namespace aexcompat::worker_runtime::pr_gpu {

#pragma pack(push, 1)

using prSuiteError = int32_t;   // >= 0 == success; 0 == suiteError_NoError
using csSDK_int32 = int32_t;
using csSDK_uint32 = uint32_t;
using csSDK_size_t = uint64_t;
using PrTime = int64_t;
using PrTimelineID = int32_t;
using prBool = int32_t;
using prFieldType = int32_t;
using PrMemoryPtr = char*;

constexpr prSuiteError kSuiteError_NoError = 0;
inline bool suite_ok(prSuiteError e) { return e >= 0; }

// PrParamType tags (only the ones the value converter emits).
enum PrParamType : int32_t {
  kPrParamType_Int32 = 3,
  kPrParamType_Int64 = 4,
  kPrParamType_Float32 = 5,
  kPrParamType_Float64 = 6,
  kPrParamType_Bool = 7,
  kPrParamType_Point = 8,
};

// pack(1): the value union starts at byte +4, right after the 4-byte type tag.
struct PrParam {
  PrParamType mType;   // +0x00
  union {              // +0x04
    int32_t mInt32;
    int64_t mInt64;
    float mFloat32;
    double mFloat64;
    uint8_t mBool;
    struct { double x, y; } mPoint;
    char mRaw[37];
  };
};

// FourCC pixel formats the plug-ins ask about / create. Adobe-private GPU
// formats use the '@CD?' prefix.
enum PrPixelFormat : int32_t {
  kPrPixelFormat_Invalid = 0x66646162,          // 'badf' sentinel
  kPrPixelFormat_GPU_BGRA_4444_16f = 0x61444340, // '@CDa'
  kPrPixelFormat_GPU_BGRA_4444_32f = 0x41444340, // '@CDA'
  kPrPixelFormat_BGRA_4444_32f = 0x61724742,     // 'BGra'
};

struct prRect { csSDK_int32 left, top, right, bottom; };

// -------- piSuites host chain: piSuites->utilFuncs->getSPBasicSuite() --------
struct SPBasicSuite {
  prSuiteError (*AcquireSuite)(const char* name, int32_t version, const void** suite);
  prSuiteError (*ReleaseSuite)(const char* name, int32_t version);
  uint8_t (*IsEqual)(const char* t1, const char* t2);
  prSuiteError (*AllocateBlock)(size_t size, void** block);
  prSuiteError (*FreeBlock)(void* block);
  prSuiteError (*ReallocateBlock)(void* block, size_t newSize, void** newBlock);
  prSuiteError (*Undefined)();
};

struct PlugUtilFuncs {         // pack(1); getSPBasicSuite is index 7 (+0x38)
  void* unused0;
  void* unused1;
  void* unused2;
  void* unused3;
  void* unused4;
  void* unused5;
  void* unused6;
  SPBasicSuite* (*getSPBasicSuite)();
  void* unused8;
};

struct piSuites {              // pack(1); reachable as PrGPUFilterInstance.piSuites
  int32_t piInterfaceVer;      // +0x00
  void* memFuncs;              // +0x04
  void* windFuncs;             // +0x0c
  void* ppixFuncs;             // +0x14
  PlugUtilFuncs* utilFuncs;    // +0x1c
  void* timelineFuncs;         // +0x24
};

// -------- Instance + render params + the 5-function filter table --------
struct PrGPUFilterInstance {   // pack(1)
  piSuites* piSuitesP;         // +0x00
  csSDK_uint32 inDeviceIndex;  // +0x08
  PrTimelineID inTimelineID;   // +0x0c
  csSDK_int32 inNodeID;        // +0x10
  void* ioPrivatePluginData;   // +0x14  (plug-in owned)
  prBool outIsRealtime;        // +0x1c
};

struct PrGPUFilterRenderParams {  // pack(1)
  PrTime inClipTime;              // +0x00
  PrTime inSequenceTime;          // +0x08
  int32_t inQuality;              // +0x10
  float inDownsampleFactorX;      // +0x14
  float inDownsampleFactorY;      // +0x18
  csSDK_uint32 inRenderWidth;     // +0x1c
  csSDK_uint32 inRenderHeight;    // +0x20
  csSDK_uint32 inRenderPARNum;    // +0x24
  csSDK_uint32 inRenderPARDen;    // +0x28
  prFieldType inRenderFieldType;  // +0x2c
  PrTime inRenderTicksPerFrame;   // +0x30
  int32_t inRenderField;          // +0x38
};

using PPixHand = void**;

struct PrGPUFilter {
  prSuiteError (*CreateInstance)(PrGPUFilterInstance*);
  prSuiteError (*DisposeInstance)(PrGPUFilterInstance*);
  prSuiteError (*GetFrameDependencies)(PrGPUFilterInstance*,
                                       const PrGPUFilterRenderParams*,
                                       csSDK_int32* ioQueryIndex, void* outDeps);
  prSuiteError (*Precompute)(PrGPUFilterInstance*,
                             const PrGPUFilterRenderParams*, csSDK_int32 index,
                             PPixHand inFrame);
  prSuiteError (*Render)(PrGPUFilterInstance*, const PrGPUFilterRenderParams*,
                         const PPixHand* inFrames, csSDK_size_t inFrameCount,
                         PPixHand* outFrame);
};

struct PrSDKString { int64_t opaque[2]; };
struct PrGPUFilterInfo {          // pack(1)
  csSDK_uint32 outInterfaceVersion;  // +0x00
  PrSDKString outMatchName;          // +0x04
};

using PrGPUFilterEntryFn = prSuiteError (*)(csSDK_uint32 inHostInterfaceVersion,
                                            csSDK_int32* ioIndex, prBool inStartup,
                                            piSuites* piSuitesP,
                                            PrGPUFilter* outFilter,
                                            PrGPUFilterInfo* outFilterInfo);

constexpr int32_t kPrSDKGPUFilterInterfaceVersion = 2;

// -------- Suites the plug-in Initialize() acquires and Render() calls --------
struct PrGPUDeviceInfo {          // pack(1)
  int32_t outDeviceFramework;     // +0x00  (CUDA=0, OpenCL=1, Metal=2)
  prBool outMeetsMinReq;          // +0x04
  void* outPlatformHandle;        // +0x08
  void* outDeviceHandle;          // +0x10
  void* outContextHandle;         // +0x18  (CUcontext)
  void* outCommandQueueHandle;    // +0x20  (CUstream)
  void* outOffscreenGLContext;    // +0x28
  void* outOffscreenGLDevice;     // +0x30
};

struct PrSDKGPUDeviceSuite {
  prSuiteError (*GetDeviceCount)(csSDK_uint32* outCount);                       // 0
  prSuiteError (*GetDeviceInfo)(csSDK_uint32 version, csSDK_uint32 deviceIndex, // 1
                                PrGPUDeviceInfo* out);
  prSuiteError (*AcquireExclusiveDeviceAccess)(csSDK_uint32);                   // 2
  prSuiteError (*ReleaseExclusiveDeviceAccess)(csSDK_uint32);                   // 3
  prSuiteError (*AllocateDeviceMemory)(csSDK_uint32, size_t, void**);           // 4
  prSuiteError (*FreeDeviceMemory)(csSDK_uint32, void*);                        // 5
  prSuiteError (*PurgeDeviceMemory)(csSDK_uint32, size_t, size_t*);             // 6
  prSuiteError (*AllocateHostMemory)(csSDK_uint32, size_t, void**);             // 7
  prSuiteError (*FreeHostMemory)(csSDK_uint32, void*);                          // 8
  prSuiteError (*PurgeHostMemory)(csSDK_uint32, size_t, size_t*);               // 9
  prSuiteError (*CreateGPUPPix)(csSDK_uint32 deviceIndex, PrPixelFormat,        // 10
                                int32_t width, int32_t height, int32_t parNum,
                                int32_t parDen, prFieldType, PPixHand* out);
  prSuiteError (*GetGPUPPixData)(PPixHand, void** outData);                     // 11
  prSuiteError (*GetGPUPPixDeviceIndex)(PPixHand, csSDK_uint32* outIndex);      // 12
  prSuiteError (*GetGPUPPixSize)(PPixHand, size_t* outSize);                    // 13
};

struct PrSDKPPixSuite {
  prSuiteError (*Dispose)(PPixHand);                                            // 0
  prSuiteError (*GetPixels)(PPixHand, int32_t access, char** outAddr);          // 1
  prSuiteError (*GetBounds)(PPixHand, prRect* out);                             // 2
  prSuiteError (*GetRowBytes)(PPixHand, csSDK_int32* out);                      // 3
  prSuiteError (*GetPixelAspectRatio)(PPixHand, csSDK_uint32* num,             // 4
                                      csSDK_uint32* den);
  prSuiteError (*GetPixelFormat)(PPixHand, PrPixelFormat* out);                 // 5
  prSuiteError (*GetUniqueKey)(PPixHand, unsigned char*, size_t);               // 6
  prSuiteError (*GetUniqueKeySize)(size_t* out);                                // 7
  prSuiteError (*GetRenderTime)(PPixHand, csSDK_int32* out);                    // 8
};

struct PrSDKPPix2Suite {
  prSuiteError (*GetSize)(PPixHand, size_t* out);                               // 0
  prSuiteError (*GetYUV420PlanarBuffers)(PPixHand, int32_t, char**, csSDK_uint32*,
                                         char**, csSDK_uint32*, char**,
                                         csSDK_uint32*);                        // 1
  prSuiteError (*GetOrigin)(PPixHand, csSDK_int32* x, csSDK_int32* y);          // 2
  prSuiteError (*GetFieldOrder)(PPixHand, prFieldType* out);                    // 3
};

// Only the leading methods up to GetParam (index 17) are declared; later ones
// are padded so the vtable offset of GetParam matches +0x88.
struct PrSDKVideoSegmentSuite {
  void* slot0_AcquireVideoSegmentsID;
  void* slot1;
  void* slot2;
  void* slot3_ReleaseVideoSegmentsID;
  void* slot4_GetHash;
  void* slot5_GetSegmentCount;
  void* slot6_GetSegmentInfo;
  void* slot7_AcquireNodeID;
  void* slot8_ReleaseVideoNodeID;
  void* slot9_GetNodeInfo;
  void* slot10_GetNodeInputCount;
  void* slot11_AcquireInputNodeID;
  void* slot12_GetNodeOperatorCount;
  void* slot13_AcquireOperatorNodeID;
  void* slot14_IterateNodeProperties;
  prSuiteError (*GetNodeProperty)(csSDK_int32 nodeID, const char* key,          // 15
                                  PrMemoryPtr* outValue);
  prSuiteError (*GetParamCount)(csSDK_int32 nodeID, csSDK_int32* out);          // 16
  prSuiteError (*GetParam)(csSDK_int32 nodeID, csSDK_int32 index, PrTime,       // 17
                           PrParam* out);
  // The plug-ins in scope call only GetParam / GetNodeProperty / GetParamCount;
  // later methods are left unimplemented (a plug-in that reaches them gets a
  // null slot, which is a diagnosable crash rather than a silent wrong answer).
  void* tail[16];
};

struct PrSDKMemoryManagerSuite {   // V4 table; PrDisposePtr at index 11 (+0x58)
  void* slot0_ReserveMemory;
  void* slot1_GetMemoryManagerSize;
  void* slot2_AddBlock;
  void* slot3_TouchBlock;
  void* slot4_RemoveBlock;
  PrMemoryPtr (*NewPtrClear)(csSDK_uint32 byteCount);        // 5
  PrMemoryPtr (*NewPtr)(csSDK_uint32 byteCount);             // 6
  csSDK_uint32 (*GetPtrSize)(PrMemoryPtr);                   // 7
  void (*SetPtrSize)(PrMemoryPtr*, csSDK_uint32);            // 8
  void* slot9_NewHandle;
  void* slot10_NewHandleClear;
  void (*PrDisposePtr)(PrMemoryPtr);                         // 11
  void* slot12_DisposeHandle;
  void* slot13_SetHandleSize;
  void* slot14_GetHandleSize;
  void* slot15_AdjustReservedMemorySize;
};

// Suites the plug-in acquires but (for the effects in scope) does not drive
// beyond acquisition. A successful acquire returning a table of null slots is
// enough; any real call surfaces as a diagnosable crash, never a wrong pixel.
struct PrSDKSequenceInfoSuite {
  void* slot0_GetFrameRect;
  void* slot1_GetPixelAspectRatio;
  void* slot2_GetFrameRate;
  void* slot3_GetFieldType;
  void* slot4_GetZeroPoint;
  void* slot5_GetTimecodeDropFrame;
  void* slot6_GetProxyFlag;
  prSuiteError (*GetImmersiveVideoVRConfiguration)(PrTimelineID,               // 7
      int32_t* outProjType, int32_t* outFrameLayout,
      csSDK_uint32* outHCapturedView, csSDK_uint32* outVCapturedView);
  void* slot8_GetWorkingColorSpace;
  void* slot9_GetGraphicsWhiteLuminance;
};

// Suite name/version constants (from the acquire call sites in Initialize).
constexpr const char* kGPUDeviceSuite = "MediaCore GPU Device Suite";
constexpr int32_t kGPUDeviceSuiteVersion = 2;
constexpr const char* kGPUImageProcessingSuite = "MediaCore GPU Image Processing Suite";
constexpr int32_t kGPUImageProcessingSuiteVersion = 1;
constexpr const char* kMemoryManagerSuite = "Premiere Memory Manager Suite";
constexpr int32_t kMemoryManagerSuiteVersion = 4;
constexpr const char* kPPixSuite = "Premiere PPix Suite";
constexpr int32_t kPPixSuiteVersion = 1;
constexpr const char* kPPix2Suite = "Premiere PPix 2 Suite";
constexpr int32_t kPPix2SuiteVersion = 3;
constexpr const char* kTransitionSuite = "PF Transition Suite";
constexpr int32_t kTransitionSuiteVersion = 1;
constexpr const char* kVideoSegmentSuite = "MediaCore Video Segment Suite";
constexpr int32_t kVideoSegmentSuiteVersion = 4;
constexpr const char* kOpaqueEffectDataSuite = "Opaque Effect Data Suite";
constexpr int32_t kOpaqueEffectDataSuiteVersion = 2;
constexpr const char* kSequenceInfoSuite = "MediaCore Sequence Info Suite";
constexpr int32_t kSequenceInfoSuiteVersion = 12;

constexpr const char* kGPUFilterEntryExport = "xGPUFilterEntry";

#pragma pack(pop)

}  // namespace aexcompat::worker_runtime::pr_gpu
