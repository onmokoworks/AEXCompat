#pragma once

// This is a deliberately small ABI mirror, not a vendored OpenFX SDK header.
// Provenance: Academy Software Foundation OpenFX, BSD-3-Clause, main branch
// (https://github.com/AcademySoftwareFoundation/openfx; checked 2026-07-23).
// Only the standard struct prefixes used by this bridge are declared here.

#include <stddef.h>

#ifdef _WIN32
#define AEXCOMPAT_OFX_EXPORT extern "C" __declspec(dllexport)
#else
#define AEXCOMPAT_OFX_EXPORT extern "C"
#endif

typedef int OfxStatus;
typedef double OfxTime;
typedef struct OfxPropertySetStruct *OfxPropertySetHandle;
typedef struct OfxImageEffectStruct *OfxImageEffectHandle;
typedef struct OfxImageClipStruct *OfxImageClipHandle;
typedef struct OfxParamSetStruct *OfxParamSetHandle;
typedef struct OfxParamStruct *OfxParamHandle;

typedef struct OfxRectD {
  double x1;
  double y1;
  double x2;
  double y2;
} OfxRectD;

typedef struct OfxRectI {
  int x1;
  int y1;
  int x2;
  int y2;
} OfxRectI;

enum {
  kOfxStatOK = 0,
  kOfxStatFailed = 1,
  kOfxStatErrFatal = 2,
  kOfxStatErrUnknown = 3,
  kOfxStatErrMissingHostFeature = 4,
  kOfxStatErrUnsupported = 5,
  kOfxStatErrExists = 6,
  kOfxStatErrFormat = 7,
  kOfxStatErrMemory = 8,
  kOfxStatErrBadHandle = 9,
  kOfxStatErrBadIndex = 10,
  kOfxStatErrValue = 11,
  kOfxStatReplyDefault = 12,
};

#define kOfxImageEffectPluginApi "OfxImageEffectPluginAPI"
#define kOfxImageEffectPluginApiVersion 1
#define kOfxActionLoad "OfxActionLoad"
#define kOfxActionUnload "OfxActionUnload"
#define kOfxActionDescribe "OfxActionDescribe"
#define kOfxActionCreateInstance "OfxActionCreateInstance"
#define kOfxActionDestroyInstance "OfxActionDestroyInstance"
#define kOfxImageEffectActionDescribeInContext "OfxImageEffectActionDescribeInContext"
#define kOfxImageEffectActionRender "OfxImageEffectActionRender"

#define kOfxImageEffectContextFilter "OfxImageEffectContextFilter"
#define kOfxImageEffectPropSupportedContexts "OfxImageEffectPropSupportedContexts"
#define kOfxImageEffectPropContext "OfxImageEffectPropContext"
#define kOfxImageEffectPropPixelDepth "OfxImageEffectPropPixelDepth"
#define kOfxImageEffectPropComponents "OfxImageEffectPropComponents"
#define kOfxImageEffectPropSupportsTiles "OfxImageEffectPropSupportsTiles"
#define kOfxImageEffectPropRenderWindow "OfxImageEffectPropRenderWindow"
#define kOfxImageEffectPropRenderScale "OfxImageEffectPropRenderScale"
#define kOfxImageEffectPropFrameRange "OfxImageEffectPropFrameRange"
#define kOfxImageEffectPropFrameStep "OfxImageEffectPropFrameStep"
#define kOfxImageEffectPropPreMultiplication "OfxImageEffectPropPreMultiplication"
#define kOfxImageEffectSimpleSourceClipName "Source"
#define kOfxImageEffectOutputClipName "Output"
#define kOfxImageComponentRGBA "OfxImageComponentRGBA"
#define kOfxBitDepthByte "OfxBitDepthByte"
#define kOfxImagePropData "OfxImagePropData"
#define kOfxImagePropBounds "OfxImagePropBounds"
#define kOfxImagePropRowBytes "OfxImagePropRowBytes"
#define kOfxImagePreMultiplied "OfxImagePreMultiplied"
#define kOfxPropTime "OfxPropTime"
#define kOfxPropLabel "OfxPropLabel"
#define kOfxPropInstanceData "OfxPropInstanceData"

