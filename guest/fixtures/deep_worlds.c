/*
 * Header-free Win64 fixture for Classic and SmartFX ARGB8/16/32F worlds.
 *
 * Build with MSVC:
 *   cl /nologo /c /O2 /GS- /Zl deep_worlds.c
 *   link /nologo /DLL /NODEFAULTLIB /ENTRY:DllMain /OUT:deep_worlds.aex deep_worlds.obj
 */

typedef unsigned char u8;
typedef unsigned int u32;
typedef unsigned long long u64;
typedef int i32;

enum {
  CMD_GLOBAL_SETUP = 1,
  CMD_PARAMS_SETUP = 4,
  CMD_RENDER = 11,
  CMD_SMART_PRE_RENDER = 23,
  CMD_SMART_RENDER = 24,
  OUT_FLAGS = 96,
  OUT_NUM_PARAMS = 48,
  OUT_FLAGS2 = 400,
  PARAM_U = 56,
  WORLD_DATA = 24,
  WORLD_ROWBYTES = 32,
  WORLD_WIDTH = 36,
  WORLD_HEIGHT = 40
};

static i32 g_smart_bit_depth;

typedef i32(__cdecl *PluginDataCallback2)(
    void *, const unsigned char *, const unsigned char *,
    const unsigned char *, const unsigned char *, i32, i32, i32, i32,
    const unsigned char *);

typedef i32(__cdecl *CheckoutLayerPixels)(void *, i32, void **);
typedef i32(__cdecl *CheckinLayerPixels)(void *, i32);
typedef i32(__cdecl *CheckoutOutput)(void *, void **);

static u64 read_u64(const void *base, u32 offset) {
  return *(const u64 *)((const u8 *)base + offset);
}

static i32 read_i32(const void *base, u32 offset) {
  return *(const i32 *)((const u8 *)base + offset);
}

static void write_i32(void *base, u32 offset, i32 value) {
  *(i32 *)((u8 *)base + offset) = value;
}

static i32 world_layout_valid(const void *world, i32 bit_depth) {
  i32 flags = read_i32(world, 16);
  i32 rowbytes = read_i32(world, WORLD_ROWBYTES);
  i32 width = read_i32(world, WORLD_WIDTH);
  i32 expected_pixel_bytes =
      bit_depth == 8 ? 4 : (bit_depth == 16 ? 8 : (bit_depth == 32 ? 16 : 0));
  return width > 0 && expected_pixel_bytes != 0 &&
         rowbytes == width * expected_pixel_bytes &&
         flags == (bit_depth == 8 ? 0 : 1);
}

static void copy_world(const void *input, void *output) {
  const u8 *source = (const u8 *)(u64)read_u64(input, WORLD_DATA);
  u8 *destination = (u8 *)(u64)read_u64(output, WORLD_DATA);
  i32 source_rowbytes = read_i32(input, WORLD_ROWBYTES);
  i32 output_rowbytes = read_i32(output, WORLD_ROWBYTES);
  i32 height = read_i32(output, WORLD_HEIGHT);
  i32 rowbytes = source_rowbytes < output_rowbytes ? source_rowbytes : output_rowbytes;
  i32 y;
  i32 x;
  if (!source || !destination || rowbytes <= 0 || height <= 0)
    return;
  for (y = 0; y < height; ++y)
    for (x = 0; x < rowbytes; ++x)
      destination[y * output_rowbytes + x] = source[y * source_rowbytes + x];
}

static u64 effect_common(u64 command, u64 out_data, int smart) {
  if (command == CMD_GLOBAL_SETUP) {
    write_i32((void *)out_data, OUT_FLAGS, 1 << 25);
    write_i32((void *)out_data, OUT_FLAGS2,
              (1 << 12) | (smart ? (1 << 10) : 0));
  } else if (command == CMD_PARAMS_SETUP) {
    write_i32((void *)out_data, OUT_NUM_PARAMS, 0);
  }
  return 0;
}

__declspec(dllexport) u64 __cdecl ClassicEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)in_data;
  (void)extra;
  effect_common(command, out_data, 0);
  if (command == CMD_RENDER && params && output) {
    void *input_param = (void *)(u64)(*(u64 *)(u64)params);
    void *input_world = (u8 *)input_param + PARAM_U;
    i32 width = read_i32(input_world, WORLD_WIDTH);
    i32 rowbytes = read_i32(input_world, WORLD_ROWBYTES);
    i32 pixel_bytes = width > 0 ? rowbytes / width : 0;
    i32 bit_depth = pixel_bytes == 4 ? 8 : (pixel_bytes == 8 ? 16 : 32);
    if (!world_layout_valid(input_world, bit_depth) ||
        !world_layout_valid((void *)(u64)output, bit_depth))
      return 4;
    copy_world(input_world, (void *)(u64)output);
  }
  return 0;
}

