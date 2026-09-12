#include "worker_smart_runtime.hpp"
#include "worker_callback_diagnostics.hpp"
#include "worker_extended_diag.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <iostream>
#include <thread>

namespace aexcompat::worker_runtime::smart {
namespace {

constexpr std::size_t kCheckoutResultBytes = 76;
constexpr std::size_t kMaxPixelCheckouts = 64;
thread_local State g_default_state;
// Counts what `--self-test-pf-checkout-intersection`'s stub allocator did; the
// stub is a plain function pointer, so it needs somewhere to record that.
thread_local int g_empty_layer_allocations_for_self_test;
thread_local State* g_active_state{};
std::atomic_uint32_t g_active_session_count{};

int32_t finish_callback(callback_diagnostics::Callback callback, int32_t result,
                        callback_diagnostics::Reason reason =
                            callback_diagnostics::Reason::None) {
  callback_diagnostics::record(callback, result, reason);
  if (aexcompat::l2_detail::extended_diag_enabled()) {
    std::cerr << "extended_diag:"
              << callback_diagnostics::CALLBACK_NAMES[static_cast<std::size_t>(callback)]
              << " -> " << result;
    if (result != 0)
      std::cerr << " ("
                << callback_diagnostics::REASON_NAMES[static_cast<std::size_t>(reason)]
                << ')';
    std::cerr << '\n' << std::flush;
  }
  return result;
}

bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale) {
  return static_cast<int64_t>(left) * right_scale ==
      static_cast<int64_t>(right) * left_scale;
}

enum class CheckoutRequestState { Full, Rect, Malformed };

void write_world_extent_hint(void* world, const std::array<int32_t, 4>& rect) {
  if (world) std::memcpy(static_cast<std::byte*>(world) + 44, rect.data(), sizeof(rect));
}

// The `extended_diag:pre_checkout_layer` line carried only the index, the
// checkout id and the time. Which rect the plug-in asked for and which one the
// host answered is what actually decides a SmartFX output extent, and having to
// infer it from the two `input_checkout_*` report fields (each holding only the
// last index-0 call) is how a multi-checkout PreRender stayed unreadable
// (issue #1285).
void trace_checkout_rect(const char* label, const std::array<int32_t, 4>& rect) {
  if (!aexcompat::l2_detail::extended_diag_enabled()) return;
  std::cerr << ' ' << label << "=[" << rect[0] << ',' << rect[1] << ','
            << rect[2] << ',' << rect[3] << ']';
}

void write_checkout_result(void* destination,
                           const std::array<int32_t, 4>& result_rect,
                           const std::array<int32_t, 4>& max_result_rect,
                           int32_t reference_width,
                           int32_t reference_height) {
  if (aexcompat::l2_detail::extended_diag_enabled()) {
    std::cerr << "extended_diag:pre_checkout_answer";
    trace_checkout_rect("result", result_rect);
    trace_checkout_rect("max_result", max_result_rect);
    std::cerr << " reference=" << reference_width << 'x' << reference_height
              << "\n" << std::flush;
  }
  auto* bytes = static_cast<unsigned char*>(destination);
  std::memset(bytes, 0, kCheckoutResultBytes);
  std::memcpy(bytes, result_rect.data(), sizeof(result_rect));
  std::memcpy(bytes + 16, max_result_rect.data(), sizeof(max_result_rect));
  const auto& runtime = state();
  const int32_t par[2] = {runtime.pixel_aspect_numerator,
                          static_cast<int32_t>(runtime.pixel_aspect_denominator)};
  std::memcpy(bytes + 32, par, sizeof(par));
  const int32_t reference_size[2] = {reference_width, reference_height};
  std::memcpy(bytes + 44, reference_size, sizeof(reference_size));
}

CheckoutRequestState parse_checkout_request(const void* request,
                                             std::array<int32_t, 4>& rect) {
  if (!request) return CheckoutRequestState::Full;
  std::memcpy(rect.data(), request, sizeof(rect));
  if (rect[2] < rect[0] || rect[3] < rect[1])
    return CheckoutRequestState::Malformed;
  return CheckoutRequestState::Rect;
}

bool empty_checkout_rect(const std::array<int32_t, 4>& rect) {
  return rect[0] == rect[2] || rect[1] == rect[3];
}

std::array<int32_t, 4> intersect_checkout_rect(
    const std::array<int32_t, 4>& rect, int32_t width, int32_t height) {
  const std::array<int32_t, 4> clipped{std::max(rect[0], 0), std::max(rect[1], 0),
      std::min(rect[2], width), std::min(rect[3], height)};
  if (clipped[0] >= clipped[2] || clipped[1] >= clipped[3]) return {0, 0, 0, 0};
  return clipped;
}

bool checkout_promised_no_pixels(const std::array<int32_t, 4>& rect) {
  const std::array<int32_t, 4> sentinel{-1, -1, -1, -1};
  return rect != sentinel && empty_checkout_rect(rect);
}

bool checkout_id_registered(const State& runtime, int32_t checkout_id) {
  return std::any_of(runtime.pixel_checkouts.begin(),
      runtime.pixel_checkouts.end(), [checkout_id](const auto& checkout) {
        return checkout.id == checkout_id;
      });
}

// Drops any registration this checkout id already has, so re-checking it out
// answers the new geometry instead of being refused.
//
// PreRender is a negotiation: a plug-in may check the same layer out several
// times with different request rects to learn what it would be given, and the
// SDK gives it no other way to ask. Treating the second call as a duplicate
// registration made the plug-in abandon the frame (issue #675). Only the last
// answer can be checked out for pixels, which is what replacing preserves.
//
// Called at each success site, never up front: a re-checkout that goes on to be
// refused (a slot that is neither hosted nor secondary, a timed slot asked at
// the wrong time) must leave the earlier registration standing, or one
// mis-probed layer would take the good answer with it.
void forget_checkout(State& runtime, int32_t checkout_id) {
  runtime.pixel_checkouts.erase(
      std::remove_if(runtime.pixel_checkouts.begin(), runtime.pixel_checkouts.end(),
          [checkout_id](const auto& checkout) { return checkout.id == checkout_id; }),
      runtime.pixel_checkouts.end());
}

}  // namespace

