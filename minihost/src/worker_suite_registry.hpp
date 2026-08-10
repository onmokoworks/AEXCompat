#pragma once

#include "suite_lease_tracker.hpp"

#include <atomic>
#include <array>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

namespace aexcompat {
class TraceWriter;
}

namespace aexcompat::worker_runtime {

enum class SuiteResolveResult {
  acquired,
  rejected_bad_param,
  not_found,
};

using SuiteResolver = SuiteResolveResult (*)(
    void* context, const char* name, int32_t version, const void** suite);

enum class UnsupportedSuiteId : uint8_t {
  aegp_proj_9,
  aegp_item_14,
  aegp_item_13,
  aegp_item_10,
  aegp_item_3,
  aegp_comp_25,
  aegp_comp_26,
  aegp_comp_21,
  aegp_comp_9,
  aegp_comp_4,
  aegp_layer_15,
  aegp_layer_11,
  aegp_layer_14,
  aegp_layer_13,
  aegp_layer_5,
  aegp_layer_8,
  aegp_collection_2,
  aegp_effect_4,
  aegp_effect_2,
  aegp_effect_3,
  aegp_stream_11,
  aegp_stream_7,
  aegp_stream_8,
  aegp_stream_4,
  aegp_iterate_1,
  aegp_keyframe_5,
  aegp_utility_5,
  aegp_utility_7,
  aegp_utility_13,
  pf_ae_adv_app_1,
  pf_ae_adv_app_2,
  drawbot_supplier_1,
  drawbot_surface_2,
  drawbot_path_1,
  pf_effect_custom_ui_overlay_theme_1,
  aegp_dynamic_stream_2,
  pf_batch_sampling_1,
  aefx_ace_1,
};

int32_t record_unsupported_suite_call(UnsupportedSuiteId suite,
                                      uint32_t slot) noexcept;

template <UnsupportedSuiteId Suite, std::size_t Slot>
int32_t __cdecl unsupported_suite_slot() noexcept {
  return record_unsupported_suite_call(Suite, static_cast<uint32_t>(Slot));
}

template <UnsupportedSuiteId Suite, std::size_t... Slots>
std::array<void*, sizeof...(Slots)> make_unsupported_suite_slots(
    std::index_sequence<Slots...>) {
  return {{reinterpret_cast<void*>(&unsupported_suite_slot<Suite, Slots>)...}};
}

template <UnsupportedSuiteId Suite, std::size_t SlotCount>
const std::array<void*, SlotCount>& unsupported_suite_slots() {
  static const auto slots = make_unsupported_suite_slots<Suite>(
      std::make_index_sequence<SlotCount>{});
  return slots;
}

class SuiteRegistry final {
 public:
  int32_t acquire(const char* name, int32_t version, const void** suite,
                  SuiteResolver resolver, void* resolver_context,
                  TraceWriter* trace_writer);
  int32_t release(const char* name, int32_t version,
                  TraceWriter* trace_writer);

  bool balanced() const;
  std::size_t live_lease_count() const;
  uint32_t live_reference_count() const;
  uint32_t acquire_count() const;
  uint32_t release_count() const;
  uint32_t rejected_release_count() const;
  std::string live_summary() const;
  suite_runtime::SuiteLeaseSnapshot snapshot() const;
  uint32_t release_since(
      const suite_runtime::SuiteLeaseSnapshot& baseline,
      TraceWriter* trace_writer);
  uint32_t force_release_all() noexcept;
  std::string missing_suites_report_json() const;
  std::string unsupported_suite_calls_report_json() const;
  std::string suite_timeline_report_json() const;

  void note_unsupported_suite_call(UnsupportedSuiteId suite,
                                   uint32_t slot) noexcept;

 private:
  static std::string safe_missing_name(const char* name);
  void record_suite_timeline(bool acquire, const char* name,
                             std::size_t name_length, bool valid_name,
                             int32_t version, int32_t result);
  void record_missing_suite(const std::string& name, int32_t version);
  int32_t reject_unknown(const char* name, int32_t version,
                         TraceWriter* trace_writer);

  suite_runtime::SuiteLeaseTracker lease_tracker_;
  std::atomic<uint32_t> rejected_releases_{};
  mutable std::mutex missing_suites_mutex_;
  std::vector<std::pair<std::string, int32_t>> missing_suites_;
  bool missing_suites_truncated_{};
  struct UnsupportedSuiteCall {
    UnsupportedSuiteId suite{};
    uint32_t slot{};
    uint32_t call_count{};
  };
  mutable std::mutex unsupported_suite_calls_mutex_;
  std::vector<UnsupportedSuiteCall> unsupported_suite_calls_;
  bool unsupported_suite_calls_truncated_{};
  struct SuiteTimelineEvent {
    uint32_t sequence{};
    bool acquire{};
    std::string name;
    int32_t version{};
    std::string selector;
    int32_t result{};
  };
  mutable std::mutex timeline_mutex_;
  std::vector<SuiteTimelineEvent> suite_timeline_;
  bool suite_timeline_truncated_{};
};

SuiteRegistry& suite_registry();
const char* set_suite_timeline_selector(const char* selector) noexcept;
const char* current_suite_timeline_selector() noexcept;

}  // namespace aexcompat::worker_runtime