#define kOfxPropertySuite "OfxPropertySuite"
#define kOfxImageEffectSuite "OfxImageEffectSuite"
#define kOfxParameterSuite "OfxParameterSuite"
#define kOfxParamTypeDouble "OfxParamTypeDouble"
#define kOfxParamPropDefault "OfxParamPropDefault"
#define kOfxParamPropDisplayMin "OfxParamPropDisplayMin"
#define kOfxParamPropDisplayMax "OfxParamPropDisplayMax"
#define kOfxParamPropDigits "OfxParamPropDigits"

typedef struct OfxHost {
  OfxPropertySetHandle host;
  const void *(*fetchSuite)(OfxPropertySetHandle host, const char *suiteName,
                            int suiteVersion);
} OfxHost;

typedef OfxStatus(OfxPluginEntryPoint)(const char *action, const void *handle,
                                       OfxPropertySetHandle inArgs,
                                       OfxPropertySetHandle outArgs);

typedef struct OfxPlugin {
  const char *pluginApi;
  int apiVersion;
  const char *pluginIdentifier;
  unsigned int pluginVersionMajor;
  unsigned int pluginVersionMinor;
  void (*setHost)(OfxHost *host);
  OfxPluginEntryPoint *mainEntry;
} OfxPlugin;

// Prefix matches OfxPropertySuiteV1 in ofxProperty.h.
typedef struct OfxPropertySuiteV1 {
  OfxStatus (*propSetPointer)(OfxPropertySetHandle, const char *, int, void *);
  OfxStatus (*propSetString)(OfxPropertySetHandle, const char *, int,
                             const char *);
  OfxStatus (*propSetDouble)(OfxPropertySetHandle, const char *, int, double);
  OfxStatus (*propSetInt)(OfxPropertySetHandle, const char *, int, int);
  OfxStatus (*propSetPointerN)(OfxPropertySetHandle, const char *, int,
                               void *const *);
  OfxStatus (*propSetStringN)(OfxPropertySetHandle, const char *, int,
                              const char *const *);
  OfxStatus (*propGetPointer)(OfxPropertySetHandle, const char *, int, void **);
  OfxStatus (*propGetString)(OfxPropertySetHandle, const char *, int, char **);
  OfxStatus (*propGetDouble)(OfxPropertySetHandle, const char *, int, double *);
  OfxStatus (*propGetInt)(OfxPropertySetHandle, const char *, int, int *);
} OfxPropertySuiteV1;

// Prefix matches OfxImageEffectSuiteV1 in ofxImageEffect.h.
typedef struct OfxImageEffectSuiteV1 {
  OfxStatus (*getPropertySet)(OfxImageEffectHandle,
                              OfxPropertySetHandle *);
  OfxStatus (*getParamSet)(OfxImageEffectHandle, OfxParamSetHandle *);
  OfxStatus (*clipDefine)(OfxImageEffectHandle, const char *,
                          OfxPropertySetHandle *);
  OfxStatus (*clipGetHandle)(OfxImageEffectHandle, const char *,
                             OfxImageClipHandle *, OfxPropertySetHandle *);
  OfxStatus (*clipGetPropertySet)(OfxImageClipHandle, OfxPropertySetHandle *);
  OfxStatus (*clipGetImage)(OfxImageClipHandle, OfxTime, const OfxRectD *,
                            OfxPropertySetHandle *);
  OfxStatus (*clipReleaseImage)(OfxPropertySetHandle);
} OfxImageEffectSuiteV1;

// Prefix matches OfxParameterSuiteV1 in ofxParam.h.
typedef struct OfxParameterSuiteV1 {
  OfxStatus (*paramDefine)(OfxParamSetHandle, const char *, const char *,
                           OfxPropertySetHandle *);
} OfxParameterSuiteV1;

AEXCOMPAT_OFX_EXPORT int OfxGetNumberOfPlugins(void);
AEXCOMPAT_OFX_EXPORT OfxPlugin *OfxGetPlugin(int nth);
AEXCOMPAT_OFX_EXPORT OfxStatus OfxSetHost(const OfxHost *host);

// Resolve-facing audit alias. Standard hosts use OfxPlugin::mainEntry; this
// export is intentionally kept as a direct forwarder for symbol inspection.
AEXCOMPAT_OFX_EXPORT OfxStatus OfxPluginMain(const char *action,
                                             const void *handle,
                                             OfxPropertySetHandle inArgs,
                                             OfxPropertySetHandle outArgs);