void State::clear_transient() {
  input_world = nullptr;
  output_world = nullptr;
  map_world = nullptr;
  input_checkout_view_world = nullptr;
  map_checkout_view_world = nullptr;
  hosted_layers.clear();
  pixel_checkouts.clear();
  map_width = 0;
  map_height = 0;
  secondary_checkout_id = -1;
  checkout_time = 0;
  checkout_time_step = 0;
  checkout_time_scale = 0;
  input_checkout_request.fill(-1);
  map_checkout_request.fill(-1);
  input_checkout_result_rect.fill(-1);
  map_checkout_result_rect.fill(-1);
  malformed_checkout_requests = 0;
  empty_checkout_pixel_denials = 0;
  empty_layer_param_checkouts = 0;
  empty_layer_param_pixel_checkouts = 0;
  empty_layer_world = {};
  empty_layer_world_live = false;
  allocate_empty_layer = nullptr;
  // Set fresh by every dispatch, but a session that ends before that would
  // otherwise carry the previous one's declared count into the range
  // `pre_checkout_layer` accepts.
  param_count = 0;
  gpu_render_dispatched = false;
}

HostTelemetry& host_telemetry() {
  static HostTelemetry telemetry;
  return telemetry;
}

State& state() { return g_active_state ? *g_active_state : g_default_state; }
bool dispatch_active() { return g_active_session_count.load(std::memory_order_acquire) != 0; }
bool current_thread_has_session() { return g_active_state != nullptr; }
const void* timed_parameter_world(int32_t slot, int32_t time, uint32_t scale,
                                  bool& timed_slot) {
  timed_slot = false;
  if (!g_active_state || scale == 0) return nullptr;
  const auto& layers = g_active_state->hosted_layers;
  timed_slot = std::any_of(layers.begin(), layers.end(),
      [slot](const auto& layer) { return layer.slot == slot && layer.timed; });
  if (!timed_slot) return nullptr;
  auto found = std::find_if(layers.begin(), layers.end(),
      [=](const auto& layer) { return layer.slot == slot && layer.timed &&
          static_cast<int64_t>(layer.time) * scale ==
          static_cast<int64_t>(time) * layer.time_scale; });
  if (found == layers.end())
    found = std::find_if(layers.begin(), layers.end(),
        [slot](const auto& layer) { return layer.slot == slot && !layer.timed; });
  if (found != layers.end()) return found->world;
  if (slot == 0 && same_rational_time(time, scale,
          g_active_state->current_time, g_active_state->current_time_scale))
    return g_active_state->input_world;
  return nullptr;
}
int32_t __cdecl width() { return g_active_state ? g_active_state->width : 0; }
int32_t __cdecl height() { return g_active_state ? g_active_state->height : 0; }

Session::Session() : previous_(g_active_state) {
  g_active_state = &state_;
  g_active_session_count.fetch_add(1, std::memory_order_release);
}

