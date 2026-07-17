#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <array>
#include <cstddef>
#include <cstring>

namespace {
template <typename T> void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}
PF_Pixel8* pixel(PF_EffectWorld& world, A_long x, A_long y) {
  return reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(world.data) +
      static_cast<size_t>(y) * world.rowbytes) + x;
}
void seed(PF_EffectWorld& world) {
  for (A_long y=0; y<2; ++y) for (A_long x=0; x<3; ++x) {
    const A_u_char value = static_cast<A_u_char>((y*3+x+1)*10);
    *pixel(world,x,y) = PF_Pixel8{255,value,value,value};
  }
}
void clear(PF_EffectWorld& world) {
  std::memset(world.data, 0, static_cast<size_t>(world.rowbytes) * world.height);
}
bool red_is(PF_EffectWorld& world, A_long x, A_long y, A_u_char value) {
  return pixel(world,x,y)->red == value;
}
PF_Err apply(const PF_WorldTransformSuite1* suite, PF_ProgPtr ref,
             PF_EffectWorld& source, PF_EffectWorld& destination,
             const std::array<A_FpLong,9>& matrix, PF_Boolean source_to_destination,
             const PF_MaskWorld* mask = nullptr) {
  PF_CompositeMode mode{}; mode.xfer=PF_Xfer_COPY; mode.opacity=PF_MAX_CHAN8;
  mode.opacitySu=PF_MAX_CHAN16; mode.rgb_only=FALSE;
  PF_Rect bounds{0,0,destination.width,destination.height};
  return suite->transform_world(ref, PF_Quality_LO, PF_MF_Alpha_STRAIGHT, PF_Field_FRAME,
      &source, &mode, mask, reinterpret_cast<const PF_FloatMatrix*>(matrix.data()), 1,
      source_to_destination, &bounds, &destination);
}
PF_Err render(PF_InData* in, PF_LayerDef* output) {
  if (!in || !in->pica_basicP || !output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  const PF_WorldTransformSuite1* transforms=nullptr; const PF_WorldSuite2* worlds=nullptr;
  PF_Err err=static_cast<PF_Err>(in->pica_basicP->AcquireSuite(kPFWorldTransformSuite,
      kPFWorldTransformSuiteVersion1,reinterpret_cast<const void**>(&transforms)));
  if(!err) err=static_cast<PF_Err>(in->pica_basicP->AcquireSuite(kPFWorldSuite,
      kPFWorldSuiteVersion2,reinterpret_cast<const void**>(&worlds)));
  PF_EffectWorld source{},destination{},mask_pixels{};
  bool source_live=false,destination_live=false,mask_live=false;
  if(!err){err=worlds->PF_NewWorld(in->effect_ref,3,2,TRUE,PF_PixelFormat_ARGB32,&source);source_live=!err;}
  if(!err){err=worlds->PF_NewWorld(in->effect_ref,6,4,TRUE,PF_PixelFormat_ARGB32,&destination);destination_live=!err;}
  if(!err){err=worlds->PF_NewWorld(in->effect_ref,3,2,TRUE,PF_PixelFormat_ARGB32,&mask_pixels);mask_live=!err;}
  const std::array<A_FpLong,9> identity{{1,0,0,0,1,0,0,0,1}};
  if(!err){seed(source);clear(destination);err=apply(transforms,in->effect_ref,source,destination,identity,TRUE);}
  if(!err && (!red_is(destination,0,0,10)||!red_is(destination,2,1,60))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> forward{{1,0,0,0,1,0,1,1,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,forward,TRUE);}
  if(!err && (!red_is(destination,1,1,10)||!red_is(destination,3,2,60))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> inverse{{1,0,0,0,1,0,-1,-1,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,inverse,FALSE);}
  if(!err && (!red_is(destination,1,1,10)||!red_is(destination,3,2,60))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> scale{{2,0,0,0,2,0,0,0,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,scale,TRUE);}
  if(!err && (!red_is(destination,0,0,10)||!red_is(destination,1,1,10)||
      !red_is(destination,4,2,60)||!red_is(destination,5,3,60))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> rotate{{0,1,0,-1,0,0,2,0,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,rotate,TRUE);}
  if(!err && (!red_is(destination,1,0,10)||!red_is(destination,0,0,40)||
      !red_is(destination,1,2,30)||!red_is(destination,0,2,60))) err=PF_Err_BAD_CALLBACK_PARAM;
  PF_MaskWorld mask{}; mask.mask=mask_pixels; mask.offset=PF_Point{0,0};
  mask.what_is_mask=PF_MaskFlag_NONE;
  if(!err){
    clear(mask.mask); clear(destination);
    for(A_long y=0;y<2;++y){pixel(mask.mask,0,y)->alpha=0;
      pixel(mask.mask,1,y)->alpha=128;pixel(mask.mask,2,y)->alpha=255;}
    err=apply(transforms,in->effect_ref,source,destination,identity,TRUE,&mask);
  }
  if(!err && (!red_is(destination,0,0,0)||!red_is(destination,1,0,10)||
      !red_is(destination,2,0,30)||!red_is(destination,1,1,25))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> projective_inverse{{1,0,0.25,0,1,0,0,0,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,projective_inverse,FALSE);}
  if(!err && (!red_is(destination,0,0,10)||!red_is(destination,1,0,20)||
      !red_is(destination,2,0,20)||!red_is(destination,3,0,20))) err=PF_Err_BAD_CALLBACK_PARAM;
  const std::array<A_FpLong,9> projective_forward{{1,0,-0.25,0,1,0,0,0,1}};
  if(!err){clear(destination);err=apply(transforms,in->effect_ref,source,destination,projective_forward,TRUE);}
  if(!err && (!red_is(destination,0,0,10)||!red_is(destination,1,0,20)||
      !red_is(destination,2,0,20)||!red_is(destination,3,0,20))) err=PF_Err_BAD_CALLBACK_PARAM;
  if(mask_live) keep_first(err,worlds->PF_DisposeWorld(in->effect_ref,&mask_pixels));
  if(destination_live) keep_first(err,worlds->PF_DisposeWorld(in->effect_ref,&destination));
  if(source_live) keep_first(err,worlds->PF_DisposeWorld(in->effect_ref,&source));
  if(worlds) keep_first(err,in->pica_basicP->ReleaseSuite(kPFWorldSuite,kPFWorldSuiteVersion2));
  if(transforms) keep_first(err,in->pica_basicP->ReleaseSuite(kPFWorldTransformSuite,kPFWorldTransformSuiteVersion1));
  if(!err) std::memset(output->data,0,static_cast<size_t>(output->rowbytes)*output->height);
  return err;
}
}
extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd,PF_InData* in,PF_OutData* out,
    PF_ParamDef*[],PF_LayerDef* output,void*){
  if(cmd==PF_Cmd_GLOBAL_SETUP){out->my_version=PF_VERSION(1,0,0,PF_Stage_DEVELOP,0);out->out_flags=PF_OutFlag_PIX_INDEPENDENT;return PF_Err_NONE;}
  if(cmd==PF_Cmd_PARAMS_SETUP){out->num_params=1;return PF_Err_NONE;}
  return cmd==PF_Cmd_RENDER?render(in,output):PF_Err_NONE;
}
