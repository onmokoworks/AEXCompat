/*
 * Header-free Win64 CRT heap fixture for the Apple Silicon guest worker.
 *
 * Build with MSVC:
 *   cl /nologo /c /O2 /GS- /Zl crt_heap.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain
 *        /OUT:crt_heap.aex crt_heap.obj ucrt.lib vcruntime.lib
 */

typedef unsigned long long u64;

__declspec(dllimport) void *__cdecl malloc(u64 size);
__declspec(dllimport) void *__cdecl calloc(u64 count, u64 element_size);
__declspec(dllimport) void __cdecl free(void *pointer);
__declspec(dllimport) int __cdecl _callnewh(u64 size);

static volatile unsigned char g_heap_observation;

__declspec(dllexport) u64 __cdecl EffectMain(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)command;
  (void)in_data;
  (void)out_data;
  (void)params;
  (void)output;
  (void)extra;
  return g_heap_observation == 0x5a ? 0 : 4;
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  unsigned char *first;
  unsigned char *zeroed;
  u64 index;
  (void)module;
  (void)reserved;
  if (reason != 1)
    return 1;

  first = (unsigned char *)malloc(31);
  zeroed = (unsigned char *)calloc(8, 4);
  if (!first || !zeroed || ((u64)first & 15) != 0 ||
      ((u64)zeroed & 15) != 0)
    return 0;
  for (index = 0; index < 32; ++index)
    if (zeroed[index] != 0)
      return 0;

  first[0] = 0x5a;
  zeroed[31] = first[0];
  g_heap_observation = zeroed[31];
  free((void *)0);
  free(first);
  free(zeroed);
  return _callnewh(64) == 0;
}