Session::~Session() {
  snapshot_->width = state_.width;
  snapshot_->height = state_.height;
  snapshot_->rowbytes = state_.rowbytes;
  snapshot_->pixel_format = state_.pixel_format;
  snapshot_->wide_time_checkout_allowed = state_.wide_time_checkout_allowed;
  snapshot_->shutter_dependency_advertised = state_.shutter_dependency_advertised;
  snapshot_->rejected_temporal_checkouts = state_.rejected_temporal_checkouts;
  snapshot_->checkout_time = state_.checkout_time;
  snapshot_->checkout_time_step = state_.checkout_time_step;
  snapshot_->checkout_time_scale = state_.checkout_time_scale;
  snapshot_->input_checkout_request = state_.input_checkout_request;
  snapshot_->map_checkout_request = state_.map_checkout_request;
  snapshot_->input_checkout_result_rect = state_.input_checkout_result_rect;
  snapshot_->map_checkout_result_rect = state_.map_checkout_result_rect;
  snapshot_->malformed_checkout_requests = state_.malformed_checkout_requests;
  snapshot_->empty_checkout_pixel_denials = state_.empty_checkout_pixel_denials;
  snapshot_->empty_layer_param_checkouts = state_.empty_layer_param_checkouts;
  snapshot_->empty_layer_param_pixel_checkouts =
      state_.empty_layer_param_pixel_checkouts;
  snapshot_->pixel_checkouts_balanced = pixel_checkouts_balanced();
  state_.clear_transient();
  g_active_state = previous_;
  g_active_session_count.fetch_sub(1, std::memory_order_release);
}

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time,
                                   int32_t time_step, uint32_t time_scale,
                                   void* result) {
  // PF does not provide a host refcon for these callback tables. A callback
  // made on a thread other than the selector thread cannot be bound safely.
  using callback_diagnostics::Callback;
  using callback_diagnostics::Reason;
  if (aexcompat::l2_detail::extended_diag_enabled())
    std::cerr << "extended_diag:pre_checkout_layer index=" << index
              << " id=" << checkout_id << " time=" << what_time << "/"
              << time_scale << " step=" << time_step << "\n" << std::flush;
  if (!g_active_state)
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::NoActiveState);
  // time_step == 0 is accepted, same acceptance as checkout_param: ForceMB and
  // WideTime (AE-shipped) request their input layer with a zero step and
  // render in AE (the #777 inference form). The step is recorded for
  // diagnostics only; negative stays refused.
  if (time_step < 0 || time_scale == 0)
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::InvalidArguments);
  auto& runtime = *g_active_state;
  const bool current_time =
      static_cast<int64_t>(what_time) * runtime.current_time_scale ==
      static_cast<int64_t>(runtime.current_time) * time_scale;
  // An explicitly supplied temporal image is available regardless of the
  // plug-in's dependency flag. Do not extend that to absent-time fallback.
  const bool explicit_time = std::any_of(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(), [=](const auto& layer) {
        return layer.slot == index && layer.timed &&
            same_rational_time(layer.time, layer.time_scale, what_time, time_scale);
      });
  if (!current_time && !runtime.wide_time_checkout_allowed && !explicit_time) {
    ++runtime.rejected_temporal_checkouts;
    return finish_callback(
        Callback::PreCheckoutLayer, 4, Reason::TemporalCheckoutDenied);
  }
  auto hosted = std::find_if(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(), [index, what_time, time_scale](const auto& layer) {
        return layer.slot == index && layer.timed &&
            same_rational_time(layer.time, layer.time_scale, what_time, time_scale);
      });
  if (hosted == runtime.hosted_layers.end())
    hosted = std::find_if(runtime.hosted_layers.begin(), runtime.hosted_layers.end(),
        [index](const auto& layer) { return layer.slot == index && !layer.timed; });
  const bool timed_slot = std::any_of(runtime.hosted_layers.begin(),
      runtime.hosted_layers.end(),
      [index](const auto& layer) { return layer.slot == index && layer.timed; });
  std::array<int32_t, 4> request_rect{};
  const CheckoutRequestState request_state = parse_checkout_request(request, request_rect);
  if (aexcompat::l2_detail::extended_diag_enabled()) {
    std::cerr << "extended_diag:pre_checkout_request state="
              << (request_state == CheckoutRequestState::Full ? "full"
                  : request_state == CheckoutRequestState::Rect ? "rect"
                                                                : "malformed");
    if (request_state != CheckoutRequestState::Full)
      trace_checkout_rect("rect", request_rect);
    std::cerr << "\n" << std::flush;
  }
  if (request_state == CheckoutRequestState::Malformed) {
    ++runtime.malformed_checkout_requests;
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::MalformedRequest);
  }
  const auto answer_rect = [&](int32_t width, int32_t height) {
    return request_state == CheckoutRequestState::Rect
        ? intersect_checkout_rect(request_rect, width, height)
        : std::array<int32_t, 4>{0, 0, width, height};
  };
  // The cap bounds how many *distinct* checkouts one PreRender may hold. It is
  // read here but only enforced against a growing set: a re-checkout replaces
  // its own registration below, so re-asking about the same layer cannot grow
  // into it.
  if (!checkout_id_registered(runtime, checkout_id) &&
      runtime.pixel_checkouts.size() >= kMaxPixelCheckouts)
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::CapacityExceeded);
  if (hosted != runtime.hosted_layers.end()) {
    if (!result)
      return finish_callback(Callback::PreCheckoutLayer, 4, Reason::InvalidArguments);
    if (!hosted->world)
      return finish_callback(Callback::PreCheckoutLayer, 4, Reason::MissingWorld);
    if (request)
      std::memcpy(runtime.map_checkout_request.data(), request,
                  sizeof(runtime.map_checkout_request));
    hosted->checkout_rect = answer_rect(hosted->width, hosted->height);
    write_world_extent_hint(hosted->view_world, hosted->checkout_rect);
    write_checkout_result(result, hosted->checkout_rect,
                          {0, 0, hosted->width, hosted->height},
                          hosted->width, hosted->height);
    hosted->checkout_id = checkout_id;
    forget_checkout(runtime, checkout_id);
    runtime.pixel_checkouts.push_back({checkout_id, hosted->world,
        hosted->view_world, hosted->checkout_rect, false});
    return finish_callback(Callback::PreCheckoutLayer, 0);
  }
  if (timed_slot && !(index == 0 && current_time))
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::UnknownLayer);
  if (request && index == 0)
    std::memcpy(runtime.input_checkout_request.data(), request,
                sizeof(runtime.input_checkout_request));
  if (request && index == runtime.secondary_layer_slot) {
    std::memcpy(runtime.map_checkout_request.data(), request,
                sizeof(runtime.map_checkout_request));
  }
  if (index == 0) {
    runtime.checkout_time = what_time;
    runtime.checkout_time_step = time_step;
    runtime.checkout_time_scale = time_scale;
  }
  if (!result)
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::InvalidArguments);
  if (index == 0) {
    // No `input_world` check: PreRender answers geometry, and a plug-in is
    // entitled to ask before any world exists to hand it. `checkout_pixels`
    // fails closed on a registration with no world, which is where a missing
    // one actually matters. Requiring it here refused the negotiation itself
    // (issue #675) and left `--self-test-pf-pre-checkout-result` and
    // `--self-test-smart-runtime-concurrency` failing, since neither sets one.
    const int32_t reference_width = runtime.full_resolution_width > 0
        ? runtime.full_resolution_width : runtime.width;
    const int32_t reference_height = runtime.full_resolution_height > 0
        ? runtime.full_resolution_height : runtime.height;
    runtime.input_checkout_result_rect = answer_rect(runtime.width, runtime.height);
    write_world_extent_hint(runtime.input_checkout_view_world,
                            runtime.input_checkout_result_rect);
    write_checkout_result(result, runtime.input_checkout_result_rect,
                          {0, 0, runtime.width, runtime.height},
                          reference_width, reference_height);
    forget_checkout(runtime, checkout_id);
    runtime.pixel_checkouts.push_back({checkout_id, runtime.input_world,
        runtime.input_checkout_view_world,
        runtime.input_checkout_result_rect, false});
    return finish_callback(Callback::PreCheckoutLayer, 0);
  }
  if (index == runtime.secondary_layer_slot && runtime.map_world) {
    runtime.map_checkout_result_rect = answer_rect(runtime.map_width, runtime.map_height);
    write_world_extent_hint(runtime.map_checkout_view_world,
                            runtime.map_checkout_result_rect);
    write_checkout_result(result, runtime.map_checkout_result_rect,
                          {0, 0, runtime.map_width, runtime.map_height},
                          runtime.map_width, runtime.map_height);
    runtime.secondary_checkout_id = checkout_id;
    forget_checkout(runtime, checkout_id);
    runtime.pixel_checkouts.push_back({checkout_id, runtime.map_world,
        runtime.map_checkout_view_world, runtime.map_checkout_result_rect,
        false});
    return finish_callback(Callback::PreCheckoutLayer, 0);
  }
  // This slot is not an arbitrary unset layer parameter: the render request
  // designates it as the external secondary input. If no world was supplied,
  // registering a synthetic transparent layer changes the plug-in's control
  // flow from "secondary unavailable" to "real transparent footage". Refuse
  // the checkout so effects with a procedural no-secondary fallback can use it;
  // other declared layer parameters still receive the empty-layer contract
  // below (issue #1243).
  if (index == runtime.secondary_layer_slot)
    return finish_callback(Callback::PreCheckoutLayer, 4, Reason::MissingWorld);
  // A layer parameter this host has no world for.
  //
  // The SDK admits this answer directly: PF_CheckoutResult::result_rect is
  // documented as "the rectangle actually available from this request (can be
  // empty)", and checkout_layer's index is "0 = input, 1..n = param", so
  // asking about a parameter is expected and an empty answer is a real one. A
  // plug-in reads the empty rect as "no layer here" and carries on; refusing
  // the call instead ends its PreRender, which is what happened to DeepGlow2
  // when it asked about a layer parameter its project leaves unset (issue
  // #898).
  //
  // The registration carries no world of its own; `checkout_pixels` answers the
  // follow-up from the dispatch's shared empty layer - see the paragraph there
  // for what shape that is and why (issues #958, #962).
  if (index >= 1 && index <= runtime.param_count) {
    ++runtime.empty_layer_param_checkouts;
    const std::array<int32_t, 4> empty{0, 0, 0, 0};
    write_checkout_result(result, empty, empty, runtime.width, runtime.height);
    forget_checkout(runtime, checkout_id);
    runtime.pixel_checkouts.push_back(
        {checkout_id, nullptr, nullptr, empty, false, true});
    return finish_callback(Callback::PreCheckoutLayer, 0);
  }
  return finish_callback(Callback::PreCheckoutLayer, 4, Reason::UnknownLayer);
}

