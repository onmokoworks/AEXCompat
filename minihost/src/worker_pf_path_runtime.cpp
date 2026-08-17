#include "worker_pf_path_runtime.hpp"
#include "worker_extended_diag.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <limits>
#include <list>
#include <mutex>
#include <new>
#include <string>
#include <unordered_map>

namespace aexcompat::pf_path_runtime {
namespace {
constexpr int32_t kBad = 516;
constexpr int32_t kOom = 4;
constexpr std::size_t kMaxPreps = 256;
using Point = std::array<double, 2>;
using Cubic = std::array<Point, 4>;

struct Prep {
  void* path{};
  int32_t segment{};
  Cubic controls{};
  std::vector<double> parameters;
  std::vector<Point> points;
  std::vector<double> lengths;
};

HostHooks g_hooks;
std::unordered_map<void*, uint32_t> g_checkouts;
std::list<Prep> g_preps;
std::mutex g_mutex;
Snapshot g_report;

std::vector<PathInfo> paths() { return g_hooks.enumerate ? g_hooks.enumerate() : std::vector<PathInfo>{}; }
PathInfo* find(std::vector<PathInfo>& values, int32_t id) {
  const auto it = std::find_if(values.begin(), values.end(), [id](const auto& p) { return p.id == id; });
  return it == values.end() ? nullptr : &*it;
}
bool checked(void* path, mask_runtime::CurveSnapshot& curve) {
  return path && g_checkouts.find(path) != g_checkouts.end() && g_hooks.snapshot &&
      g_hooks.snapshot(path, curve);
}
std::size_t vertices(const mask_runtime::CurveSnapshot& p) {
  return p.vertices.size() - static_cast<std::size_t>(!p.open && !p.vertices.empty());
}
Point eval(const Cubic& c, double t) {
  const auto lerp = [t](const Point& a, const Point& b) {
    return Point{a[0] + (b[0] - a[0]) * t,
                 a[1] + (b[1] - a[1]) * t};
  };
  const auto a = lerp(c[0], c[1]);
  const auto b = lerp(c[1], c[2]);
  const auto d = lerp(c[2], c[3]);
  return lerp(lerp(a, b), lerp(b, d));
}
Point deriv(const Cubic& c, double t) {
  const double u = 1.0 - t;
  return {3*u*u*(c[1][0]-c[0][0])+6*u*t*(c[2][0]-c[1][0])+3*t*t*(c[3][0]-c[2][0]),
          3*u*u*(c[1][1]-c[0][1])+6*u*t*(c[2][1]-c[1][1])+3*t*t*(c[3][1]-c[2][1])};
}
double distance(const Point& a, const Point& b) { return std::hypot(a[0]-b[0], a[1]-b[1]); }
double flatness(const Cubic& c) {
  const double chord = distance(c[0], c[3]);
  return distance(c[0], c[1]) + distance(c[1], c[2]) + distance(c[2], c[3]) - chord;
}
Cubic half(const Cubic& c, bool right) {
  const Point a{(c[0][0]+c[1][0])/2,(c[0][1]+c[1][1])/2};
  const Point b{(c[1][0]+c[2][0])/2,(c[1][1]+c[2][1])/2};
  const Point d{(c[2][0]+c[3][0])/2,(c[2][1]+c[3][1])/2};
  const Point e{(a[0]+b[0])/2,(a[1]+b[1])/2};
  const Point f{(b[0]+d[0])/2,(b[1]+d[1])/2};
  const Point m{(e[0]+f[0])/2,(e[1]+f[1])/2};
  return right ? Cubic{m,f,d,c[3]} : Cubic{c[0],a,e,m};
}
void append(const Cubic& c, double begin, double end, double tolerance, int depth,
            std::vector<double>& parameters, std::vector<Point>& points) {
  if (depth >= 20 || points.size() >= 65537 || flatness(c) <= tolerance) {
    parameters.push_back(end); points.push_back(c[3]); return;
  }
  const double mid=(begin+end)/2;
  append(half(c,false),begin,mid,tolerance*0.5,depth+1,parameters,points);
  append(half(c,true),mid,end,tolerance*0.5,depth+1,parameters,points);
}
bool populate(Prep& prep, void* path, int32_t segment, int32_t frequency) {
  mask_runtime::CurveSnapshot curve;
  if (!checked(path, curve) || segment < 0 || frequency < 1 || frequency > 1024) return false;
  const auto count=vertices(curve);
  const auto segments=static_cast<int32_t>(curve.open && count ? count-1 : count);
  if (segment >= segments) return false;
  const auto& a=curve.vertices[segment]; const auto& b=curve.vertices[(segment+1)%count];
  prep.path=path; prep.segment=segment;
  prep.controls={Point{a.x,a.y},Point{a.x+a.tangent_out_x,a.y+a.tangent_out_y},
                 Point{b.x+b.tangent_in_x,b.y+b.tangent_in_y},Point{b.x,b.y}};
  for (const auto& p:prep.controls) if(!std::isfinite(p[0])||!std::isfinite(p[1])) return false;
  const double polygon=distance(prep.controls[0],prep.controls[1])+distance(prep.controls[1],prep.controls[2])+distance(prep.controls[2],prep.controls[3]);
  const double tolerance=std::max(1e-9,std::max(1.0,polygon)*1e-7/std::sqrt(static_cast<double>(frequency)));
  prep.parameters={0}; prep.points={prep.controls[0]};
  append(prep.controls,0,1,tolerance,0,prep.parameters,prep.points);
  prep.lengths={0};
  for(size_t i=1;i<prep.points.size();++i) prep.lengths.push_back(prep.lengths.back()+distance(prep.points[i-1],prep.points[i]));
  return prep.points.size()>=2 && std::isfinite(prep.lengths.back());
}
Prep* registered(void** handle, void* path, int32_t segment, bool require_checkout) {
  if(!handle||!*handle||!path) return nullptr;
  auto* candidate=static_cast<Prep*>(*handle);
  auto it=std::find_if(g_preps.begin(),g_preps.end(),[candidate](auto& p){return &p==candidate;});
  if(it==g_preps.end()||it->path!=path||it->segment!=segment||
     (require_checkout&&g_checkouts.find(path)==g_checkouts.end())) return nullptr;
  return &*it;
}
bool evaluate(Prep& prep,double length,double* x,double* y,double* dx,double* dy){
  if(!x||!y||!std::isfinite(length)||prep.lengths.empty()||length<0||length>prep.lengths.back()) return false;
  auto upper=std::lower_bound(prep.lengths.begin(),prep.lengths.end(),length);
  size_t r=static_cast<size_t>(upper-prep.lengths.begin()); if(r==0)r=1; if(r>=prep.points.size())r=prep.points.size()-1;
  const size_t l=r-1; const double span=prep.lengths[r]-prep.lengths[l];
  const double t=prep.parameters[l]+(prep.parameters[r]-prep.parameters[l])*(span==0?0:(length-prep.lengths[l])/span);
  const auto p=eval(prep.controls,t); *x=p[0]; *y=p[1];
  if(dx&&dy){auto d=deriv(prep.controls,t); double m=std::hypot(d[0],d[1]);*dx=m?d[0]/m:0;*dy=m?d[1]/m:0;}
  return std::isfinite(*x)&&std::isfinite(*y)&&(!dx||(std::isfinite(*dx)&&std::isfinite(*dy)));
}
struct RasterPoint{double x,y;};
bool flatten(const mask_runtime::CurveSnapshot& curve,int quality,std::vector<RasterPoint>& out){
  const auto count=vertices(curve); if(curve.open||count<3||count>64)return false;
  const int samples=quality?16:8;
  for(size_t s=0;s<count;++s){const auto&a=curve.vertices[s];const auto&b=curve.vertices[(s+1)%count];Cubic c={Point{a.x,a.y},Point{a.x+a.tangent_out_x,a.y+a.tangent_out_y},Point{b.x+b.tangent_in_x,b.y+b.tangent_in_y},Point{b.x,b.y}};for(int i=0;i<samples;++i){auto p=eval(c,static_cast<double>(i)/samples);out.push_back({p[0],p[1]});}}
  return out.size()>=3 && out.size()<=64*16;
}
bool inside(const std::vector<RasterPoint>& p,double x,double y){bool value=false;for(size_t i=0,j=p.size()-1;i<p.size();j=i++){if(((p[i].y>y)!=(p[j].y>y))&&(x<(p[j].x-p[i].x)*(y-p[i].y)/(p[j].y-p[i].y)+p[i].x))value=!value;}return value;}
double edge_distance(const std::vector<RasterPoint>& p,double x,double y,double fx,double fy){fx=std::max(fx,1e-6);fy=std::max(fy,1e-6);double best=std::numeric_limits<double>::infinity();for(size_t i=0,j=p.size()-1;i<p.size();j=i++){double ax=p[j].x/fx,ay=p[j].y/fy,bx=p[i].x/fx,by=p[i].y/fy,px=x/fx,py=y/fy,dx=bx-ax,dy=by-ay,l=dx*dx+dy*dy,t=l==0?0:std::clamp(((px-ax)*dx+(py-ay)*dy)/l,0.0,1.0);best=std::min(best,std::hypot(px-(ax+t*dx),py-(ay+t*dy)));}return best;}
// AEXCOMPAT_EXTENDED_DIAG trace of the enumeration/checkout entry points, so a
// plug-in that acquires the suite and then fails without any visible host
// refusal (issue #1253: Scribble / Inner-Outer-Key / Reshape_New answered
// PF_Err_OUT_OF_MEMORY with no `-> 4` in the trace) shows whether it asked the
// host how many paths there are, and what it was told. Off by default.
void diag_paths(const char* name, void* effect, int32_t index_or_id, int32_t value,
                int32_t result) {
  if (!aexcompat::l2_detail::extended_diag_enabled()) return;
  std::cerr << "extended_diag:path_" << name << " effect=" << effect
            << " arg=" << index_or_id << " value=" << value << " -> " << result
            << "\n" << std::flush;
}
bool supported_world_view(const WorldView& view) {
  return (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb32 &&
          view.pixel_bytes == 4) ||
      (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb64 &&
       view.pixel_bytes == 8) ||
      (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb128 &&
       view.pixel_bytes == 16);
}
} // namespace

void configure(HostHooks hooks){g_hooks=hooks;}
HostHooks host_hooks(){return g_hooks;}
void reset(){std::lock_guard lock(g_mutex);g_checkouts.clear();g_preps.clear();g_report={};}
Snapshot snapshot(){std::lock_guard lock(g_mutex);auto result=g_report;result.live_preps=static_cast<uint32_t>(g_preps.size());return result;}
bool lifetimes_balanced(){std::lock_guard lock(g_mutex);return g_report.checkout_calls==g_report.checkin_calls&&g_checkouts.empty()&&g_report.preps_created==g_report.preps_disposed&&g_preps.empty();}
int32_t __cdecl num_paths(void* effect,int32_t* count){auto p=paths();if(!effect||!count){diag_paths("num_paths",effect,-1,-1,4);return 4;}*count=static_cast<int32_t>(p.size());diag_paths("num_paths",effect,-1,*count,0);return 0;}
int32_t __cdecl path_info(void* effect,int32_t index,int32_t* id){auto p=paths();if(!effect||!id||index<0||static_cast<size_t>(index)>=p.size()){diag_paths("info",effect,index,-1,4);return 4;}*id=p[index].id;diag_paths("info",effect,index,*id,0);return 0;}
// PF_CheckoutPath / PF_CheckinPath.
//
// A unique_id that names no path on the layer is not a bad call: the SDK
// contract (AE_EffectSuites.h, PF_PathQuerySuite1) is that PF_CheckoutPath
// "can return NULL ptr if path doesn't exist", and PF_PathDef documents that a
// path param's path_id is PF_PathID_NONE (0) when no mask is chosen and may
// name a deleted mask otherwise. AE-shipped effects lean on that: Scribble
// checks out its "Mask" param's path_id unconditionally (0 with no mask) and
// throws its "Not enough memory to execute Scribble" AbortException on any
// non-zero PF_CheckoutPath result, then tests the pointer for NULL to mean
// "no mask" (Scribble.aex FUN_18006ccb0 / FUN_18006d280, issue #1253);
// Inner-Outer-Key and Reshape check the returned pointer the same way. So an
// absent path answers PF_Err_NONE with *pathPP = NULL, and the matching
// PF_CheckinPath of that NULL pointer for the same absent id answers
// PF_Err_NONE too. Everything that does name host state stays fail-closed: a
// non-NULL pointer that is not the live checkout of that id, a checkin of NULL
// for an id that does resolve to a path, a checkout with a null out pointer,
// a non-positive time step or a zero time scale are still rejected and
// counted, exactly as before.
int32_t __cdecl checkout_path(void* effect,int32_t id,int32_t,int32_t step,uint32_t scale,void** out){
  auto p=paths();auto* found=find(p,id);std::lock_guard lock(g_mutex);
  // No enumeration hook is the ordinary state of a render without a host mask
  // context (`configure_mask_scene` runs only for a mask trailer or the
  // sequence-data routes), i.e. a layer with no masks; it is not a
  // misconfiguration and answers like any absent path.
  if(!effect||!out||step<=0||!scale){++g_report.invalid_operations;diag_paths("checkout",effect,id,step,4);return 4;}
  if(!found){*out=nullptr;++g_report.absent_checkouts;diag_paths("checkout_absent",effect,id,step,0);return 0;}
  ++g_checkouts[found->handle];++g_report.checkout_calls;*out=found->handle;diag_paths("checkout",effect,id,step,0);return 0;}
int32_t __cdecl checkin_path(void* effect,int32_t id,int32_t changed,void* path){
  std::lock_guard lock(g_mutex);auto it=g_checkouts.find(path);auto p=paths();auto* found=find(p,id);
  if(effect&&!changed&&!found&&!path){++g_report.absent_checkins;diag_paths("checkin_absent",effect,id,changed,0);return 0;}
  if(!effect||changed||!found||found->handle!=path||it==g_checkouts.end()||!it->second){++g_report.invalid_operations;diag_paths("checkin",effect,id,changed,4);return 4;}
  if(!--it->second)g_checkouts.erase(it);++g_report.checkin_calls;diag_paths("checkin",effect,id,changed,0);return 0;}
int32_t __cdecl path_is_open(void* effect,void* path,int8_t* open){std::lock_guard lock(g_mutex);mask_runtime::CurveSnapshot c;if(!effect||!open||!checked(path,c))return 4;*open=c.open?1:0;return 0;}
int32_t __cdecl path_num_segments(void* effect,void* path,int32_t* count){std::lock_guard lock(g_mutex);mask_runtime::CurveSnapshot c;if(!effect||!count||!checked(path,c))return 4;auto n=vertices(c);*count=static_cast<int32_t>(c.open&&n?n-1:n);return 0;}
int32_t __cdecl path_vertex_info(void* effect,void* path,int32_t index,PathVertex* out){std::lock_guard lock(g_mutex);mask_runtime::CurveSnapshot c;if(!effect||!out||index<0||!checked(path,c)||static_cast<size_t>(index)>=c.vertices.size())return 4;auto&v=c.vertices[index];*out={v.x,v.y,v.tangent_in_x,v.tangent_in_y,v.tangent_out_x,v.tangent_out_y};return 0;}
int32_t __cdecl path_prepare_seg_length(void* effect,void* path,int32_t segment,int32_t frequency,void** out){if(!effect||!out||*out){std::lock_guard lock(g_mutex);++g_report.invalid_operations;return kBad;}try{std::lock_guard lock(g_mutex);Prep p;if(!populate(p,path,segment,frequency)){++g_report.invalid_operations;return kBad;}if(g_preps.size()>=kMaxPreps)return kOom;g_preps.push_back(std::move(p));*out=&g_preps.back();++g_report.preps_created;return 0;}catch(const std::bad_alloc&){return kOom;}catch(...){std::lock_guard lock(g_mutex);++g_report.invalid_operations;return kBad;}}
int32_t __cdecl path_get_seg_length(void* effect,void* path,int32_t segment,void** prep,double* length){std::lock_guard lock(g_mutex);auto*p=registered(prep,path,segment,true);if(!effect||!length||!p||p->lengths.empty())return kBad;*length=p->lengths.back();return 0;}
int32_t __cdecl path_eval_seg_length(void* effect,void* path,void** prep,int32_t segment,double length,double*x,double*y){std::lock_guard lock(g_mutex);auto*p=registered(prep,path,segment,true);return effect&&p&&evaluate(*p,length,x,y,nullptr,nullptr)?0:kBad;}
int32_t __cdecl path_eval_seg_length_deriv1(void* effect,void* path,void** prep,int32_t segment,double length,double*x,double*y,double*dx,double*dy){std::lock_guard lock(g_mutex);auto*p=registered(prep,path,segment,true);return effect&&p&&evaluate(*p,length,x,y,dx,dy)?0:kBad;}
int32_t __cdecl path_cleanup_seg_length(void* effect,void* path,int32_t segment,void** prep){std::lock_guard lock(g_mutex);auto*p=registered(prep,path,segment,false);if(!effect||!p){++g_report.invalid_operations;return kBad;}auto it=std::find_if(g_preps.begin(),g_preps.end(),[p](auto&v){return &v==p;});g_preps.erase(it);*prep=nullptr;++g_report.preps_disposed;return 0;}
int32_t __cdecl path_is_inverted(void* effect,int32_t id,int8_t*out){auto p=paths();auto*f=find(p,id);if(!effect||!f||!out)return 4;*out=f->inverted?1:0;return 0;}
int32_t __cdecl path_get_mask_mode(void* effect,int32_t id,int32_t*out){auto p=paths();auto*f=find(p,id);if(!effect||!f||!out)return 4;*out=f->mode;return 0;}
int32_t __cdecl path_get_name(void* effect,int32_t id,char*out){auto p=paths();auto*f=find(p,id);if(!effect||!f||!out)return 4;auto value="Mask "+std::to_string(f->dynamic_order+1);if(value.size()>31)return 4;std::memcpy(out,value.c_str(),value.size()+1);return 0;}
int32_t __cdecl mask_world_with_path(void* effect, void** path, double fx,
    double fy, int32_t invert, double opacity, int32_t quality, void* world,
    LegacyRect* bounds) {
  std::lock_guard lock(g_mutex);
  void* handle = path ? *path : nullptr;
  mask_runtime::CurveSnapshot curve;
  g_report.last_feather_x = fx;
  g_report.last_feather_y = fy;
  g_report.last_opacity = opacity;
  g_report.last_quality = quality;
  g_report.reject_reason = !effect || !world ? 1 :
      (!checked(handle, curve) || curve.open ? 2 :
       (!std::isfinite(fx) || !std::isfinite(fy) || fx < 0 || fy < 0 ||
        fx > 32768 || fy > 32768 ? 3 :
        (!std::isfinite(opacity) || opacity < 0 || opacity > 1 ? 4 :
         ((invert != 0 && invert != 1) || (quality != 0 && quality != 1) ? 5 : 0))));
  if (g_report.reject_reason) {
    ++g_report.invalid_operations;
    return 4;
  }
  WorldView view;
  if (!g_hooks.bounded_world || !g_hooks.bounded_world(world, view) ||
      !supported_world_view(view)) {
    g_report.reject_reason = 6;
    ++g_report.invalid_operations;
    return 4;
  }
  LegacyRect area{0, 0, view.width, view.height};
  if (bounds) {
    std::memcpy(g_report.last_bounds.data(), bounds, sizeof(*bounds));
    area = *bounds;
  }
  if (area.left < 0 || area.top < 0 || area.right < area.left ||
      area.bottom < area.top || area.right > view.width ||
      area.bottom > view.height) {
    g_report.reject_reason = 7;
    ++g_report.invalid_operations;
    return 4;
  }
  std::vector<RasterPoint> points;
  if (!flatten(curve, quality, points)) {
    g_report.reject_reason = 8;
    ++g_report.invalid_operations;
    return 4;
  }
  if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb128) {
    for (int y = area.top; y < area.bottom; ++y)
      for (int x = area.left; x < area.right; ++x) {
        const auto* pixel = view.pixels + static_cast<size_t>(y) * view.rowbytes +
            static_cast<size_t>(x) * view.pixel_bytes;
        if (!std::isfinite(reinterpret_cast<const float*>(pixel)[0])) {
          g_report.reject_reason = 10;
          ++g_report.invalid_operations;
          return 4;
        }
      }
  }
  for (int y = area.top; y < area.bottom; ++y)
    for (int x = area.left; x < area.right; ++x) {
      const bool in = inside(points, x + .5, y + .5);
      double coverage = in ? 1 : 0;
      if (fx > 0 || fy > 0) {
        const double distance = edge_distance(points, x + .5, y + .5, fx, fy);
        coverage = std::clamp(.5 + (in ? distance : -distance), 0.0, 1.0);
      }
      if (invert) coverage = 1 - coverage;
      auto* pixel = view.pixels + static_cast<size_t>(y) * view.rowbytes +
          static_cast<size_t>(x) * view.pixel_bytes;
      const double factor = opacity * coverage;
      if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb32) {
        pixel[0] = static_cast<uint8_t>(std::lround(pixel[0] * factor));
      } else if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
        auto* channel = reinterpret_cast<uint16_t*>(pixel);
        *channel = static_cast<uint16_t>(std::lround(*channel * factor));
      } else {
        auto* channel = reinterpret_cast<float*>(pixel);
        *channel = std::clamp(*channel * static_cast<float>(factor), 0.0f, 1.0f);
      }
    }
  ++g_report.mask_calls;
  g_report.reject_reason = 0;
  return 0;
}
int32_t __cdecl mask_world_with_scene(void* effect, double fx, double fy,
    int32_t quality, void* world, LegacyRect* bounds) {
  // Deterministic host composition policy for all active masks of one layer.
  // The base value, the per-mode combination rules, and the invert-then-
  // opacity application order are a documented host policy that has NOT been
  // verified against an After Effects oracle yet. reject_reason values reuse
  // the mask_world_with_path codes where the failing contract is the same;
  // 11 = unsupported or out-of-range mask mode, 12 = invalid mask opacity.
  std::lock_guard lock(g_mutex);
  g_report.last_feather_x = fx;
  g_report.last_feather_y = fy;
  g_report.last_quality = quality;
  g_report.reject_reason = !effect || !world ? 1 :
      (!std::isfinite(fx) || !std::isfinite(fy) || fx < 0 || fy < 0 ||
       fx > 32768 || fy > 32768 ? 3 : ((quality != 0 && quality != 1) ? 5 : 0));
  if (g_report.reject_reason) {
    ++g_report.invalid_operations;
    return 4;
  }
  WorldView view;
  if (!g_hooks.bounded_world || !g_hooks.bounded_world(world, view) ||
      !supported_world_view(view)) {
    g_report.reject_reason = 6;
    ++g_report.invalid_operations;
    return 4;
  }
  LegacyRect area{0, 0, view.width, view.height};
  if (bounds) {
    std::memcpy(g_report.last_bounds.data(), bounds, sizeof(*bounds));
    area = *bounds;
  }
  if (area.left < 0 || area.top < 0 || area.right < area.left ||
      area.bottom < area.top || area.right > view.width ||
      area.bottom > view.height) {
    g_report.reject_reason = 7;
    ++g_report.invalid_operations;
    return 4;
  }
  struct Participant {
    std::vector<RasterPoint> points;
    int32_t mode{};
    bool inverted{};
    double opacity_scale{};
  };
  // Enumeration order is the host's dynamic_order ascending. Every rule is
  // validated before any pixel is touched so rejection leaves the world
  // unchanged (fail-closed).
  const auto scene = paths();
  std::vector<Participant> participants;
  bool has_add = false;
  try {
    for (const auto& path : scene) {
      if (path.mode < 0 || path.mode > 7 || path.mode >= 4) {
        // Defense in depth on the 0..7 range (set_mask_mode already bounds
        // it); LIGHTEN/DARKEN/DIFFERENCE/ACCUM (4..7) are unobserved, so they
        // fail closed instead of silently compositing a guess.
        g_report.reject_reason = 11;
        ++g_report.invalid_operations;
        return 4;
      }
      if (path.mode == 0) continue;  // PF_MaskMode_NONE: shape does nothing
      mask_runtime::CurveSnapshot curve;
      // Open paths do not participate in masking; like mask_world_with_path
      // this contract rejects them (and stale or missing checkouts) closed.
      if (path.open || !checked(path.handle, curve)) {
        g_report.reject_reason = 2;
        ++g_report.invalid_operations;
        return 4;
      }
      if (!std::isfinite(path.opacity) || path.opacity < 0 ||
          path.opacity > 100) {
        g_report.reject_reason = 12;
        ++g_report.invalid_operations;
        return 4;
      }
      Participant participant;
      participant.mode = path.mode;
      participant.inverted = path.inverted;
      participant.opacity_scale = path.opacity / 100;
      if (!flatten(curve, quality, participant.points)) {
        g_report.reject_reason = 8;
        ++g_report.invalid_operations;
        return 4;
      }
      has_add = has_add || path.mode == 1;
      participants.push_back(std::move(participant));
    }
    if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb128) {
      for (int y = area.top; y < area.bottom; ++y)
        for (int x = area.left; x < area.right; ++x) {
          const auto* pixel = view.pixels + static_cast<size_t>(y) * view.rowbytes +
              static_cast<size_t>(x) * view.pixel_bytes;
          if (!std::isfinite(reinterpret_cast<const float*>(pixel)[0])) {
            g_report.reject_reason = 10;
            ++g_report.invalid_operations;
            return 4;
          }
        }
    }
    // Base policy: without any ADD mask the layer alpha starts fully covered,
    // mirroring AE's documented "no ADD mask keeps the whole layer" behaviour.
    std::vector<double> coverage(
        static_cast<size_t>(area.right - area.left) * (area.bottom - area.top),
        has_add ? 0.0 : 1.0);
    const auto index = [&area](int x, int y) {
      return static_cast<size_t>(y - area.top) * (area.right - area.left) +
          (x - area.left);
    };
    for (const auto& participant : participants)
      for (int y = area.top; y < area.bottom; ++y)
        for (int x = area.left; x < area.right; ++x) {
          const bool in = inside(participant.points, x + .5, y + .5);
          double c = in ? 1 : 0;
          if (fx > 0 || fy > 0) {
            const double distance = edge_distance(participant.points, x + .5,
                                                  y + .5, fx, fy);
            c = std::clamp(.5 + (in ? distance : -distance), 0.0, 1.0);
          }
          if (participant.inverted) c = 1 - c;
          c *= participant.opacity_scale;
          double& a = coverage[index(x, y)];
          if (participant.mode == 1) {
            a = std::max(a, c);        // ADD
          } else if (participant.mode == 2) {
            a = a * (1 - c);           // SUBTRACT
          } else {
            a = std::min(a, c);        // INTERSECT
          }
        }
    for (int y = area.top; y < area.bottom; ++y)
      for (int x = area.left; x < area.right; ++x) {
        auto* pixel = view.pixels + static_cast<size_t>(y) * view.rowbytes +
            static_cast<size_t>(x) * view.pixel_bytes;
        const double factor = coverage[index(x, y)];
        if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb32) {
          pixel[0] = static_cast<uint8_t>(std::lround(pixel[0] * factor));
        } else if (view.pixel_format == aexcompat::world_registry::kPixelFormatArgb64) {
          auto* channel = reinterpret_cast<uint16_t*>(pixel);
          *channel = static_cast<uint16_t>(std::lround(*channel * factor));
        } else {
          auto* channel = reinterpret_cast<float*>(pixel);
          *channel = std::clamp(*channel * static_cast<float>(factor), 0.0f, 1.0f);
        }
      }
  } catch (const std::bad_alloc&) {
    return kOom;
  }
  ++g_report.composition_calls;
  g_report.reject_reason = 0;
  return 0;
}
} // namespace aexcompat::pf_path_runtime
