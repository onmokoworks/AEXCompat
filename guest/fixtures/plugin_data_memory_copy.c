/*
 * Header-free Win64 PluginData + CRT memory-copy fixture.
 *
 * Build with MSVC (disable intrinsic expansion so imports remain observable):
 *   cl /nologo /c /O2 /Oi- /GS- /Zl plugin_data_memory_copy.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain
 *        /OUT:plugin_data_memory_copy.aex
 *        plugin_data_memory_copy.obj vcruntime.lib
 */

typedef unsigned long long u64;
typedef unsigned int u32;
typedef int i32;

__declspec(dllimport) void *__cdecl memcpy(void *destination,
                                          const void *source, u64 count);
__declspec(dllimport) void *__cdecl memmove(void *destination,
                                           const void *source, u64 count);

typedef i32(__cdecl *PluginDataCallback2)(
    void *, const unsigned char *, const unsigned char *,
    const unsigned char *, const unsigned char *, i32, i32, i32, i32,
    const unsigned char *);

static const unsigned char kName[] =
    "AEXCompat PluginData CRT Memory Copy Fixture";
static const unsigned char kCategory[] =
    "AEXCompat Long Metadata Fixture Category";
static unsigned char gName[sizeof(kName)];
static unsigned char gCategory[sizeof(kCategory)];
static unsigned char gOverlap[] = "0123456789abcdef";
static volatile unsigned char gMemoryCopyObservation;

__declspec(dllexport) u64 __cdecl MemoryCopyEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)command;
  (void)in_data;
  (void)out_data;
  (void)params;
  (void)output;
  (void)extra;
  return gMemoryCopyObservation == 1 ? 0 : 4;
}

__declspec(dllexport) i32 __cdecl PluginDataEntryFunction2(
    void *context, PluginDataCallback2 callback, void *basic_suite,
    const char *host_name, const char *host_version) {
  const i32 effect_kind = 0x65464b54; /* MSVC 'eFKT' */
  (void)basic_suite;
  if (!callback || !host_name || !host_version || !gMemoryCopyObservation)
    return 3;
  return callback(context, gName,
                  (const unsigned char *)"fixture.crt-memory-copy", gCategory,
                  (const unsigned char *)"MemoryCopyEffect", effect_kind, 13,
                  29, 0, (const unsigned char *)0);
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  (void)module;
  (void)reserved;
  if (reason != 1)
    return 1;

  if (memcpy(gName, kName, sizeof(kName)) != gName ||
      memcpy(gCategory, kCategory, sizeof(kCategory)) != gCategory)
    return 0;
  if (memmove(gOverlap + 4, gOverlap, 12) != gOverlap + 4)
    return 0;
  if (gOverlap[0] != '0' || gOverlap[3] != '3' ||
      gOverlap[4] != '0' || gOverlap[15] != 'b')
    return 0;
  gMemoryCopyObservation = 1;
  return 1;
}