int32_t __cdecl checkout_pixels(void*, int32_t checkout_id, void** world) {
  using callback_diagnostics::Callback;
  using callback_diagnostics::Reason;
  if (!g_active_state)
    return finish_callback(Callback::CheckoutPixels, 4, Reason::NoActiveState);
  if (!world)
    return finish_callback(Callback::CheckoutPixels, 4, Reason::InvalidArguments);
  *world = nullptr;
  auto& runtime = *g_active_state;
  const bool use_views = !runtime.gpu_render_dispatched;
  auto checkout = std::find_if(runtime.pixel_checkouts.begin(),
      runtime.pixel_checkouts.end(), [checkout_id](const auto& candidate) {
        return candidate.id == checkout_id;
      });
  if (checkout == runtime.pixel_checkouts.end())
    return finish_callback(Callback::CheckoutPixels, 4, Reason::UnknownCheckout);
  checkout->checkout_attempted = true;
  // A checkout PreRender already answered as empty. The plug-in was told there
  // are no pixels here (an empty result_rect, which the SDK documents as a real
  // answer), and asking for them anyway is not a fault on either side: AE hands
  // an effect a layer parameter with no source as an empty layer, not as a
  // failed call. Refusing instead ended DeepGlow2's SmartRender (issue #898).
  //
  // What comes back is an empty *layer*, not an empty answer: a real
  // PF_EffectWorld at the session's geometry with every pixel zero. A layer
  // parameter with no layer is transparent, not absent, and the two shapes that
  // tried to say "absent" each broke a real plug-in - a null pointer behind
  // PF_Err_NONE (3DGlasses answered PF_Err_BAD_CALLBACK_PARAM) and a 120-byte
  // zeroed world describing a 0x0 layer (DeepGlow2 answered
  // PF_Err_INTERNAL_STRUCT_DAMAGED). Both were shipped, in that order, and each
  // was measured only against the plug-in it was written for (issues #958,
  // #962).
  //
  // One world for all of a dispatch's empty parameters, handed out as many
  // times as it is asked for and not re-cleared between checkouts. Checked-out
  // layer pixels are the host's to read from, not the plug-in's to write to, so
  // sharing them is the same thing AE does with one "None" layer; a plug-in
  // that writes through this would see its own writes on the next empty
  // parameter, which is recorded on #962 rather than paid for with a
  // full-frame clear per checkout.
  //
  // The geometry deliberately does not match the empty rect PreRender answered
  // for the same checkout. Answering the session rect there instead was tried
  // and is worse: 3DGlasses fails at both, where with the empty rect it renders.
  // Why a plug-in reads the pair that way is an oracle question (#962).
  if (checkout->empty_layer_param) {
    // Allocated on first need, through the hook the dispatch installs: the
    // world comes out of the same bounded registry a plug-in's own
    // PF_NEW_WORLD draws from, so a frame that never asks for an empty layer
    // must not hold a full frame's worth of it. Without one there is nothing to
    // hand back that the host's other callbacks would accept, so fail closed.
    if (!runtime.empty_layer_world_live && runtime.allocate_empty_layer)
      runtime.empty_layer_world_live =
          runtime.allocate_empty_layer(runtime.empty_layer_world.data());
    if (!runtime.empty_layer_world_live)
      return finish_callback(Callback::CheckoutPixels, 4, Reason::MissingWorld);
    if (checkout->checked_out) {
      *world = runtime.empty_layer_world.data();
      return finish_callback(Callback::CheckoutPixels, 0);
    }
    ++runtime.empty_layer_param_pixel_checkouts;
    checkout->checked_out = true;
    checkout->ever_checked_out = true;
    *world = runtime.empty_layer_world.data();
    return finish_callback(Callback::CheckoutPixels, 0);
  }
  if (!checkout->world)
    return finish_callback(Callback::CheckoutPixels, 4, Reason::MissingWorld);
  if (checkout_promised_no_pixels(checkout->rect)) {
    ++runtime.empty_checkout_pixel_denials;
    return finish_callback(Callback::CheckoutPixels, 4, Reason::EmptyResult);
  }
  *world = use_views && checkout->view_world
      ? checkout->view_world : checkout->world;
  if (checkout->checked_out)
    return finish_callback(Callback::CheckoutPixels, 0);
  checkout->checked_out = true;
  checkout->ever_checked_out = true;
  return finish_callback(Callback::CheckoutPixels, 0);
}

