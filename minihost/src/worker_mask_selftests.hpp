#pragma once

namespace aexcompat::l2_detail {

bool verify_keyframe_ownership_rejection();
bool verify_dynamic_stream_tree_rejection();
bool verify_mask_double_dispose_rejected();
bool verify_stream_dispose_with_live_value_rejected();
bool verify_stream_metadata_and_ownership_rejection();
bool verify_outline_mutation_rejection();
bool verify_mask_attribute_and_ownership_rejection();

}  // namespace aexcompat::l2_detail
