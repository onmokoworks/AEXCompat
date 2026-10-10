#include "worker_pf_world_transform_runtime.hpp"
#include "worker_world_registry.hpp"
#include "trace_writer.hpp"
#include "gpu_memory_world_transport.hpp"

#include <array>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <chrono>
#include <cstring>
#include <cstdlib>
#include <iostream>
#include <thread>

namespace aexcompat::l2_detail { aexcompat::TraceWriter* g_trace_writer{}; }
// The standalone CPU test never bootstraps GPU transport. Fail loudly if this
// unused linkage boundary is reached; it must not become fake GPU evidence.
namespace aexcompat::gpu_runtime::memory_world_transport {
void configure_host_world_fallback(HostNewWorld, HostDisposeWorld, HostOwnsWorld,
                                   HostRecognizesSmartWorld) { std::abort(); }
}
namespace wr = aexcompat::world_registry;
namespace ws = aexcompat::world_safety;
namespace wt = aexcompat::pf_world_transform;
namespace {
const char* session = "argb8";
std::atomic<bool> pause_snapshot{}, snapshot_ready{}, resume_snapshot{};
ws::OwnedWorldResolution snapshot(const void* world, wr::AegpWorldSnapshot& out) {
  const auto result = wr::snapshot_owned_aegp_pf_world(world, out);
  if (pause_snapshot && result == ws::OwnedWorldResolution::resolved) {
    snapshot_ready = true;
    while (!resume_snapshot) std::this_thread::yield();
  }
  return result;
}
void configure(bool enabled = true, bool owned = true) {
  static uint32_t calls;
  static int32_t x, y;
  static uint8_t opacity;
  if (!enabled) { wt::configure({}); return; }
  wt::configure({{&ws::bounded_typed_world, &wr::resolve_dispatch_world_format,
                 +[]() -> const char* { return session; },
                 +[](const char* value) { session = value; return true; },
                 &ws::bounded_argb8_world, &wr::hosts_world_pixels,
                 owned ? &snapshot : nullptr},
                {&calls, &x, &y, &opacity}});
}
struct World {
  void** handle{};
  ws::LocalEffectWorld pf{};
  explicit World(int type) {
    if (wr::aegp_world_new_owned(0, type, 5, 3, &handle) ||
        wr::aegp_world_fill_pf_world(handle, &pf)) std::abort();
  }
  ~World() { if (handle) wr::aegp_world_dispose(handle); }
};
bool every(const World& w, int bytes, const void* expected) {
  for (int y = 0; y < w.pf.height; ++y)
    for (int x = 0; x < w.pf.width; ++x)
      if (std::memcmp(static_cast<const char*>(w.pf.data) + y*w.pf.rowbytes + x*bytes,
                      expected, bytes)) return false;
  return true;
}
bool matrix() {
  const uint8_t c8[]{255, 0, 255, 255};
  const uint16_t c16[]{32768, 0, 32768, 32768};
  const float cf[]{1, 0, 1, 1}, half[]{0.5f, 1, 1, 1}, hdr[]{1, 0.125f, 0.5f, 1.75f};
  const void* colors[]{c8,c16,cf};
  const uint8_t h8[]{255,32,128,255}, p8[]{128,128,128,128}, r8[]{128,255,255,255};
  const uint16_t h16[]{32768,4096,16384,32768}, p16[]{16384,16384,16384,16384},
                 r16[]{16384,32768,32768,32768};
  const float pf[]{.5f,.5f,.5f,.5f}, rf[]{.5f,1,1,1};
  const void* hdr_expected[]{h8,h16,hdr};
  const void* premult_expected[]{p8,p16,pf};
  const void* reverse_expected[]{r8,r16,rf};
  const auto fills = std::array{&wt::fill_world8,&wt::fill_world16,&wt::fill_world_float};
  const auto premults = std::array{&wt::premultiply_color8,&wt::premultiply_color16,
                                 &wt::premultiply_color_float};
  const uint8_t black8[4]{};
  const uint16_t black16[4]{};
  const float blackf[4]{};
  const void* mattes[]{black8,black16,blackf};
  for (const char* format : {"argb8","argb16","argb32f"}) {
    session = format;
    for (int type=1; type<=3; ++type) {
      const int bytes = type==1 ? 4 : type==2 ? 8 : 16;
      World a(type), b(type);
      auto copy = a.pf;
      for (int variant=0; variant<3; ++variant) {
        if (fills[variant](nullptr,colors[variant],nullptr,&copy) ||
            !every(a,bytes,colors[type-1])) return false;
        if (wt::fill_world_float(nullptr,half,nullptr,&copy) ||
            premults[variant](nullptr,&copy,mattes[variant],1,&b.pf) ||
            !every(b,bytes,premult_expected[type-1])) return false;
        auto alias = b.pf;
        if (premults[variant](nullptr,&b.pf,mattes[variant],0,&alias) ||
            !every(b,bytes,reverse_expected[type-1])) return false;
      }
      if (wt::fill_world_float(nullptr,hdr,nullptr,&copy) ||
          !every(a,bytes,hdr_expected[type-1])) return false;
      ws::DispatchWorldFormat unresolved;
      if (wr::resolve_dispatch_world_format(&copy,unresolved)) return false;
      auto altered_flags = copy;
      altered_flags.world_flags ^= 1;
      if (wt::fill_world_float(nullptr,hdr,nullptr,&altered_flags) ||
          !every(a,bytes,hdr_expected[type-1])) return false;
      for (const int offset : {1, bytes, a.pf.rowbytes*a.pf.height-1,
                               a.pf.rowbytes*a.pf.height}) {
        auto interior = copy;
        interior.data = static_cast<char*>(copy.data) + offset;
        wr::AegpWorldSnapshot refused;
        if (wr::snapshot_owned_aegp_pf_world(&interior,refused)!=
                ws::OwnedWorldResolution::rejected ||
            wt::fill_world_float(nullptr,hdr,nullptr,&interior)!=516 ||
            wt::premultiply_color_float(nullptr,&interior,blackf,1,&b.pf)!=516 ||
            !every(a,bytes,hdr_expected[type-1])) return false;
      }
      for (int field=0; field<3; ++field) {
        auto invalid = copy;
        if (field==0) --invalid.width;
        if (field==1) ++invalid.height;
        if (field==2) ++invalid.rowbytes;
        if (wt::fill_world8(nullptr,c8,nullptr,&invalid)!=516 ||
            wt::premultiply_color_float(nullptr,&invalid,blackf,1,&b.pf)!=516 ||
            !every(a,bytes,hdr_expected[type-1])) return false;
      }
      const LegacyRect outside{0,0,6,3};
      if (wt::fill_world8(nullptr,c8,&outside,&copy)!=516 ||
          !every(a,bytes,hdr_expected[type-1])) return false;
      if (wt::fill_world8(nullptr,nullptr,nullptr,&copy)) return false;
      const std::array<unsigned char,16> zero{};
      if (!every(a,bytes,zero.data())) return false;
      if (wt::fill_world_float(nullptr,half,nullptr,&copy) ||
          wt::premultiply_world8(nullptr,1,&copy) ||
          !every(a,bytes,premult_expected[type-1])) return false;
    }
  }
  return true;
}
bool negatives() {
  const float color[]{1,1,1,1}, black[4]{};
  for (int a=1; a<=3; ++a) for (int b=1; b<=3; ++b) {
    if (a==b) continue;
    World src(a), dst(b);
    if (wt::premultiply_color_float(nullptr,&src.pf,black,1,&dst.pf)!=516) return false;
    const std::array<unsigned char,16> zero{};
    if (!every(dst,b==1?4:b==2?8:16,zero.data())) return false;
  }
  World w(3);
  alignas(16) unsigned char foreign_pixels[5*3*4]{};
  ws::LocalEffectWorld foreign{};
  foreign.data=foreign_pixels; foreign.rowbytes=5*4;
  foreign.width=5; foreign.height=3;
  if (wt::premultiply_color_float(nullptr,&foreign,black,1,&w.pf)!=516 ||
      wt::premultiply_color_float(nullptr,&w.pf,black,1,&foreign)!=516 ||
      std::any_of(std::begin(foreign_pixels),std::end(foreign_pixels),
                  [](unsigned char value){ return value!=0; })) return false;
  {
    ws::DispatchWorldFormatScope scope;
    auto registered = foreign;
    if (!scope.register_world(&registered,wr::kPixelFormatArgb32)) return false;
    // A known registered reference retargeted to an otherwise-valid owned
    // backing must not acquire new authority through the callback-local hook.
    registered=w.pf;
    if (wt::fill_world_float(nullptr,color,nullptr,&registered)!=516) return false;
  }
  configure(false);
  if (wt::fill_world_float(nullptr,color,nullptr,&w.pf)!=516) return false;
  configure();
  wr::AegpWorldSnapshot pin;
  if (wr::snapshot_owned_aegp_pf_world(&w.pf,pin)!=ws::OwnedWorldResolution::resolved)
    return false;
  auto handle = w.handle;
  if (wr::aegp_world_dispose(handle)) return false;
  w.handle = nullptr;
  wr::AegpWorldSnapshot missing;
  int32_t type;
  if (wr::aegp_world_get_type(handle,&type)==0 || wr::aegp_world_dispose(handle)==0 ||
      wr::snapshot_owned_aegp_pf_world(&w.pf,missing)!=ws::OwnedWorldResolution::not_owned)
    return false;
  // A PF value carries no generation; raw stale-value fallback is not claimed
  // safe here. Only the opaque handle and the new owned lease are qualified.
  return true;
}
bool pf_new_world_controls() {
  const float color[]{1,0,1,1};
  const uint8_t c8[]{255,0,255,255};
  const uint16_t c16[]{32768,0,32768,32768};
  const void* expected[]{c8,c16,color};
  const int formats[]{wr::kPixelFormatArgb32,wr::kPixelFormatArgb64,wr::kPixelFormatArgb128};
  for (const char* format : {"argb8","argb16","argb32f"}) {
    session=format;
    for(int type=0;type<3;++type) {
      ws::LocalEffectWorld world{};
      if(wr::new_world(nullptr,5,3,1,formats[type],&world)) return false;
      struct Dispose { ws::LocalEffectWorld& world;
        ~Dispose(){ wr::dispose_world(nullptr,&world); } } cleanup{world};
      const int bytes=type==0?4:type==1?8:16;
      if(wt::fill_world_float(nullptr,color,nullptr,&world)) return false;
      for(int y=0;y<3;++y) for(int x=0;x<5;++x)
        if(std::memcmp(static_cast<char*>(world.data)+y*world.rowbytes+x*bytes,
                       expected[type],bytes)) return false;
    }
  }
  return true;
}
bool concurrency() {
  session = "argb8";
  const float color[]{1,.125f,.5f,1.75f}, black[4]{};
  {
    World w(3);
    wr::AegpWorldSnapshot retained;
    if (!wr::snapshot_aegp_world(w.handle,retained)) return false;
    snapshot_ready=false; resume_snapshot=false; pause_snapshot=true;
    int result=-1;
    std::atomic<bool> done{false};
    std::unique_lock<std::mutex> pixel_lock(retained.backing_pin->pixels_mutex);
    std::thread fill([&]{ result=wt::fill_world_float(nullptr,color,nullptr,&w.pf); done=true; });
    while (!snapshot_ready) std::this_thread::yield();
    const auto disposed=wr::aegp_world_dispose(w.handle);
    w.handle=nullptr;
    pause_snapshot=false; resume_snapshot=true;
    std::this_thread::sleep_for(std::chrono::milliseconds(25));
    const bool respected_lock = !done.load();
    pixel_lock.unlock();
    fill.join();
    if (disposed || !respected_lock || result || !every(w,16,color)) return false;
  }
  World a(3), b(3);
  if (wt::fill_world_float(nullptr,color,nullptr,&a.pf) ||
      wt::fill_world_float(nullptr,color,nullptr,&b.pf)) return false;
  std::atomic<bool> ok{true};
  std::thread forward([&]{ for(int i=0;i<20;++i)
    if(wt::premultiply_color_float(nullptr,&a.pf,black,1,&b.pf)) ok=false; });
  std::thread reverse([&]{ for(int i=0;i<20;++i)
    if(wt::premultiply_color_float(nullptr,&b.pf,black,1,&a.pf)) ok=false; });
  forward.join(); reverse.join();
  std::thread fill([&]{ for(int i=0;i<20;++i)
    if(wt::fill_world_float(nullptr,color,nullptr,&a.pf)) ok=false; });
  std::thread blur([&]{ for(int i=0;i<20;++i)
    if(wr::aegp_world_fast_blur(1,0,1,a.handle)) ok=false; });
  fill.join(); blur.join();
  // A uniform world is invariant under blur; a final serialized fill gives
  // an exact oracle independent of scheduling and blur's float rounding.
  return ok && !wt::fill_world_float(nullptr,color,nullptr,&a.pf) && every(a,16,color);
}
}
int main(int argc, char** argv) {
  const bool mutation = argc==2 && std::strcmp(argv[1],"--without-owned-resolution")==0;
  configure(true,!mutation);
  if (mutation && wt::configured()) return 5;
  if (!matrix()) return 1;
  if (!negatives()) return 2;
  if (!concurrency()) return 3;
  if (!pf_new_world_controls()) return 6;
  if (!wr::aegp_lifetimes_balanced() || !wr::lifetimes_balanced()) return 4;
  std::cout << "{\"aegp_pf_matte\":\"passed\"}\n";
}
