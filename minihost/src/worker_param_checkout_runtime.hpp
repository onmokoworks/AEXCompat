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

// The frame the hosted (non-classic) ledger answers checkouts for. `checkout_param`
// refuses any other time unless the plug-in advertised wide time input, and until
// issue #828 nothing set this: the ledger kept its default current_time 0 /
// time_scale 1, so every SmartFX frame past t=0 had its parameter checkouts
// refused with PF_Err_OUT_OF_MEMORY. The classic path has always configured the
// equivalent state on its dispatch context (`classic::Context::configure_checkout_time`).
// Call this before the frame's first selector reaches the plug-in, not just before
// SMART_PRE_RENDER: QUERY_DYNAMIC_FLAGS is allowed to check parameters out too.
// A zero `time_scale` leaves the gate admitting nothing rather than being taken as
// a scale of 1.
void configure_hosted_checkout_time(int32_t current_time, uint32_t time_scale,
                                    bool wide_time_allowed) noexcept;
bool param_checkouts_balanced();
void automatic_checkin_pre_render_params();

}  // namespace aexcompat::l2_detail