__declspec(dllexport) u64 __cdecl EightBitEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)in_data;
  (void)extra;
  if (command == CMD_PARAMS_SETUP)
    write_i32((void *)out_data, OUT_NUM_PARAMS, 0);
  if (command == CMD_RENDER && params && output) {
    void *input_param = (void *)(u64)(*(u64 *)(u64)params);
    copy_world((u8 *)input_param + PARAM_U, (void *)(u64)output);
  }
  return 0;
}

__declspec(dllexport) u64 __cdecl SmartEffect(
    u64 command, u64 in_data, u64 out_data, u64 params, u64 output, u64 extra) {
  (void)in_data;
  (void)params;
  (void)output;
  effect_common(command, out_data, 1);
  if (command == CMD_SMART_PRE_RENDER && extra) {
    void *pre_input = (void *)(u64)read_u64((void *)(u64)extra, 0);
    void *pre_output = (void *)(u64)read_u64((void *)(u64)extra, 8);
    i32 index;
    if (!pre_input || !pre_output)
      return 4;
    g_smart_bit_depth = *(const short *)((const u8 *)pre_input + 44);
    for (index = 0; index < 4; ++index) {
      i32 value = read_i32(pre_input, (u32)index * 4);
      write_i32(pre_output, (u32)index * 4, value);
      write_i32(pre_output, 16 + (u32)index * 4, value);
    }
  } else if (command == CMD_SMART_RENDER && extra) {
    void *callbacks = (void *)(u64)read_u64((void *)(u64)extra, 8);
    CheckoutLayerPixels checkout_layer;
    CheckinLayerPixels checkin_layer;
    CheckoutOutput checkout_output;
    void *input_world = 0;
    void *output_world = 0;
    i32 error;
    if (!callbacks)
      return 4;
    checkout_layer = (CheckoutLayerPixels)(u64)read_u64(callbacks, 0);
    checkin_layer = (CheckinLayerPixels)(u64)read_u64(callbacks, 8);
    checkout_output = (CheckoutOutput)(u64)read_u64(callbacks, 16);
    if (!checkout_layer || !checkin_layer || !checkout_output)
      return 4;
    error = checkout_layer(0, 0, &input_world);
    if (!error)
      error = checkout_output(0, &output_world);
    if (!error &&
        (!world_layout_valid(input_world, g_smart_bit_depth) ||
         !world_layout_valid(output_world, g_smart_bit_depth)))
      error = 4;
    if (!error)
      copy_world(input_world, output_world);
    if (checkin_layer(0, 0) != 0 && !error)
      error = 4;
    return (u64)(u32)error;
  }
  return 0;
}

__declspec(dllexport) i32 __cdecl PluginDataEntryFunction2(
    void *context, PluginDataCallback2 callback, void *basic_suite,
    const char *host_name, const char *host_version) {
  const i32 effect_kind = 0x65464b54;
  (void)basic_suite;
  if (!callback || !host_name || !host_version)
    return 3;
  if (callback(context, (const unsigned char *)"Deep Classic",
               (const unsigned char *)"fixture.deep.classic",
               (const unsigned char *)"AEXCompat Tests",
               (const unsigned char *)"ClassicEffect", effect_kind, 13, 29, 0,
               (const unsigned char *)"https://example.invalid/classic") != 0)
    return 3;
  if (callback(context, (const unsigned char *)"Eight Bit Only",
               (const unsigned char *)"fixture.deep.8only",
               (const unsigned char *)"AEXCompat Tests",
               (const unsigned char *)"EightBitEffect", effect_kind, 13, 29, 0,
               (const unsigned char *)"https://example.invalid/8only") != 0)
    return 3;
  return callback(context, (const unsigned char *)"Deep Smart",
                  (const unsigned char *)"fixture.deep.smart",
                  (const unsigned char *)"AEXCompat Tests",
                  (const unsigned char *)"SmartEffect", effect_kind, 13, 29, 0,
                  (const unsigned char *)"https://example.invalid/smart");
}

__declspec(dllexport) int __stdcall DllMain(void *module, unsigned long reason,
                                           void *reserved) {
  (void)module;
  (void)reason;
  (void)reserved;
  return 1;
}
