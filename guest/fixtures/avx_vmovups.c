/*
 * Header-only Win64 AVX fixture for the Apple Silicon guest worker.
 *
 * Build from an MSVC x64 developer shell:
 *   cl /nologo /c /O2 /GS- /Zl /arch:AVX2 avx_vmovups.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain
 *        /OUT:avx_vmovups.aex avx_vmovups.obj
 */

#include <immintrin.h>

typedef unsigned long long u64;

__declspec(align(32)) static float g_source[8] = {
    1.0f, -2.0f, 3.5f, 4.25f, -5.0f, 6.75f, 7.0f, -8.5f,
};
__declspec(align(32)) static float g_destination[8];
static volatile unsigned int g_observation;

__declspec(dllexport) u64 __cdecl EffectMain(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)command;
  (void)in_data;
  (void)out_data;
  (void)params;
  (void)output;
  (void)extra;
  return g_observation == 0x40600000U ? 0 : 4;
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  __m256 value;
  (void)module;
  (void)reserved;
  if (reason != 1)
    return 1;

  value = _mm256_loadu_ps(g_source);
  _mm256_storeu_ps(g_destination, value);
  g_observation = ((unsigned int *)g_destination)[2];
  return 1;
}
