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
bool param_checkouts_balanced();
void automatic_checkin_pre_render_params();

}  // namespace aexcompat::l2_detail
