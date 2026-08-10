#pragma once

#include <cstdint>

namespace aexcompat::l2_detail {

// PF parameter checkout/checkin callbacks and their balance accounting,
// owned beside the checkout ledger in worker_runtime::parameters::state()
// (issue #170). Classic dispatch contexts take precedence over the hosted
// ledger exactly as they did in worker_main.
int32_t __cdecl checkout_param(void*, int32_t index, int32_t what_time, int32_t time_step,
                               uint32_t time_scale, void* definition);
int32_t __cdecl checkin_param(void*, void* definition);

// The frame and WIDE_TIME_INPUT declaration associated with hosted checkouts.
// The declaration informs dependency/cache policy; checkout_param evaluates a
// requested temporal value regardless of that flag. The classic path has its
// equivalent state on its dispatch context (`classic::Context::configure_checkout_time`).
// Call this before the frame's first selector reaches the plug-in, not just before
// SMART_PRE_RENDER: QUERY_DYNAMIC_FLAGS is allowed to check parameters out too.
void configure_hosted_checkout_time(int32_t current_time, uint32_t time_scale,
                                    bool wide_time_allowed) noexcept;
bool param_checkouts_balanced();
void automatic_checkin_pre_render_params();

}  // namespace aexcompat::l2_detail