int32_t __cdecl checkin_pixels(void*, int32_t checkout_id) {
  using callback_diagnostics::Callback;
  using callback_diagnostics::Reason;
  if (!g_active_state)
    return finish_callback(Callback::CheckinPixels, 4, Reason::NoActiveState);
  auto& runtime = *g_active_state;
  auto checkout = std::find_if(runtime.pixel_checkouts.begin(),
      runtime.pixel_checkouts.end(), [checkout_id](const auto& candidate) {
        return candidate.id == checkout_id;
      });
  if (checkout == runtime.pixel_checkouts.end())
    return finish_callback(Callback::CheckinPixels, 4, Reason::UnknownCheckout);
  if (!checkout->checked_out &&
      (checkout->ever_checked_out || checkout->checkout_attempted))
    return finish_callback(Callback::CheckinPixels, 4, Reason::NotCheckedOut);
  // Some effects retire every successful PreRender checkout id even when
  // SmartRender did not need that id's pixels. PathArray does this for an
  // empty alternate layer: rejecting the first checkin makes the otherwise
  // valid render fail. Consume the unused registration exactly once. Erasing
  // it keeps a second checkin and any later checkout fail-closed as stale,
  // while the ordinary checkout/checkin/re-checkout lifecycle below remains
  // reusable.
  if (!checkout->checked_out) {
    runtime.pixel_checkouts.erase(checkout);
    return finish_callback(Callback::CheckinPixels, 0);
  }
  checkout->checked_out = false;
  return finish_callback(Callback::CheckinPixels, 0);
}

bool pixel_checkouts_balanced() {
  if (!g_active_state) return true;
  const auto& runtime = *g_active_state;
  return std::none_of(runtime.pixel_checkouts.begin(),
      runtime.pixel_checkouts.end(),
      [](const auto& checkout) { return checkout.checked_out; });
}

int32_t __cdecl checkout_output(void*, void** world) {
  using callback_diagnostics::Callback;
  using callback_diagnostics::Reason;
  if (!g_active_state)
    return finish_callback(Callback::CheckoutOutput, 4, Reason::NoActiveState);
  if (!world)
    return finish_callback(Callback::CheckoutOutput, 4, Reason::InvalidArguments);
  *world = nullptr;
  auto& runtime = *g_active_state;
  if (!runtime.output_world)
    return finish_callback(Callback::CheckoutOutput, 4, Reason::MissingWorld);
  *world = runtime.output_world;
  return finish_callback(Callback::CheckoutOutput, 0);
}

