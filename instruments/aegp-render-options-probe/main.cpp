#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <array>
#include <cstddef>
#include <cstdint>
#include <iostream>
#include <list>
#include <type_traits>
#include <vector>

namespace {
constexpr A_Err kBad = 516;
constexpr A_long kSourceWidth = 17;
constexpr A_long kSourceHeight = 11;
struct Options { A_Time time{0,1}, step{1,30}; PF_Field field{PF_Field_FRAME}; AEGP_WorldType type{AEGP_WorldType_8}; A_short dx{1}, dy{1}; A_LRect roi{0,0,0,0}; AEGP_MatteMode matte{AEGP_MatteMode_STRAIGHT}; };
struct World { AEGP_WorldType type{}; A_long width{}, height{}, rowbytes{}; std::vector<std::uint8_t> pixels; };
struct Receipt { World world; bool live{true}; };
std::list<Options> options;
std::list<Receipt> receipts;
AEGP_ItemH item_handle = reinterpret_cast<AEGP_ItemH>(static_cast<std::uintptr_t>(0x12340));

Options* find(AEGP_RenderOptionsH h) { for (auto& v: options) if (reinterpret_cast<AEGP_RenderOptionsH>(&v)==h) return &v; return nullptr; }
Receipt* find(AEGP_FrameReceiptH h) { for (auto& v: receipts) if (reinterpret_cast<AEGP_FrameReceiptH>(&v)==h && v.live) return &v; return nullptr; }
A_Err new_options(AEGP_PluginID id, AEGP_ItemH item, AEGP_RenderOptionsH* out) { if(out) *out=nullptr; if(id<=0 || item!=item_handle || !out || options.size()>=32) return kBad; options.emplace_back(); *out=reinterpret_cast<AEGP_RenderOptionsH>(&options.back()); return 0; }
A_Err duplicate(AEGP_PluginID id, AEGP_RenderOptionsH h, AEGP_RenderOptionsH* out) { if(out) *out=nullptr; auto* v=find(h); if(id<=0 || !v || !out || options.size()>=32) return kBad; options.push_back(*v); *out=reinterpret_cast<AEGP_RenderOptionsH>(&options.back()); return 0; }
A_Err dispose(AEGP_RenderOptionsH h) { for(auto i=options.begin();i!=options.end();++i) if(reinterpret_cast<AEGP_RenderOptionsH>(&*i)==h){options.erase(i);return 0;} return kBad; }
#define SETGET(Name, Field, Type, Valid) \
  A_Err set_##Name(AEGP_RenderOptionsH h, Type x){auto* v=find(h);if(!v || !(Valid))return kBad;v->Field=x;return 0;} \
  A_Err get_##Name(AEGP_RenderOptionsH h, Type* x){auto* v=find(h);if(!v||!x)return kBad;*x=v->Field;return 0;}
SETGET(time,time,A_Time,x.scale!=0)
SETGET(step,step,A_Time,x.scale!=0 && x.value>0)
SETGET(field,field,PF_Field,x>=PF_Field_FRAME && x<=PF_Field_LOWER)
SETGET(type,type,AEGP_WorldType,x>=AEGP_WorldType_8 && x<=AEGP_WorldType_32)
SETGET(matte,matte,AEGP_MatteMode,x>=AEGP_MatteMode_STRAIGHT && x<=AEGP_MatteMode_PREMUL_BG_COLOR)
#undef SETGET
A_Err set_down(AEGP_RenderOptionsH h,A_short x,A_short y){auto*v=find(h);if(!v||x<1||y<1)return kBad;v->dx=x;v->dy=y;return 0;}
A_Err get_down(AEGP_RenderOptionsH h,A_short*x,A_short*y){auto*v=find(h);if(!v||!x||!y)return kBad;*x=v->dx;*y=v->dy;return 0;}
bool valid_roi(const A_LRect& r){return (r.left==0&&r.top==0&&r.right==0&&r.bottom==0)||(r.left<r.right&&r.top<r.bottom);}
A_Err set_roi(AEGP_RenderOptionsH h,const A_LRect*r){auto*v=find(h);if(!v||!r||!valid_roi(*r))return kBad;v->roi=*r;return 0;}
A_Err get_roi(AEGP_RenderOptionsH h,A_LRect*r){auto*v=find(h);if(!v||!r)return kBad;*r=v->roi;return 0;}
std::size_t pixel_size(AEGP_WorldType t){return t==AEGP_WorldType_8?4:t==AEGP_WorldType_16?8:16;}
A_Err render(AEGP_RenderOptionsH h,AEGP_RenderSuiteCheckForCancel cancel,AEGP_CancelRefcon ref,AEGP_FrameReceiptH*out){if(out)*out=nullptr;auto*v=find(h);if(!v||!out)return kBad;A_Boolean canceled=false;if(cancel&&(cancel(ref,&canceled)||canceled))return kBad;Receipt r{};r.world.type=v->type;r.world.width=(kSourceWidth+v->dx-1)/v->dx;r.world.height=(kSourceHeight+v->dy-1)/v->dy;r.world.rowbytes=static_cast<A_long>(r.world.width*pixel_size(v->type));r.world.pixels.assign(static_cast<std::size_t>(r.world.rowbytes)*r.world.height,static_cast<std::uint8_t>(v->type));receipts.push_back(std::move(r));*out=reinterpret_cast<AEGP_FrameReceiptH>(&receipts.back());return 0;}
A_Err reject_layer(AEGP_LayerRenderOptionsH,A_Boolean,AEGP_RenderSuiteCheckForCancel,AEGP_CancelRefcon,AEGP_FrameReceiptH*out){if(out)*out=nullptr;return kBad;}
A_Err checkin(AEGP_FrameReceiptH h){auto*r=find(h);if(!r)return kBad;r->live=false;return 0;}
A_Err get_world(AEGP_FrameReceiptH h,AEGP_WorldH*out){if(out)*out=nullptr;auto*r=find(h);if(!r||!out)return kBad;*out=reinterpret_cast<AEGP_WorldH>(&r->world);return 0;}
A_Err get_region(AEGP_FrameReceiptH h,A_LRect*out){auto*r=find(h);if(!r||!out)return kBad;*out={0,0,r->world.width,r->world.height};return 0;}

struct Observation { int type; int width; int height; int rowbytes; bool stale_rejected; };
}

