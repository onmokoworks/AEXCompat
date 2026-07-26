/*
 * Header-free Win64 PluginData fixture for the Apple Silicon guest worker.
 *
 * Build with MSVC:
 *   cl /nologo /c /O2 /GS- /Zl plugin_data_multi.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain
 *        /OUT:plugin_data_multi.aex plugin_data_multi.obj
 */

typedef unsigned long long u64;
typedef unsigned int u32;
typedef int i32;

static void set_out_flags(u64 command, u64 out_data, u32 flags) {
  /* PF_Cmd_GLOBAL_SETUP and PF_OutData::out_flags in the validated SDK ABI. */
  if (command == 1 && out_data)
    *(u32 *)(out_data + 96) = flags;
}

typedef i32(__cdecl *PluginDataCallback2)(
    void *, const unsigned char *, const unsigned char *,
    const unsigned char *, const unsigned char *, i32, i32, i32, i32,
    const unsigned char *);

__declspec(dllexport) u64 __cdecl FirstEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)in_data;
  (void)params;
  (void)output;
  (void)extra;
  set_out_flags(command, out_data, 0x100);
  return 0;
}

__declspec(dllexport) u64 __cdecl SecondEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)in_data;
  (void)params;
  (void)output;
  (void)extra;
  set_out_flags(command, out_data, 0x200);
  return 0;
}

__declspec(dllexport) i32 __cdecl PluginDataEntryFunction2(
    void *context, PluginDataCallback2 callback, void *basic_suite,
    const char *host_name, const char *host_version) {
  const i32 effect_kind = 0x65464b54; /* MSVC 'eFKT' */
  (void)basic_suite;
  if (!callback || !host_name || !host_version) return 3;
  if (callback(context, (const unsigned char *)"Fixture First",
               (const unsigned char *)"fixture.first",
               (const unsigned char *)"AEXCompat Tests",
               (const unsigned char *)"FirstEffect", effect_kind, 13, 29, 0,
               (const unsigned char *)"https://example.invalid/first") != 0)
    return 3;
  return callback(context, (const unsigned char *)"Fixture Second",
                  (const unsigned char *)"fixture.second",
                  (const unsigned char *)"AEXCompat Tests",
                  (const unsigned char *)"SecondEffect", effect_kind, 13, 29, 9,
                  (const unsigned char *)"https://example.invalid/second");
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  (void)module;
  (void)reason;
  (void)reserved;
  return 1;
}
