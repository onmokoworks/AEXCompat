/*
 * Header-free Win64 PF ANSI Suite v2 fixture.
 *
 * Build with MSVC:
 *   cl /nologo /c /O2 /GS- /Zl pf_ansi_v2.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain
 *        /OUT:pf_ansi_v2.aex pf_ansi_v2.obj
 */

typedef unsigned long long u64;
typedef int i32;

int _fltused;

typedef i32(__cdecl *AcquireSuite)(const char *, i32, const void **);
typedef double(__cdecl *AnsiUnary)(double);
typedef char *(__cdecl *AnsiStrcpy)(char *, const char *);
typedef i32(__cdecl *AnsiStrcpyBounded)(char *, u64, const char *);
typedef i32(__cdecl *PluginDataCallback2)(
    void *, const unsigned char *, const unsigned char *,
    const unsigned char *, const unsigned char *, i32, i32, i32, i32,
    const unsigned char *);

static volatile i32 gAnsiObservation;

__declspec(dllexport) u64 __cdecl AnsiEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  void **basic_suite;
  AcquireSuite acquire_suite;
  void **ansi;
  AnsiUnary sine;
  AnsiStrcpy copy;
  AnsiStrcpyBounded bounded_copy;
  char copied[16];
  char truncated[8];
  double sine_value;
  (void)out_data;
  (void)params;
  (void)output;
  (void)extra;
  if (command != 1)
    return gAnsiObservation == 1 ? 0 : 4;
  if (!in_data)
    return 4;

  /* PF_InData::pica_basicP and SPBasicSuite::AcquireSuite. */
  basic_suite = *(void ***)(in_data + 384);
  if (!basic_suite || !basic_suite[0])
    return 4;
  acquire_suite = (AcquireSuite)basic_suite[0];
  ansi = (void **)0;
  if (acquire_suite("PF ANSI Suite", 2, (const void **)&ansi) != 0 || !ansi)
    return 4;
  if (ansi[19] != (void *)0 || !ansi[12] || !ansi[16] || !ansi[20])
    return 4;

  sine = (AnsiUnary)ansi[12];
  copy = (AnsiStrcpy)ansi[16];
  bounded_copy = (AnsiStrcpyBounded)ansi[20];
  sine_value = sine(0.5);
  if (sine_value < 0.4794255386042029 || sine_value > 0.4794255386042031)
    return 4;
  if (copy(copied, "classic strcpy") != copied || copied[3] != 's')
    return 4;
  if (bounded_copy(truncated, sizeof(truncated), "bounded metadata") != 0)
    return 4;
  if (truncated[0] != 'b' || truncated[6] != 'd' || truncated[7] != '\0')
    return 4;
  gAnsiObservation = 1;
  return 0;
}

__declspec(dllexport) i32 __cdecl PluginDataEntryFunction2(
    void *context, PluginDataCallback2 callback, void *basic_suite,
    const char *host_name, const char *host_version) {
  const i32 effect_kind = 0x65464b54; /* MSVC 'eFKT' */
  (void)basic_suite;
  if (!callback || !host_name || !host_version)
    return 3;
  return callback(context, (const unsigned char *)"PF ANSI v2 Fixture",
                  (const unsigned char *)"fixture.pf-ansi-v2",
                  (const unsigned char *)"AEXCompat Tests",
                  (const unsigned char *)"AnsiEffect", effect_kind, 13, 29, 0,
                  (const unsigned char *)0);
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  (void)module;
  (void)reason;
  (void)reserved;
  return 1;
}
