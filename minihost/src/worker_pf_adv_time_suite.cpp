#include "worker_pf_adv_time_suite.hpp"

#include <climits>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <limits>
#include <type_traits>

namespace aexcompat::worker_runtime::pf_adv_time {

ItemTelemetry& item_telemetry() {
  static ItemTelemetry telemetry;
  return telemetry;
}
namespace {

struct AdvTimeDisplayPrefVersion3 { char display_mode; int32_t framemax; int32_t frames_per_foot; char frames_start; uint8_t nondrop30; uint8_t honor_source_timecode; uint8_t use_feet_frames; };
struct AdvTimeDisplayPrefVersion2 { char display_mode; char framemax; char frames_per_foot; char frames_start; uint8_t nondrop30; uint8_t honor_source_timecode; uint8_t use_feet_frames; };
struct AdvTimeDisplayPrefVersion1 { char time_display_format; char framemax; char nondrop30; char frames_per_foot; };
struct Time { int32_t value{}; uint32_t scale{1}; };
static_assert(sizeof(AdvTimeDisplayPrefVersion1) == 4 && alignof(AdvTimeDisplayPrefVersion1) == 1);
static_assert(sizeof(AdvTimeDisplayPrefVersion2) == 7 && alignof(AdvTimeDisplayPrefVersion2) == 1);
static_assert(sizeof(AdvTimeDisplayPrefVersion3) == 16 && alignof(AdvTimeDisplayPrefVersion3) == alignof(int32_t));
static_assert(offsetof(AdvTimeDisplayPrefVersion3, framemax) == 4 && offsetof(AdvTimeDisplayPrefVersion3, frames_per_foot) == 8);

constexpr int64_t kHeadlessFramesPerSecond = 30;
constexpr std::size_t kPfMaxTimeBufferSize = 32;
VerificationHooks g_hooks{};

bool checked_floor_ratio(int64_t numerator, uint64_t denominator, int64_t* result,
                         bool* has_remainder = nullptr) {
  if (!result || denominator == 0 || denominator > static_cast<uint64_t>(INT64_MAX)) return false;
  const auto divisor = static_cast<int64_t>(denominator);
  int64_t quotient = numerator / divisor;
  const int64_t remainder = numerator % divisor;
  if (remainder < 0) --quotient;
  *result = quotient;
  if (has_remainder) *has_remainder = remainder != 0;
  return true;
}

int32_t format_headless_time(int32_t value, uint32_t scale, char* buffer) {
  if (buffer) buffer[0] = '\0';
  if (!buffer || scale == 0) return 4;
  int64_t frame{};
  if (!checked_floor_ratio(static_cast<int64_t>(value) * kHeadlessFramesPerSecond,
                           scale, &frame)) return 4;
  const int written = std::snprintf(buffer, kPfMaxTimeBufferSize, "%lld",
                                    static_cast<long long>(frame));
  if (written < 0 || static_cast<std::size_t>(written) >= kPfMaxTimeBufferSize) {
    buffer[0] = '\0'; return 4;
  }
  return 0;
}
int32_t __cdecl format_active(int32_t value, uint32_t scale, uint8_t, char* buffer) { return format_headless_time(value, scale, buffer); }
int32_t __cdecl format(void*, void*, int32_t value, uint32_t scale, uint8_t duration, char* buffer) { return format_active(value, scale, duration, buffer); }
int32_t __cdecl format_plus(void*, void*, int32_t value, uint32_t scale, uint8_t, uint8_t duration, char* buffer) { return format_active(value, scale, duration, buffer); }
int32_t __cdecl display_pref_v3(AdvTimeDisplayPrefVersion3* pref, int32_t* starting_frame) {
  if (!pref || !starting_frame) return 4;
  // The headless host has no AE UI preference state: publish one stable frames mode.
  std::memset(pref, 0, sizeof(*pref)); pref->display_mode = 1;
  pref->framemax = static_cast<int32_t>(kHeadlessFramesPerSecond);
  pref->nondrop30 = 1; *starting_frame = 0; return 0;
}
int32_t __cdecl display_pref_v1(AdvTimeDisplayPrefVersion1* pref, int32_t* starting_frame) {
  if (!pref || !starting_frame) return 4;
  *pref = {1, static_cast<char>(kHeadlessFramesPerSecond), 1, 0};
  *starting_frame = 0; return 0;
}
bool checked_char(int32_t value, char* narrowed) {
  if (!narrowed || value < CHAR_MIN || value > CHAR_MAX) return false;
  *narrowed = static_cast<char>(value); return true;
}
int32_t __cdecl display_pref_v2(AdvTimeDisplayPrefVersion2* pref, int32_t* starting_frame) {
  if (!pref || !starting_frame) return 4;
  AdvTimeDisplayPrefVersion2 result{};
  if (!checked_char(1, &result.display_mode) ||
      !checked_char(static_cast<int32_t>(kHeadlessFramesPerSecond), &result.framemax) ||
      !checked_char(0, &result.frames_per_foot) || !checked_char(0, &result.frames_start)) return 4;
  result.nondrop30 = 1; *pref = result; *starting_frame = 0; return 0;
}
int32_t __cdecl count_frames(const Time* start, const Time* step, uint8_t include_partial, int32_t* frame_count) {
  if (frame_count) *frame_count = 0;
  if (!start || !step || !frame_count || start->scale == 0 || step->scale == 0 || step->value <= 0) return 4;
  int64_t count{}; bool partial{};
  if (!checked_floor_ratio(static_cast<int64_t>(start->value) * step->scale,
      static_cast<uint64_t>(start->scale) * static_cast<uint64_t>(step->value), &count, &partial)) return 4;
  if (include_partial && partial) { if (count == INT64_MAX) return 4; ++count; }
  if (count < INT32_MIN || count > INT32_MAX) return 4;
  *frame_count = static_cast<int32_t>(count); return 0;
}

struct AdvTimeSuite1 { decltype(&format_active) format_active; decltype(&format) format; decltype(&format_plus) format_plus; decltype(&display_pref_v1) get_display_pref; };
struct AdvTimeSuite2 { decltype(&format_active) format_active; decltype(&format) format; decltype(&format_plus) format_plus; decltype(&display_pref_v2) get_display_pref; };
struct AdvTimeSuite3 { decltype(&format_active) format_active; decltype(&format) format; decltype(&format_plus) format_plus; decltype(&display_pref_v3) get_display_pref; };
struct AdvTimeSuite4 { decltype(&format_active) format_active; decltype(&format) format; decltype(&format_plus) format_plus; decltype(&display_pref_v3) get_display_pref; decltype(&count_frames) count_frames; };
static_assert(sizeof(AdvTimeSuite1) == 4 * sizeof(void*));
static_assert(sizeof(AdvTimeSuite2) == 4 * sizeof(void*));
static_assert(sizeof(AdvTimeSuite3) == 4 * sizeof(void*));
static_assert(sizeof(AdvTimeSuite4) == 5 * sizeof(void*));
static_assert(offsetof(AdvTimeSuite1, format_active) == 0 && offsetof(AdvTimeSuite1, get_display_pref) == 3 * sizeof(void*));
static_assert(offsetof(AdvTimeSuite2, format_active) == 0 && offsetof(AdvTimeSuite2, get_display_pref) == 3 * sizeof(void*));
static_assert(offsetof(AdvTimeSuite3, format_active) == 0 && offsetof(AdvTimeSuite3, get_display_pref) == 3 * sizeof(void*));
static_assert(offsetof(AdvTimeSuite4, format_active) == 0 && offsetof(AdvTimeSuite4, count_frames) == 4 * sizeof(void*));
static_assert(std::is_same_v<decltype(AdvTimeSuite1::format_active), decltype(AdvTimeSuite4::format_active)>);
AdvTimeSuite1 g_adv_time_suite1{&format_active, &format, &format_plus, &display_pref_v1};
AdvTimeSuite2 g_adv_time_suite2{&format_active, &format, &format_plus, &display_pref_v2};
AdvTimeSuite3 g_adv_time_suite3{&format_active, &format, &format_plus, &display_pref_v3};
AdvTimeSuite4 g_adv_time_suite4{&format_active, &format, &format_plus, &display_pref_v3, &count_frames};

}  // namespace

const void* suite(int32_t version) noexcept {
  switch (version) { case 1: return &g_adv_time_suite1; case 2: return &g_adv_time_suite2; case 3: return &g_adv_time_suite3; case 4: return &g_adv_time_suite4; default: return nullptr; }
}
bool configure_verification_hooks(const VerificationHooks& hooks) noexcept {
  if (!hooks.acquire || !hooks.release || !hooks.acquire_count || !hooks.release_count || !hooks.leases_balanced) return false;
  g_hooks = hooks; return true;
}
bool verify_suite_versions() {
  if (!g_hooks.acquire) return false;
  struct Guard1 { uint32_t before{0x13579bdf}; AdvTimeDisplayPrefVersion1 pref{}; uint32_t after{0x2468ace0}; } p1;
  struct Guard2 { uint32_t before{0x11223344}; AdvTimeDisplayPrefVersion2 pref{}; uint32_t after{0x55667788}; } p2;
  struct Guard3 { uint32_t before{0x10203040}; AdvTimeDisplayPrefVersion3 pref{}; uint32_t after{0x50607080}; } p3;
  struct Buffer { uint32_t before{0x89abcdef}; char text[kPfMaxTimeBufferSize]{}; uint32_t after{0xfedcba98}; } formatted;
  const void *s1{}, *s2{}, *s3{}, *s4{}; const auto acq = g_hooks.acquire_count(), rel = g_hooks.release_count();
  bool ok = g_hooks.acquire("PF AE Adv Time Suite",1,&s1)==0 && g_hooks.acquire("PF AE Adv Time Suite",2,&s2)==0 && g_hooks.acquire("PF AE Adv Time Suite",3,&s3)==0 && g_hooks.acquire("PF AE Adv Time Suite",4,&s4)==0 && s1==suite(1) && s2==suite(2) && s3==suite(3) && s4==suite(4) && s1!=s2 && s1!=s3 && s1!=s4 && s2!=s3 && s2!=s4 && s3!=s4;
  int32_t f1=-1,f2=-1,f3=-1;
  if (s1&&s2&&s3) { const auto* v1=static_cast<const AdvTimeSuite1*>(s1); const auto* v2=static_cast<const AdvTimeSuite2*>(s2); const auto* v3=static_cast<const AdvTimeSuite3*>(s3); ok = ok && v1->format_active==g_adv_time_suite4.format_active && v1->format==g_adv_time_suite4.format && v1->format_plus==g_adv_time_suite4.format_plus && v1->format_active(1,1,0,formatted.text)==0 && std::strcmp(formatted.text,"30")==0 && v1->get_display_pref(&p1.pref,&f1)==0 && v2->get_display_pref(&p2.pref,&f2)==0 && v3->get_display_pref(&p3.pref,&f3)==0 && v3->get_display_pref==g_adv_time_suite4.get_display_pref; }
  ok = ok && formatted.before==0x89abcdef && formatted.after==0xfedcba98 && p1.before==0x13579bdf && p1.after==0x2468ace0 && p2.before==0x11223344 && p2.after==0x55667788 && p3.before==0x10203040 && p3.after==0x50607080 && p1.pref.time_display_format==1 && p1.pref.framemax==30 && p1.pref.nondrop30==1 && p2.pref.display_mode==1 && p2.pref.framemax==30 && p3.pref.display_mode==1 && p3.pref.framemax==30 && f1==0 && f2==0 && f3==0;
  char narrowed=42; ok = ok && !checked_char(CHAR_MAX+1,&narrowed) && narrowed==42 && !checked_char(CHAR_MIN-1,&narrowed) && narrowed==42;
  ok = g_hooks.release("PF AE Adv Time Suite",4)==0 && g_hooks.release("PF AE Adv Time Suite",3)==0 && g_hooks.release("PF AE Adv Time Suite",2)==0 && g_hooks.release("PF AE Adv Time Suite",1)==0 && ok;
  return ok && g_hooks.acquire_count()==acq+4 && g_hooks.release_count()==rel+4 && g_hooks.leases_balanced();
}

}  // namespace aexcompat::worker_runtime::pf_adv_time