bool concurrency_self_test() {
  std::atomic<int> ready{};
  std::atomic<bool> release{};
  std::array<bool, 2> passed{};
  std::array<std::shared_ptr<const Snapshot>, 2> snapshots;
  std::array<std::thread, 2> workers;
  for (int index = 0; index < 2; ++index) {
    workers[index] = std::thread([&, index] {
      Session session;
      snapshots[index] = session.snapshot();
      auto& runtime = state();
      runtime.width = 320 + index;
      runtime.height = 180 + index;
      runtime.current_time = 10 + index;
      runtime.current_time_scale = 24;
      runtime.pixel_aspect_numerator = 1 + index;
      runtime.pixel_aspect_denominator = 2 + index;
      runtime.wide_time_checkout_allowed = index != 0;
      runtime.shutter_dependency_advertised = index == 0;
      runtime.rejected_temporal_checkouts = 40 + index;
      runtime.output_world = reinterpret_cast<void*>(static_cast<uintptr_t>(index + 1));
      ++ready;
      while (!release.load(std::memory_order_acquire)) std::this_thread::yield();
      std::array<unsigned char, kCheckoutResultBytes> result{};
      void* output{};
      const int32_t checkout_status = pre_checkout_layer(
          nullptr, 0, 0, nullptr, 10 + index, 1, 24, result.data());
      int32_t result_width{}, result_height{}, par_numerator{};
      std::memcpy(&result_width, result.data() + 8, sizeof(result_width));
      std::memcpy(&result_height, result.data() + 12, sizeof(result_height));
      std::memcpy(&par_numerator, result.data() + 32, sizeof(par_numerator));
      passed[index] = checkout_status == 0 && checkout_output(nullptr, &output) == 0 &&
          output == runtime.output_world &&
          result_width == 320 + index && result_height == 180 + index &&
          par_numerator == 1 + index && width() == 320 + index &&
          height() == 180 + index;
    });
  }
  while (ready.load(std::memory_order_acquire) != 2) std::this_thread::yield();
  release.store(true, std::memory_order_release);
  for (auto& worker : workers) worker.join();
  void* cross_thread_output{};
  return passed[0] && passed[1] && snapshots[0] && snapshots[1] &&
      snapshots[0]->width == 320 && snapshots[1]->width == 321 &&
      snapshots[0]->rejected_temporal_checkouts == 40 &&
      snapshots[1]->rejected_temporal_checkouts == 41 &&
      snapshots[0]->shutter_dependency_advertised &&
      snapshots[1]->wide_time_checkout_allowed &&
      checkout_output(nullptr, &cross_thread_output) == 4 && width() == 0 &&
      height() == 0;
}