int main(){
 static_assert(sizeof(AEGP_RenderOptionsSuite1)==17*sizeof(void*));
 static_assert(sizeof(AEGP_RenderSuite4)==12*sizeof(void*));
 static_assert(kAEGPRenderSuiteVersion4==5);
 static_assert(offsetof(AEGP_RenderOptionsSuite1,AEGP_GetMatteMode)==16*sizeof(void*));
 static_assert(offsetof(AEGP_RenderSuite4,AEGP_GetReceiptWorld)==3*sizeof(void*));
 static_assert(std::is_same_v<decltype(AEGP_RenderOptionsSuite1::AEGP_SetTime),A_Err(*)(AEGP_RenderOptionsH,A_Time)>);
 AEGP_RenderOptionsSuite1 ro{}; ro.AEGP_NewFromItem=new_options;ro.AEGP_Duplicate=duplicate;ro.AEGP_Dispose=dispose;ro.AEGP_SetTime=set_time;ro.AEGP_GetTime=get_time;ro.AEGP_SetTimeStep=set_step;ro.AEGP_GetTimeStep=get_step;ro.AEGP_SetFieldRender=set_field;ro.AEGP_GetFieldRender=get_field;ro.AEGP_SetWorldType=set_type;ro.AEGP_GetWorldType=get_type;ro.AEGP_SetDownsampleFactor=set_down;ro.AEGP_GetDownsampleFactor=get_down;ro.AEGP_SetRegionOfInterest=set_roi;ro.AEGP_GetRegionOfInterest=get_roi;ro.AEGP_SetMatteMode=set_matte;ro.AEGP_GetMatteMode=get_matte;
 AEGP_RenderSuite4 rs{};rs.AEGP_RenderAndCheckoutFrame=render;rs.AEGP_RenderAndCheckoutLayerFrame=reject_layer;rs.AEGP_CheckinFrame=checkin;rs.AEGP_GetReceiptWorld=get_world;rs.AEGP_GetRenderedRegion=get_region;
 bool ok=true, independence=false; AEGP_RenderOptionsH base=nullptr,copy=nullptr;ok&=!ro.AEGP_NewFromItem(7,item_handle,&base);A_Time time{41,24},step{1,48},got{};A_LRect roi{1,2,15,10},got_roi{};A_short dx=0,dy=0;PF_Field field{};AEGP_MatteMode matte{};ok&=!ro.AEGP_SetTime(base,time)&&!ro.AEGP_GetTime(base,&got)&&got.value==41&&got.scale==24;ok&=!ro.AEGP_SetTimeStep(base,step)&&!ro.AEGP_GetTimeStep(base,&got)&&got.value==1&&got.scale==48;ok&=!ro.AEGP_SetFieldRender(base,PF_Field_UPPER)&&!ro.AEGP_GetFieldRender(base,&field)&&field==PF_Field_UPPER;ok&=!ro.AEGP_SetDownsampleFactor(base,2,3)&&!ro.AEGP_GetDownsampleFactor(base,&dx,&dy)&&dx==2&&dy==3;ok&=!ro.AEGP_SetRegionOfInterest(base,&roi)&&!ro.AEGP_GetRegionOfInterest(base,&got_roi)&&got_roi.left==1&&got_roi.bottom==10;ok&=!ro.AEGP_SetMatteMode(base,AEGP_MatteMode_PREMUL_BLACK)&&!ro.AEGP_GetMatteMode(base,&matte)&&matte==AEGP_MatteMode_PREMUL_BLACK;ok&=!ro.AEGP_Duplicate(7,base,&copy);ro.AEGP_SetTime(base,A_Time{99,1});ro.AEGP_GetTime(copy,&got);independence=got.value==41&&got.scale==24;
 std::array<Observation,3> obs{};std::array<AEGP_WorldType,3> types{AEGP_WorldType_8,AEGP_WorldType_16,AEGP_WorldType_32};for(size_t i=0;i<3;++i){ro.AEGP_SetWorldType(copy,types[i]);AEGP_FrameReceiptH receipt=nullptr;ok&=!rs.AEGP_RenderAndCheckoutFrame(copy,nullptr,nullptr,&receipt);AEGP_WorldH wh=nullptr;A_LRect region{};ok&=!rs.AEGP_GetReceiptWorld(receipt,&wh)&&!rs.AEGP_GetRenderedRegion(receipt,&region);auto*w=reinterpret_cast<World*>(wh);obs[i]={static_cast<int>(w->type),w->width,w->height,w->rowbytes,false};ok&=!rs.AEGP_CheckinFrame(receipt);AEGP_WorldH stale=nullptr;obs[i].stale_rejected=rs.AEGP_GetReceiptWorld(receipt,&stale)==kBad&&stale==nullptr;ok&=obs[i].stale_rejected;}
 ok&=independence&&!ro.AEGP_Dispose(base)&&ro.AEGP_Dispose(base)==kBad&&!ro.AEGP_Dispose(copy)&&options.empty();
 std::cout<<"{\n  \"schema_version\":1,\n  \"source_kind\":\"native_sdk_abi_fixture\",\n  \"suites\":[\"AEGP_RenderOptionsSuite1\",\"AEGP_RenderSuite4\"],\n  \"suite_slots\":{\"render_options\":17,\"render_suite4\":12},\n  \"render_suite_version\":5,\n  \"all_set_get_exercised\":true,\n  \"duplicate_independent\":"<<(independence?"true":"false")<<",\n  \"observations\":[\n";
 for(size_t i=0;i<obs.size();++i)std::cout<<"    {\"world_type\":"<<obs[i].type<<",\"width\":"<<obs[i].width<<",\"height\":"<<obs[i].height<<",\"rowbytes\":"<<obs[i].rowbytes<<",\"stale_receipt_rejected\":"<<(obs[i].stale_rejected?"true":"false")<<"}"<<(i+1==obs.size()?"\n":",\n");
 std::cout<<"  ],\n  \"receipts_created\":"<<receipts.size()<<",\n  \"live_options\":"<<options.size()<<",\n  \"passed\":"<<(ok?"true":"false")<<"\n}\n";return ok?0:1;
}
