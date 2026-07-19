#pragma once
#include "worker_pf_path_runtime.hpp"

namespace aexcompat::l2_detail {
bool verify_pf_path_data_hardening(aexcompat::pf_path_runtime::HostHooks path_hooks,
    aexcompat::mask_runtime::HostContext mask_hooks);
}