bool checkout_intersection_self_test() {
  Session session;
  auto& runtime = state();
  runtime.width = 640;
  runtime.height = 360;
  runtime.current_time = 7;
  runtime.current_time_scale = 30;
  aexcompat::world_safety::EffectWorldStorage input_world{}, input_view{};
  runtime.input_world = input_world.data();
  runtime.input_checkout_view_world = input_view.data();
  const auto verify = [&](const std::array<int32_t, 4>* requested,
                          int32_t expected_status,
                          const std::array<int32_t, 4>& expected) {
    // Every case runs in ONE PreRender scope, re-checking out the same id.
    // That is how a plug-in negotiates: the SDK gives it no other way to ask
    // what a given request rect would be answered with. Clearing between cases
    // here is what let a "already registered, refuse it" rule look correct
    // while it made real plug-ins abandon the frame (issue #675).
    std::array<std::byte, 44> request{};
    if (requested) std::memcpy(request.data(), requested->data(), sizeof(*requested));
    std::array<std::byte, kCheckoutResultBytes> result{};
    const int32_t status = pre_checkout_layer(nullptr, 0, 0,
        requested ? request.data() : nullptr, 7, 1, 30, result.data());
    if (status != expected_status) return false;
    if (status != 0) return true;
    std::array<int32_t, 4> actual{}, maximum{};
    std::memcpy(actual.data(), result.data(), sizeof(actual));
    std::memcpy(maximum.data(), result.data() + 16, sizeof(maximum));
    return actual == expected && maximum == std::array<int32_t, 4>{0, 0, 640, 360};
  };
  const std::array<int32_t, 4> partial{10, 20, 100, 200};
  const std::array<int32_t, 4> oversized{-50, -50, 10000, 10000};
  const std::array<int32_t, 4> disjoint{700, 400, 800, 500};
  const std::array<int32_t, 4> degenerate{5, 3, 5, 300};
  const std::array<int32_t, 4> inverted{100, 0, 10, 50};
  bool passed = verify(nullptr, 0, {0, 0, 640, 360}) &&
      verify(&partial, 0, partial) &&
      verify(&oversized, 0, {0, 0, 640, 360}) &&
      verify(&disjoint, 0, {0, 0, 0, 0}) &&
      verify(&degenerate, 0, {0, 0, 0, 0});
  const uint32_t malformed_before = runtime.malformed_checkout_requests;
  passed = verify(&inverted, 4, {}) &&
      runtime.malformed_checkout_requests == malformed_before + 1 && passed;
  aexcompat::world_safety::EffectWorldStorage hosted_world{}, hosted_view{};
  runtime.hosted_layers.push_back({3, 0, 1, false, 50, 40, -1,
      hosted_world.data(), hosted_view.data(), {-1, -1, -1, -1}});
  const std::array<int32_t, 4> hosted_request_rect{10, 10, 60, 60};
  std::array<std::byte, 44> hosted_request{};
  std::memcpy(hosted_request.data(), hosted_request_rect.data(),
              sizeof(hosted_request_rect));
  std::array<std::byte, kCheckoutResultBytes> hosted_result{};
  passed = pre_checkout_layer(nullptr, 3, 7, hosted_request.data(), 7, 1, 30,
                              hosted_result.data()) == 0 && passed;
  std::array<int32_t, 4> hosted_answer{}, hosted_maximum{};
  std::memcpy(hosted_answer.data(), hosted_result.data(), sizeof(hosted_answer));
  std::memcpy(hosted_maximum.data(), hosted_result.data() + 16,
              sizeof(hosted_maximum));
  passed = hosted_answer == std::array<int32_t, 4>{10, 10, 50, 40} &&
      hosted_maximum == std::array<int32_t, 4>{0, 0, 50, 40} &&
      runtime.hosted_layers.front().checkout_rect == hosted_answer && passed;
  void* checked_out{};
  // Re-checking out an id answers the NEW geometry, leaves exactly one
  // registration, and that registration carries the new answer. Refusing the
  // second call is what issue #675 was: a plug-in re-asking for geometry got
  // PF_Err_OUT_OF_MEMORY and gave up on the frame. Asking with a different rect
  // is what separates "replaced" from "erased and re-pushed unchanged".
  const std::size_t registrations_before = runtime.pixel_checkouts.size();
  const std::array<int32_t, 4> narrower_rect{20, 15, 40, 30};
  std::array<std::byte, 44> narrower_request{};
  std::memcpy(narrower_request.data(), narrower_rect.data(), sizeof(narrower_rect));
  const auto registration_for = [&](int32_t id) {
    return std::find_if(runtime.pixel_checkouts.begin(),
        runtime.pixel_checkouts.end(),
        [id](const auto& checkout) { return checkout.id == id; });
  };
  // A slot that is neither hosted nor secondary is refused, and that refusal
  // must not take the registration standing above it.
  passed = pre_checkout_layer(nullptr, 9, 7, hosted_request.data(), 7, 1, 30,
                              hosted_result.data()) == 4 &&
      registration_for(7) != runtime.pixel_checkouts.end() && passed;
  // A non-empty registration without an admitted world remains a hard
  // failure; repeated-checkout idempotency must not turn missing backing into
  // a successful null answer.
  runtime.pixel_checkouts.push_back({99, nullptr, nullptr, partial, false, false});
  checked_out = input_world.data();
  passed = checkout_pixels(nullptr, 99, &checked_out) == 4 &&
      checked_out == nullptr && checkin_pixels(nullptr, 99) == 4 && passed;
  forget_checkout(runtime, 99);
  passed = pre_checkout_layer(nullptr, 3, 7, narrower_request.data(), 7, 1, 30,
                              hosted_result.data()) == 0 &&
      runtime.pixel_checkouts.size() == registrations_before &&
      registration_for(7) != runtime.pixel_checkouts.end() &&
      registration_for(7)->rect == narrower_rect &&
      pre_checkout_layer(nullptr, 3, 8, hosted_request.data(), 7, 1, 30,
                         hosted_result.data()) == 0 &&
      checkout_pixels(nullptr, 999, &checked_out) == 4 &&
      checkin_pixels(nullptr, 999) == 4 &&
      checkout_pixels(nullptr, 7, &checked_out) == 0 &&
      checked_out == hosted_view.data() &&
      checkout_pixels(nullptr, 7, &checked_out) == 0 &&
      checked_out == hosted_view.data() &&
      !pixel_checkouts_balanced() &&
      checkin_pixels(nullptr, 7) == 0 &&
      pixel_checkouts_balanced() &&
      checkin_pixels(nullptr, 7) == 4 &&
      checkout_pixels(nullptr, 7, &checked_out) == 0 &&
      checkin_pixels(nullptr, 7) == 0 &&
      checkout_pixels(nullptr, 8, &checked_out) == 0 &&
      checkin_pixels(nullptr, 8) == 0 && passed;
  runtime.hosted_layers.clear();
  passed = pre_checkout_layer(nullptr, 0, 0, nullptr, 7, 1, 30,
                              hosted_result.data()) == 0 && passed;
  runtime.input_checkout_result_rect = {0, 0, 0, 0};
  runtime.pixel_checkouts.back().rect = runtime.input_checkout_result_rect;
  const uint32_t denials_before = runtime.empty_checkout_pixel_denials;
  checked_out = nullptr;
  passed = checkout_pixels(nullptr, 0, &checked_out) == 4 && !checked_out &&
      runtime.empty_checkout_pixel_denials == denials_before + 1 && passed;
  runtime.input_checkout_result_rect = partial;
  runtime.pixel_checkouts.back().rect = runtime.input_checkout_result_rect;
  write_world_extent_hint(runtime.input_checkout_view_world, partial);
  checked_out = nullptr;
  passed = checkout_pixels(nullptr, 0, &checked_out) == 0 &&
      checked_out == input_view.data() &&
      !pixel_checkouts_balanced() &&
      checkin_pixels(nullptr, 0) == 0 &&
      pixel_checkouts_balanced() && passed;
  runtime.gpu_render_dispatched = true;
  checked_out = nullptr;
  passed = checkout_pixels(nullptr, 0, &checked_out) == 0 &&
      checked_out == input_world.data() &&
      checkin_pixels(nullptr, 0) == 0 &&
      pixel_checkouts_balanced() && passed;

  // A layer parameter this host has no world for: PreRender answers an empty
  // rect and SmartRender hands back the session's empty layer. Both halves are
  // pinned because both are what a plug-in reads, and the pair decides whether
  // a real effect renders. Two other shapes stood here and each broke one:
  // a null pointer behind PF_Err_NONE (3DGlasses) and a 120-byte zeroed world
  // describing a 0x0 layer (DeepGlow2 answered PF_Err_INTERNAL_STRUCT_DAMAGED
  // for the whole frame). Nothing pinned the shape while it changed twice
  // (issues #958, #962).
  runtime.gpu_render_dispatched = false;
  runtime.param_count = 9;
  std::array<std::byte, kCheckoutResultBytes> empty_result{};
  const uint32_t empty_before = runtime.empty_layer_param_checkouts;
  const uint32_t empty_pixels_before = runtime.empty_layer_param_pixel_checkouts;
  passed = pre_checkout_layer(nullptr, 5, 21, nullptr, 7, 1, 30,
                              empty_result.data()) == 0 &&
      runtime.empty_layer_param_checkouts == empty_before + 1 && passed;
  std::array<int32_t, 4> empty_answer{1, 1, 1, 1}, empty_maximum{1, 1, 1, 1};
  std::memcpy(empty_answer.data(), empty_result.data(), sizeof(empty_answer));
  std::memcpy(empty_maximum.data(), empty_result.data() + 16,
              sizeof(empty_maximum));
  const std::array<int32_t, 4> nothing{0, 0, 0, 0};
  // No hook and no layer - the dispatch installs the one and it allocates the
  // other - so the checkout fails closed rather than answering with a pointer
  // to an uninitialized struct.
  checked_out = input_world.data();
  passed = empty_answer == nothing && empty_maximum == nothing &&
      !runtime.empty_layer_world_live && !runtime.allocate_empty_layer &&
      checkout_pixels(nullptr, 21, &checked_out) == 4 && passed;
  // With the hook, the checkout allocates on first need and spends the checkout
  // like any other: an empty layer is an answer, not a skipped call. The hook
  // stands in for the dispatch's allocation here; what it does with the storage
  // is the registry's business, and what this pins is that the runtime asks for
  // one exactly once and hands back what came of it.
  g_empty_layer_allocations_for_self_test = 0;
  runtime.allocate_empty_layer = +[](void* storage) {
    ++g_empty_layer_allocations_for_self_test;
    std::memset(storage, 0, 120);
    return true;
  };
  checked_out = nullptr;
  passed = checkout_pixels(nullptr, 21, &checked_out) == 0 &&
      checked_out == runtime.empty_layer_world.data() &&
      runtime.empty_layer_world_live &&
      g_empty_layer_allocations_for_self_test == 1 &&
      runtime.empty_layer_param_pixel_checkouts == empty_pixels_before + 1 &&
      checkout_pixels(nullptr, 21, &checked_out) == 0 &&
      checked_out == runtime.empty_layer_world.data() &&
      runtime.empty_layer_param_pixel_checkouts == empty_pixels_before + 1 &&
      !pixel_checkouts_balanced() &&
      checkin_pixels(nullptr, 21) == 0 &&
      pixel_checkouts_balanced() && passed;
  // The configured secondary input is not an arbitrary unset parameter. When
  // no secondary world was supplied, refusing its checkout leaves the effect
  // free to use its own procedural fallback instead of presenting a synthetic
  // transparent input as real footage.
  checked_out = input_world.data();
  passed = pre_checkout_layer(nullptr, 6, 24, nullptr, 7, 1, 30,
                              empty_result.data()) == 4 &&
      checkout_pixels(nullptr, 24, &checked_out) == 4 && !checked_out && passed;
  // A second ordinary empty parameter reuses the lazily allocated world rather
  // than allocating again: the registry it comes from is bounded and shared
  // with the plug-in's own worlds.
  passed = pre_checkout_layer(nullptr, 8, 24, nullptr, 7, 1, 30,
                              empty_result.data()) == 0 &&
      checkout_pixels(nullptr, 24, &checked_out) == 0 &&
      g_empty_layer_allocations_for_self_test == 1 &&
      checkin_pixels(nullptr, 24) == 0 && passed;
  // A successful PreRender registration may be retired without a pixel
  // checkout. This is the exact PathArray shape: it registers an empty
  // alternate layer, uses only its other layers in SmartRender, then checks
  // every registered id back in. The first checkin consumes the registration;
  // a duplicate checkin and a later checkout remain stale failures, and no
  // empty world was allocated for pixels that were never requested.
  const int allocations_before_unused_checkin =
      g_empty_layer_allocations_for_self_test;
  passed = pre_checkout_layer(nullptr, 9, 26, nullptr, 7, 1, 30,
                              empty_result.data()) == 0 &&
      checkin_pixels(nullptr, 26) == 0 &&
      checkin_pixels(nullptr, 26) == 4 &&
      checkout_pixels(nullptr, 26, &checked_out) == 4 &&
      g_empty_layer_allocations_for_self_test ==
          allocations_before_unused_checkin &&
      passed;
  // A hook that cannot allocate leaves the checkout refused, not answered.
  runtime.empty_layer_world_live = false;
  runtime.allocate_empty_layer = +[](void*) { return false; };
  checked_out = input_world.data();
  passed = pre_checkout_layer(nullptr, 7, 25, nullptr, 7, 1, 30,
                              empty_result.data()) == 0 &&
      checkout_pixels(nullptr, 25, &checked_out) == 4 && passed;
  runtime.allocate_empty_layer = nullptr;
  runtime.empty_layer_world_live = false;
  // A parameter index past what the plug-in declared is still unknown; the
  // empty answer is for the range it did declare, not for anything asked.
  passed = pre_checkout_layer(nullptr, 5, 22, nullptr, 7, 1, 30,
                              empty_result.data()) == 0 &&
      pre_checkout_layer(nullptr, 10, 23, nullptr, 7, 1, 30,
                         empty_result.data()) == 4 && passed;
  return passed;
}

}  // namespace aexcompat::worker_runtime::smart
