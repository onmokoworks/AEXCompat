#pragma once
namespace aexcompat::l2_detail {
bool verify_pre_checkout_result_contract();
bool verify_handle_resize_while_locked_rejected();
bool verify_utils_handle_callbacks_wired();
bool verify_aegp_memory_and_strings_rejection();
bool verify_aegp_keyframe_suite5_mutations(bool abi_wiring);
}
