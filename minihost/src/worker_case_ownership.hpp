#pragma once
#include <algorithm>
#include <cstdint>
#include <utility>
#include <vector>

namespace aexcompat::worker_runtime {
// Compare outstanding ownership, not cumulative allocation counters. Module-owned
// defaults and caches may remain resident. Retired tombstones are not ownership.
using CaseOwnership = std::vector<std::pair<uintptr_t, uint64_t>>;
}
namespace aexcompat::worker_runtime::handles { aexcompat::worker_runtime::CaseOwnership case_memory_ownership(); }
namespace aexcompat::pf_path_runtime { aexcompat::worker_runtime::CaseOwnership case_path_ownership(); }
namespace aexcompat::world_registry { aexcompat::worker_runtime::CaseOwnership case_world_ownership(); }
namespace aexcompat::render_options { aexcompat::worker_runtime::CaseOwnership case_options_ownership(); }
namespace aexcompat::render_receipts { aexcompat::worker_runtime::CaseOwnership case_receipt_ownership(); }
namespace aexcompat::gpu_runtime::memory_world_transport { aexcompat::worker_runtime::CaseOwnership case_gpu_ownership(); }
namespace aexcompat::extended_inter { aexcompat::worker_runtime::CaseOwnership case_inter_ownership(); }
namespace aexcompat::color_settings { aexcompat::worker_runtime::CaseOwnership case_profile_ownership(); }
namespace aexcompat::l2_detail { aexcompat::worker_runtime::CaseOwnership case_drawbot_ownership(); }
namespace aexcompat::l2_detail { aexcompat::worker_runtime::CaseOwnership case_mask_ownership(); }
namespace aexcompat::scene_runtime { aexcompat::worker_runtime::CaseOwnership case_scene_ownership(); }
namespace aexcompat::worker_runtime::compute_cache { aexcompat::worker_runtime::CaseOwnership case_cache_ownership(); }
namespace aexcompat::aegp_async_layer { aexcompat::worker_runtime::CaseOwnership case_async_ownership(); }
namespace aexcompat::host_audio { aexcompat::worker_runtime::CaseOwnership case_audio_ownership(); }
aexcompat::worker_runtime::CaseOwnership case_sampling_ownership();

namespace aexcompat::worker_runtime {
inline std::vector<CaseOwnership> case_ownership() {
  return {
      aexcompat::worker_runtime::handles::case_memory_ownership(),
      aexcompat::pf_path_runtime::case_path_ownership(),
      aexcompat::world_registry::case_world_ownership(),
      aexcompat::render_options::case_options_ownership(),
      aexcompat::render_receipts::case_receipt_ownership(),
      aexcompat::gpu_runtime::memory_world_transport::case_gpu_ownership(),
      aexcompat::extended_inter::case_inter_ownership(),
      aexcompat::color_settings::case_profile_ownership(),
      aexcompat::l2_detail::case_drawbot_ownership(),
      aexcompat::l2_detail::case_mask_ownership(),
      aexcompat::scene_runtime::case_scene_ownership(),
      aexcompat::worker_runtime::compute_cache::case_cache_ownership(),
      aexcompat::aegp_async_layer::case_async_ownership(),
      aexcompat::host_audio::case_audio_ownership(),
      ::case_sampling_ownership()};
}
}
