#include "worker_suite_registry.hpp"
#include "worker_suite_call_slot_probe.hpp"
#include "worker_selector_dispatch.hpp"

#include <windows.h>

#include <array>
#include <cstring>
#include <iostream>
#include <limits>
#include <string>

namespace {

using aexcompat::worker_runtime::SuiteResolveResult;
using aexcompat::worker_runtime::UnsupportedSuiteId;
using aexcompat::worker_runtime::unsupported_suite_slots;

int g_resolver_calls{};
void* g_synthetic_global_data_output{};

constexpr std::size_t kSyntheticInGlobalDataOffset = 312;
constexpr std::size_t kSyntheticOutGlobalDataOffset = 40;
constexpr std::size_t kSyntheticInEffectRefOffset = 184;
constexpr std::size_t kSyntheticInSpecVersionOffset = 196;
constexpr std::size_t kSyntheticInApplicationIdOffset = 204;

void capture_audit() {}
bool audit_passed() { return true; }

using OpaqueTableClassification =
    aexcompat::worker_runtime::ExtendedLookupOpaqueTableClassification;

OpaqueTableClassification classify_as_sealed(void*) noexcept {
  return OpaqueTableClassification::other_loaded_sealed_module;
}

OpaqueTableClassification classify_as_system(void*) noexcept {
  return OpaqueTableClassification::other_loaded_system_module;
}

OpaqueTableClassification classify_as_invalid_active(void*) noexcept {
  return OpaqueTableClassification::active_effect_module;
}

void write_global_data(std::array<std::byte, 408>& buffer,
                       std::size_t offset, void* value) {
  std::memcpy(buffer.data() + offset, &value, sizeof(value));
}

void write_application_id(
    std::array<std::byte, 408>& buffer, uint32_t value) {
  std::memcpy(
      buffer.data() + kSyntheticInApplicationIdOffset,
      &value, sizeof(value));
}

uint32_t pack_spec_version(int16_t major, int16_t minor) {
  return static_cast<uint32_t>(static_cast<uint16_t>(major)) |
      (static_cast<uint32_t>(static_cast<uint16_t>(minor)) << 16);
}

void write_spec_version(
    std::array<std::byte, 408>& buffer, int16_t major, int16_t minor) {
  const uint32_t packed = pack_spec_version(major, minor);
  std::memcpy(
      buffer.data() + kSyntheticInSpecVersionOffset,
      &packed, sizeof(packed));
}

__declspec(noinline) int32_t synthetic_global_data_entry(
    int32_t command, void*, void* output, void**, void*, void*) {
  if (command == 1 && output) {
    std::memcpy(static_cast<std::byte*>(output) +
                    kSyntheticOutGlobalDataOffset,
                &g_synthetic_global_data_output,
                sizeof(g_synthetic_global_data_output));
  }
  return 0;
}

__declspec(noinline) int32_t normal_return_512(
    int32_t, void*, void*, void**, void*, void*) {
  return 512;
}

__declspec(noinline) int32_t trapped_read_null(
    int32_t, void*, void*, void**, void*, void*) {
  return *static_cast<volatile int*>(nullptr);
}

__declspec(noinline) int32_t trapped_read_low(
    int32_t, void*, void*, void**, void*, void*) {
  return *reinterpret_cast<volatile int*>(static_cast<uintptr_t>(0x20));
}

__declspec(noinline) int32_t trapped_write_module(
    int32_t, void*, void*, void**, void*, void*) {
  HMODULE kernel32 = GetModuleHandleW(L"kernel32.dll");
  FARPROC target = kernel32
      ? GetProcAddress(kernel32, "GetCurrentProcessId") : nullptr;
  if (!target) return -1;
  auto* byte = reinterpret_cast<volatile unsigned char*>(target);
  const unsigned char value = *byte;
  *byte = value;
  return 0;
}

SuiteResolveResult resolve_known(void*, const char* name, int32_t version,
                                 const void** suite) {
  ++g_resolver_calls;
  if (std::strcmp(name, "Known Suite") == 0 && version == 1) {
    *suite = reinterpret_cast<const void*>(0x1234);
    return SuiteResolveResult::acquired;
  }
  return SuiteResolveResult::not_found;
}

bool rejected_without_resolving(aexcompat::worker_runtime::SuiteRegistry& registry,
                                const char* name) {
  const int calls_before = g_resolver_calls;
  const void* suite = reinterpret_cast<const void*>(0x5678);
  return registry.acquire(name, 1, &suite, &resolve_known, nullptr, nullptr) == 4 &&
      suite == nullptr && g_resolver_calls == calls_before &&
      registry.release(name, 1, nullptr) == 1;
}

std::size_t count_occurrences(const std::string& text,
                              const std::string& needle) {
  std::size_t count{};
  for (std::size_t offset{}; (offset = text.find(needle, offset)) !=
                             std::string::npos; offset += needle.size()) {
    ++count;
  }
  return count;
}

uint32_t invoke_probe_slot(const void* table, std::size_t slot) {
  using ProbeCallback = int32_t(__cdecl*)(
      uintptr_t, uintptr_t, uintptr_t, uintptr_t,
      uintptr_t, uintptr_t, uintptr_t, uintptr_t);
  const auto* slots = static_cast<void* const*>(table);
  __try {
    reinterpret_cast<ProbeCallback>(slots[slot])(
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88);
    return 0;
  } __except(EXCEPTION_EXECUTE_HANDLER) {
    return GetExceptionCode();
  }
}

bool verify_global_data_handoff_diagnostics() {
  using namespace aexcompat::worker_runtime;
  auto& telemetry = selector_dispatch_telemetry();
  std::byte first_state{};
  std::byte second_state{};
  const auto verify_case =
      [&](void* global_output, void* next_input, bool expected_same) {
        telemetry.invocations.clear();
        telemetry.invocations_truncated = false;
        std::array<std::byte, 408> input{};
        std::array<std::byte, 408> output{};
        g_synthetic_global_data_output = global_output;
        uint32_t global_exception = 1;
        uint32_t params_exception = 1;
        const int32_t global_result = invoke_entry_seh(
            &synthetic_global_data_entry, 1, input.data(), output.data(),
            nullptr, nullptr, nullptr, &global_exception);
        write_global_data(input, kSyntheticInGlobalDataOffset, next_input);
        const int32_t params_result = invoke_entry_seh(
            &synthetic_global_data_entry, 4, input.data(), output.data(),
            nullptr, nullptr, nullptr, &params_exception);
        if (global_result != 0 || params_result != 0 ||
            global_exception != 0 || params_exception != 0 ||
            telemetry.invocations.size() != 2)
          return false;
        const auto& global =
            telemetry.invocations[0].global_data_handoff;
        const auto& params =
            telemetry.invocations[1].global_data_handoff;
        const bool null_case = global_output == nullptr;
        const bool output_state_valid =
            global.output_after_return.captured &&
            global.output_after_return.is_null == null_case &&
            global.output_after_return.classification ==
                (null_case ? "null" : "heap_or_unknown") &&
            (null_case
                 ? global.output_after_return.process_local_token.empty()
                 : !global.output_after_return.process_local_token.empty());
        const bool input_state_valid =
            params.input_at_entry.captured &&
            params.input_at_entry.is_null == (next_input == nullptr) &&
            params.has_previous_output &&
            params.same_identity_as_previous_output == expected_same;
        const bool token_relation =
            null_case
                ? params.input_at_entry.process_local_token.empty()
                : expected_same
                    ? params.input_at_entry.process_local_token ==
                          global.output_after_return.process_local_token
                    : params.input_at_entry.process_local_token !=
                          global.output_after_return.process_local_token;
        const std::string report = selector_invocations_report_json();
        return output_state_valid && input_state_valid && token_relation &&
            report.find("\"global_data_handoff\":{") !=
                std::string::npos &&
            report.find("\"process_local_token\":") !=
                std::string::npos &&
            report.find("\"same_identity_as_previous_output\":") !=
                std::string::npos;
      };
  return verify_case(nullptr, nullptr, true) &&
      verify_case(&first_state, &first_state, true) &&
      verify_case(&first_state, &second_state, false);
}

bool verify_effect_ref_entry_diagnostics() {
  using namespace aexcompat::worker_runtime;
  auto& telemetry = selector_dispatch_telemetry();
  std::byte first_ref{};
  std::byte second_ref{};
  const auto verify_case =
      [&](void* global_ref, void* params_ref, bool expected_same) {
        telemetry.invocations.clear();
        telemetry.invocations_truncated = false;
        std::array<std::byte, 408> input{};
        std::array<std::byte, 408> output{};
        write_global_data(
            input, kSyntheticInEffectRefOffset, global_ref);
        uint32_t global_exception = 1;
        uint32_t params_exception = 1;
        const int32_t global_result = invoke_entry_seh(
            &synthetic_global_data_entry, 1, input.data(), output.data(),
            nullptr, nullptr, nullptr, &global_exception);
        write_global_data(
            input, kSyntheticInEffectRefOffset, params_ref);
        const int32_t params_result = invoke_entry_seh(
            &synthetic_global_data_entry, 4, input.data(), output.data(),
            nullptr, nullptr, nullptr, &params_exception);
        if (global_result != 0 || params_result != 0 ||
            global_exception != 0 || params_exception != 0 ||
            telemetry.invocations.size() != 2)
          return false;
        const auto& global =
            telemetry.invocations[0].effect_ref_at_entry;
        const auto& params =
            telemetry.invocations[1].effect_ref_at_entry;
        const bool null_case = global_ref == nullptr;
        const bool global_state_valid =
            global.state.captured &&
            global.state.is_null == null_case &&
            global.state.classification ==
                (null_case ? "null" : "heap_or_unknown") &&
            !global.has_global_setup_entry;
        const bool params_state_valid =
            params.state.captured &&
            params.state.is_null == (params_ref == nullptr) &&
            params.has_global_setup_entry &&
            params.same_identity_as_global_setup_entry == expected_same;
        const bool token_relation =
            null_case
                ? params.state.process_local_token.empty()
                : expected_same
                    ? params.state.process_local_token ==
                          global.state.process_local_token
                    : params.state.process_local_token !=
                          global.state.process_local_token;
        const std::string report = selector_invocations_report_json();
        return global_state_valid && params_state_valid && token_relation &&
            report.find("\"effect_ref_at_entry\":{") !=
                std::string::npos &&
            report.find(
                "\"same_identity_as_global_setup_entry\":") !=
                std::string::npos;
      };
  return verify_case(nullptr, nullptr, true) &&
      verify_case(&first_ref, &first_ref, true) &&
      verify_case(&first_ref, &second_ref, false);
}

bool verify_application_id_entry_diagnostics() {
  using namespace aexcompat::worker_runtime;
  auto& telemetry = selector_dispatch_telemetry();
  constexpr uint32_t kAfterEffectsId = 0x46585443u;
  constexpr uint32_t kUnexpectedId = 0x00010203u;
  constexpr uint32_t kChangedId = 0x50724d72u;
  const auto verify_case =
      [&](uint32_t global_id, uint32_t params_id, bool expected_same,
          const char* expected_global_code,
          const char* expected_params_code) {
        telemetry.invocations.clear();
        telemetry.invocations_truncated = false;
        std::array<std::byte, 408> input{};
        std::array<std::byte, 408> output{};
        write_application_id(input, global_id);
        uint32_t global_exception = 1;
        uint32_t params_exception = 1;
        const int32_t global_result = invoke_entry_seh(
            &synthetic_global_data_entry, 1, input.data(), output.data(),
            nullptr, nullptr, nullptr, &global_exception);
        write_application_id(input, params_id);
        const int32_t params_result = invoke_entry_seh(
            &synthetic_global_data_entry, 4, input.data(), output.data(),
            nullptr, nullptr, nullptr, &params_exception);
        if (global_result != 0 || params_result != 0 ||
            global_exception != 0 || params_exception != 0 ||
            telemetry.invocations.size() != 2)
          return false;
        const auto& global =
            telemetry.invocations[0].appl_id_at_entry;
        const auto& params =
            telemetry.invocations[1].appl_id_at_entry;
        const std::string report = selector_invocations_report_json();
        return global.captured && global.value == global_id &&
            global.printable_code == expected_global_code &&
            !global.has_global_setup_entry &&
            params.captured && params.value == params_id &&
            params.printable_code == expected_params_code &&
            params.has_global_setup_entry &&
            params.same_value_as_global_setup_entry == expected_same &&
            report.find("\"appl_id_at_entry\":{") !=
                std::string::npos &&
            report.find(
                "\"host_setting_source\":\"worker_effect_bootstrap\"") !=
                std::string::npos;
      };
  return verify_case(
             kAfterEffectsId, kAfterEffectsId, true, "FXTC", "FXTC") &&
      verify_case(
             kUnexpectedId, kUnexpectedId, true,
             "\\x00\\x01\\x02\\x03", "\\x00\\x01\\x02\\x03") &&
      verify_case(
             kAfterEffectsId, kChangedId, false, "FXTC", "PrMr");
}

bool verify_spec_version_entry_diagnostics() {
  using namespace aexcompat::worker_runtime;
  auto& telemetry = selector_dispatch_telemetry();
  const auto verify_case =
      [&](int16_t global_major, int16_t global_minor,
          int16_t params_major, int16_t params_minor,
          bool expected_same) {
        telemetry.invocations.clear();
        telemetry.invocations_truncated = false;
        std::array<std::byte, 408> input{};
        std::array<std::byte, 408> output{};
        write_spec_version(input, global_major, global_minor);
        uint32_t global_exception = 1;
        uint32_t params_exception = 1;
        const int32_t global_result = invoke_entry_seh(
            &synthetic_global_data_entry, 1, input.data(), output.data(),
            nullptr, nullptr, nullptr, &global_exception);
        write_spec_version(input, params_major, params_minor);
        const int32_t params_result = invoke_entry_seh(
            &synthetic_global_data_entry, 4, input.data(), output.data(),
            nullptr, nullptr, nullptr, &params_exception);
        if (global_result != 0 || params_result != 0 ||
            global_exception != 0 || params_exception != 0 ||
            telemetry.invocations.size() != 2)
          return false;
        const auto& global =
            telemetry.invocations[0].version_at_entry;
        const auto& params =
            telemetry.invocations[1].version_at_entry;
        const std::string report = selector_invocations_report_json();
        return global.captured &&
            global.raw_packed_value ==
                pack_spec_version(global_major, global_minor) &&
            global.major == global_major && global.minor == global_minor &&
            !global.has_global_setup_entry &&
            params.captured &&
            params.raw_packed_value ==
                pack_spec_version(params_major, params_minor) &&
            params.major == params_major && params.minor == params_minor &&
            params.has_global_setup_entry &&
            params.same_value_as_global_setup_entry == expected_same &&
            report.find("\"version_at_entry\":{") !=
                std::string::npos &&
            report.find("\"raw_packed_u32\":\"0x") !=
                std::string::npos;
      };
  return verify_case(13, 29, 13, 29, true) &&
      verify_case(13, 29, 13, 28, false) &&
      verify_case(0, 0, -1, -1, false);
}

bool verify_host_callback_timeline_diagnostics() {
  using namespace aexcompat::worker_runtime;
  reset_host_callback_timeline();
  const char* previous =
      set_host_callback_timeline_selector("GLOBAL_SETUP");
  record_host_callback_invocation(
      "synthetic.success", 0,
      HostCallbackClassification::implemented);
  record_host_callback_invocation(
      "synthetic.success", 0,
      HostCallbackClassification::implemented);
  record_host_callback_invocation(
      "synthetic.unsupported", 4,
      HostCallbackClassification::unsupported);
  record_host_callback_invocation(
      "synthetic.failure", -7,
      HostCallbackClassification::implemented);
  record_host_callback_invocation(
      "synthetic.fallback", 0,
      HostCallbackClassification::fallback);
  auto& telemetry = host_callback_timeline_telemetry();
  const std::string report = selector_invocations_report_json();
  const bool classifications_valid =
      telemetry.records.size() == 4 &&
      telemetry.records[0].call_count == 2 &&
      telemetry.records[0].success &&
      telemetry.records[1].classification ==
          HostCallbackClassification::unsupported &&
      !telemetry.records[1].success &&
      telemetry.records[1].return_code == 4 &&
      !telemetry.records[2].success &&
      telemetry.records[2].return_code == -7 &&
      telemetry.records[3].classification ==
          HostCallbackClassification::fallback &&
      report.find("\"host_callback_timeline\":{") != std::string::npos &&
      report.find("\"maximum_records\":128") != std::string::npos &&
      report.find("\"call_count\":2") != std::string::npos &&
      report.find("\"classification\":\"unsupported\"") !=
          std::string::npos &&
      report.find("\"status\":\"failure\"") != std::string::npos;

  reset_host_callback_timeline();
  set_host_callback_timeline_selector("PARAMS_SETUP");
  for (std::size_t index = 0;
       index < kMaxHostCallbackTimelineRecords + 1; ++index) {
    record_host_callback_invocation(
        index % 2 == 0 ? "synthetic.a" : "synthetic.b",
        static_cast<int32_t>(index % 2),
        HostCallbackClassification::implemented);
  }
  const bool truncation_valid =
      telemetry.records.size() == kMaxHostCallbackTimelineRecords &&
      telemetry.truncated;
  set_host_callback_timeline_selector(previous);
  reset_host_callback_timeline();
  return classifications_valid && truncation_valid;
}

bool verify_extended_lookup_timeline_diagnostics() {
  using namespace aexcompat::worker_runtime;
  reset_extended_lookup_diagnostics();
  const char* previous =
      set_host_callback_timeline_selector("GLOBAL_SETUP");
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::null,
      ExtendedLookupStringTableState::valid,
      ExtendedLookupStringTableState::valid, 7,
      ExtendedLookupOutcome::found, 0);
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::null,
      ExtendedLookupStringTableState::valid,
      ExtendedLookupStringTableState::valid, 7,
      ExtendedLookupOutcome::found, 0);
  set_host_callback_timeline_selector("PARAMS_SETUP");
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::active_effect_module,
      ExtendedLookupStringTableState::valid,
      ExtendedLookupStringTableState::none, 8,
      ExtendedLookupOutcome::missing, 4);
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::active_resource_module,
      ExtendedLookupStringTableState::none,
      ExtendedLookupStringTableState::valid, 9,
      ExtendedLookupOutcome::found, 0);
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::other_loaded_sealed_module,
      ExtendedLookupStringTableState::none,
      ExtendedLookupStringTableState::none, -1,
      ExtendedLookupOutcome::missing, 4);
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::other_loaded_system_module,
      ExtendedLookupStringTableState::invalid,
      ExtendedLookupStringTableState::none,
      std::numeric_limits<int32_t>::max(),
      ExtendedLookupOutcome::invalid, 4);
  record_extended_lookup_diagnostic(
      ExtendedLookupOpaqueTableClassification::unrecognized,
      ExtendedLookupStringTableState::none,
      ExtendedLookupStringTableState::invalid,
      std::numeric_limits<int32_t>::min(),
      ExtendedLookupOutcome::invalid, 4);
  auto& telemetry = extended_lookup_timeline_telemetry();
  const std::string report = selector_invocations_report_json();
  const std::size_t lookup_begin =
      report.find("\"extended_lookup_timeline\":{");
  const std::size_t lookup_end =
      report.find(",\"extended_allocation_timeline\":", lookup_begin);
  const std::string lookup_report =
      lookup_begin != std::string::npos && lookup_end != std::string::npos
          ? report.substr(lookup_begin, lookup_end - lookup_begin)
          : std::string{};
  const bool states_valid =
      telemetry.records.size() == 6 &&
      telemetry.records[0].selector == "GLOBAL_SETUP" &&
      telemetry.records[0].call_count == 2 &&
      telemetry.records[0].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::null &&
      telemetry.records[0].raw_private_table_state ==
          ExtendedLookupStringTableState::valid &&
      telemetry.records[0].windows_resource_source_state ==
          ExtendedLookupStringTableState::valid &&
      telemetry.records[0].outcome == ExtendedLookupOutcome::found &&
      telemetry.records[0].return_code == 0 &&
      telemetry.records[1].selector == "PARAMS_SETUP" &&
      telemetry.records[1].lookup_id == 8 &&
      telemetry.records[1].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::active_effect_module &&
      telemetry.records[1].outcome == ExtendedLookupOutcome::missing &&
      telemetry.records[1].return_code == 4 &&
      telemetry.records[2].lookup_id == 9 &&
      telemetry.records[2].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::active_resource_module &&
      telemetry.records[2].windows_resource_source_state ==
          ExtendedLookupStringTableState::valid &&
      telemetry.records[2].outcome == ExtendedLookupOutcome::found &&
      telemetry.records[3].lookup_id == -1 &&
      telemetry.records[3].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::
              other_loaded_sealed_module &&
      telemetry.records[3].raw_private_table_state ==
          ExtendedLookupStringTableState::none &&
      telemetry.records[3].outcome == ExtendedLookupOutcome::missing &&
      telemetry.records[4].lookup_id ==
          std::numeric_limits<int32_t>::max() &&
      telemetry.records[4].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::
              other_loaded_system_module &&
      telemetry.records[4].raw_private_table_state ==
          ExtendedLookupStringTableState::invalid &&
      telemetry.records[4].outcome == ExtendedLookupOutcome::invalid &&
      telemetry.records[5].lookup_id ==
          std::numeric_limits<int32_t>::min() &&
      telemetry.records[5].opaque_table_classification ==
          ExtendedLookupOpaqueTableClassification::unrecognized &&
      telemetry.records[5].windows_resource_source_state ==
          ExtendedLookupStringTableState::invalid &&
      lookup_report.find("\"maximum_records\":128") != std::string::npos &&
      lookup_report.find("\"selector\":\"GLOBAL_SETUP\"") !=
          std::string::npos &&
      lookup_report.find("\"selector\":\"PARAMS_SETUP\"") !=
          std::string::npos &&
      lookup_report.find("\"opaque_table_classification\":\"null\"") !=
          std::string::npos &&
      lookup_report.find(
          "\"opaque_table_classification\":\"active_effect_module\"") !=
          std::string::npos &&
      lookup_report.find(
          "\"opaque_table_classification\":\"active_resource_module\"") !=
          std::string::npos &&
      lookup_report.find(
          "\"opaque_table_classification\":\"other_loaded_sealed_module\"") !=
          std::string::npos &&
      lookup_report.find(
          "\"opaque_table_classification\":\"other_loaded_system_module\"") !=
          std::string::npos &&
      lookup_report.find(
          "\"opaque_table_classification\":\"unrecognized\"") !=
          std::string::npos &&
      lookup_report.find("\"raw_private_table_state\":\"valid\"") !=
          std::string::npos &&
      lookup_report.find("\"windows_resource_source_state\":\"valid\"") !=
          std::string::npos &&
      lookup_report.find("\"lookup_id\":-1") != std::string::npos &&
      lookup_report.find("\"lookup_id\":2147483647") != std::string::npos &&
      lookup_report.find("\"outcome\":\"invalid\"") != std::string::npos &&
      lookup_report.find("\"call_count\":2") != std::string::npos &&
      lookup_report.find("\"value\"") == std::string::npos &&
      lookup_report.find("\"address\"") == std::string::npos &&
      lookup_report.find("\"path\"") == std::string::npos &&
      lookup_report.find("0x") == std::string::npos &&
      lookup_report.find(":\\") == std::string::npos;

  reset_extended_lookup_diagnostics();
  for (std::size_t index = 0;
       index < kMaxExtendedLookupTimelineRecords + 1; ++index) {
    record_extended_lookup_diagnostic(
        ExtendedLookupOpaqueTableClassification::unrecognized,
        ExtendedLookupStringTableState::valid,
        ExtendedLookupStringTableState::none,
        static_cast<int32_t>(index), ExtendedLookupOutcome::missing, 4);
  }
  const bool truncation_valid =
      telemetry.records.size() == kMaxExtendedLookupTimelineRecords &&
      telemetry.truncated;
  set_host_callback_timeline_selector(previous);
  reset_extended_lookup_diagnostics();
  return states_valid && truncation_valid;
}

bool verify_extended_lookup_table_classification() {
  using namespace aexcompat::worker_runtime;
  HMODULE executable = GetModuleHandleW(nullptr);
  HMODULE kernel32 = GetModuleHandleW(L"kernel32.dll");
  FARPROC function = kernel32
      ? GetProcAddress(kernel32, "GetCurrentProcessId") : nullptr;
  std::byte private_memory{};
  if (!executable || !kernel32 || !function) return false;
  const void* image_address = reinterpret_cast<const void*>(function);
  return classify_extended_lookup_table(
             nullptr, executable, kernel32, &classify_as_sealed) ==
             ExtendedLookupOpaqueTableClassification::null &&
      classify_extended_lookup_table(
          image_address, kernel32, nullptr, &classify_as_system) ==
          ExtendedLookupOpaqueTableClassification::active_effect_module &&
      classify_extended_lookup_table(
          image_address, executable, kernel32, &classify_as_system) ==
          ExtendedLookupOpaqueTableClassification::active_resource_module &&
      classify_extended_lookup_table(
          image_address, executable, nullptr, &classify_as_sealed) ==
          ExtendedLookupOpaqueTableClassification::
              other_loaded_sealed_module &&
      classify_extended_lookup_table(
          image_address, executable, nullptr, &classify_as_system) ==
          ExtendedLookupOpaqueTableClassification::
              other_loaded_system_module &&
      classify_extended_lookup_table(
          &private_memory, executable, kernel32, &classify_as_sealed) ==
          ExtendedLookupOpaqueTableClassification::unrecognized &&
      classify_extended_lookup_table(
          image_address, executable, nullptr, &classify_as_invalid_active) ==
          ExtendedLookupOpaqueTableClassification::unrecognized;
}

bool verify_extended_allocation_timeline_diagnostics() {
  using namespace aexcompat::worker_runtime;
  std::byte first_allocation{};
  std::byte second_allocation{};
  std::byte foreign_allocation{};
  const char* previous =
      set_host_callback_timeline_selector("GLOBAL_SETUP");

  reset_extended_allocation_diagnostics();
  record_extended_allocation_selector_entry("GLOBAL_SETUP");
  observe_extended_allocation(&first_allocation);
  record_extended_allocation_selector_exit("GLOBAL_SETUP");
  set_host_callback_timeline_selector("PARAMS_SETUP");
  record_extended_allocation_selector_entry("PARAMS_SETUP");
  const std::string live_report = selector_invocations_report_json();
  const bool live_handoff =
      live_report.find("\"extended_allocation_timeline\":{") !=
          std::string::npos &&
      live_report.find("\"global_setup_live_allocation_count\":1") !=
          std::string::npos &&
      live_report.find("\"state\":\"live\"") != std::string::npos &&
      live_report.find("\"owner_selector\":\"GLOBAL_SETUP\"") !=
          std::string::npos;

  reset_extended_allocation_diagnostics();
  set_host_callback_timeline_selector("GLOBAL_SETUP");
  record_extended_allocation_selector_entry("GLOBAL_SETUP");
  observe_extended_allocation(&second_allocation);
  observe_extended_free(&second_allocation);
  record_extended_allocation_selector_exit("GLOBAL_SETUP");
  set_host_callback_timeline_selector("PARAMS_SETUP");
  record_extended_allocation_selector_entry("PARAMS_SETUP");
  const std::string freed_report = selector_invocations_report_json();
  const bool freed_before_next_selector =
      freed_report.find("\"frees\":1") != std::string::npos &&
      freed_report.find("\"state\":\"non_live\"") != std::string::npos &&
      freed_report.find("\"global_setup_live_allocation_count\":0") !=
          std::string::npos;

  reset_extended_allocation_diagnostics();
  set_host_callback_timeline_selector("GLOBAL_SETUP");
  record_extended_allocation_selector_entry("GLOBAL_SETUP");
  observe_extended_free(&foreign_allocation);
  observe_extended_allocation(&first_allocation);
  observe_extended_free(&first_allocation);
  observe_extended_free(&first_allocation);
  record_extended_allocation_selector_exit("GLOBAL_SETUP");
  const std::string invalid_report = selector_invocations_report_json();
  const bool invalid_free =
      invalid_report.find("\"invalid_frees\":1") != std::string::npos &&
      invalid_report.find("\"double_frees\":1") != std::string::npos;

  reset_extended_allocation_diagnostics();
  set_host_callback_timeline_selector("PARAMS_SETUP");
  for (std::size_t index = 0;
       index < kMaxExtendedAllocationTimelineRecords / 2 + 1; ++index) {
    record_extended_allocation_selector_entry("PARAMS_SETUP");
    record_extended_allocation_selector_exit("PARAMS_SETUP");
  }
  const std::string truncated_report = selector_invocations_report_json();
  const bool truncation =
      truncated_report.find(
          "\"extended_allocation_timeline\":{\"maximum_records\":128") !=
          std::string::npos &&
      truncated_report.find("\"truncated\":true") != std::string::npos;

  set_host_callback_timeline_selector(previous);
  reset_extended_allocation_diagnostics();
  return live_handoff && freed_before_next_selector &&
      invalid_free && truncation;
}

}  // namespace

int main() {
  using namespace aexcompat::worker_runtime;
  configure_selector_dispatch_audit(&capture_audit, &audit_passed);
  auto& selector_telemetry = selector_dispatch_telemetry();
  const bool handoff_diagnostics_passed =
      verify_global_data_handoff_diagnostics();
  const bool effect_ref_diagnostics_passed =
      verify_effect_ref_entry_diagnostics();
  const bool application_id_diagnostics_passed =
      verify_application_id_entry_diagnostics();
  const bool spec_version_diagnostics_passed =
      verify_spec_version_entry_diagnostics();
  const bool callback_timeline_diagnostics_passed =
      verify_host_callback_timeline_diagnostics();
  const bool extended_lookup_timeline_diagnostics_passed =
      verify_extended_lookup_timeline_diagnostics();
  const bool extended_lookup_table_classification_passed =
      verify_extended_lookup_table_classification();
  const bool allocation_timeline_diagnostics_passed =
      verify_extended_allocation_timeline_diagnostics();
  selector_telemetry.invocations.clear();
  selector_telemetry.invocations_truncated = false;
  uint32_t normal_exception = 1;
  uint32_t null_exception = 0;
  uint32_t low_exception = 0;
  uint32_t module_exception = 0;
  const int32_t normal_result = invoke_entry_seh(
      &normal_return_512, 4, nullptr, nullptr, nullptr, nullptr, nullptr,
      &normal_exception);
  const int32_t null_result = invoke_entry_seh(
      &trapped_read_null, 4, nullptr, nullptr, nullptr, nullptr, nullptr,
      &null_exception);
  const int32_t low_result = invoke_entry_seh(
      &trapped_read_low, 4, nullptr, nullptr, nullptr, nullptr, nullptr,
      &low_exception);
  const int32_t module_result = invoke_entry_seh(
      &trapped_write_module, 4, nullptr, nullptr, nullptr, nullptr, nullptr,
      &module_exception);
  const std::string selector_report = selector_invocations_report_json();

  aexcompat::worker_runtime::SuiteRegistry registry;
  const void* suite{};
  bool passed = handoff_diagnostics_passed &&
      effect_ref_diagnostics_passed &&
      application_id_diagnostics_passed &&
      spec_version_diagnostics_passed &&
      callback_timeline_diagnostics_passed &&
      extended_lookup_timeline_diagnostics_passed &&
      extended_lookup_table_classification_passed &&
      allocation_timeline_diagnostics_passed &&
      normal_result == 512 && normal_exception == 0 &&
      null_result == 512 && null_exception == EXCEPTION_ACCESS_VIOLATION &&
      low_result == 512 && low_exception == EXCEPTION_ACCESS_VIOLATION &&
      module_result == 512 &&
      module_exception == EXCEPTION_ACCESS_VIOLATION &&
      selector_telemetry.invocations.size() == 4 &&
      selector_telemetry.invocations[0].selector == "PARAMS_SETUP" &&
      selector_telemetry.invocations[0].invocation_completed_normally &&
      selector_telemetry.invocations[0].has_raw_return_code &&
      selector_telemetry.invocations[0].raw_return_code == 512 &&
      selector_telemetry.invocations[0].host_result_code == 512 &&
      !selector_telemetry.invocations[0].seh_caught &&
      !selector_telemetry.invocations[0].has_plugin_rva &&
      !selector_telemetry.invocations[1].invocation_completed_normally &&
      !selector_telemetry.invocations[1].has_raw_return_code &&
      selector_telemetry.invocations[1].host_result_code == 512 &&
      selector_telemetry.invocations[1].seh_caught &&
      selector_telemetry.invocations[1].seh_code ==
          EXCEPTION_ACCESS_VIOLATION &&
      selector_telemetry.invocations[1].fault_module_class == "plugin" &&
      selector_telemetry.invocations[1].has_plugin_rva &&
      selector_telemetry.invocations[1].access_type == "read" &&
      selector_telemetry.invocations[1].has_fault_address &&
      selector_telemetry.invocations[1].fault_address.classification ==
          "null" &&
      selector_telemetry.invocations[1].has_register_snapshot &&
      selector_telemetry.invocations[1].registers.size() == 5 &&
      selector_telemetry.invocations[1].stack_values.size() == 6 &&
      selector_telemetry.invocations[2].access_type == "read" &&
      selector_telemetry.invocations[2].fault_address.classification ==
          "low" &&
      selector_telemetry.invocations[3].access_type == "write" &&
      selector_telemetry.invocations[3].fault_address.classification ==
          "module" &&
      selector_telemetry.invocations[3].fault_address.has_relative_offset &&
      !selector_telemetry.invocations[3].fault_address.module.empty() &&
      selector_report.find(
          "\"invocation_completed_normally\":true,"
          "\"raw_return_code\":512,\"host_result_code\":512,"
          "\"seh_caught\":false,\"seh_code\":null") != std::string::npos &&
      selector_report.find(
          "\"invocation_completed_normally\":false,"
          "\"raw_return_code\":null,\"host_result_code\":512,"
          "\"seh_caught\":true,\"seh_code\":3221225477") !=
          std::string::npos &&
      selector_report.find("\"fault_module_class\":\"plugin\"") !=
          std::string::npos &&
      selector_report.find("\"plugin_rva\":\"0x") != std::string::npos &&
      selector_report.find(
          "\"access_type\":\"read\",\"fault_address\":"
          "{\"classification\":\"null\"") != std::string::npos &&
      selector_report.find(
          "\"access_type\":\"read\",\"fault_address\":"
          "{\"classification\":\"low\"") != std::string::npos &&
      selector_report.find(
          "\"access_type\":\"write\",\"fault_address\":"
          "{\"classification\":\"module\"") != std::string::npos &&
      selector_report.find("\"registers\":{\"rcx\":") !=
          std::string::npos &&
      selector_report.find("\"stack_pointer_values\":[") !=
          std::string::npos &&
      registry.acquire("Known Suite", 1, &suite, &resolve_known,
                                 nullptr, nullptr) == 0 &&
      suite == reinterpret_cast<const void*>(0x1234) &&
      registry.release("Known Suite", 1, nullptr) == 0 && registry.balanced();

  std::array<char, 98> overlong{};
  overlong.fill('A');
  overlong.back() = '\0';
  passed = passed && rejected_without_resolving(registry, overlong.data());

  passed = passed && rejected_without_resolving(
      registry, reinterpret_cast<const char*>(static_cast<uintptr_t>(1)));
  {
    const int calls_before = g_resolver_calls;
    suite = reinterpret_cast<const void*>(0x5678);
    passed = passed &&
        registry.acquire("Known Suite", 65536, &suite, &resolve_known, nullptr,
                         nullptr) == 4 &&
        suite == nullptr && g_resolver_calls == calls_before;
  }

  for (int index = 0; index < 17; ++index) {
    const std::string name = "Missing Suite " + std::to_string(index);
    suite = reinterpret_cast<const void*>(0x5678);
    passed = passed &&
        registry.acquire(name.c_str(), 1, &suite, &resolve_known, nullptr,
                         nullptr) == 1 &&
        suite == nullptr;
  }
  const std::string missing_report = registry.missing_suites_report_json();
  passed = passed &&
      count_occurrences(missing_report, "\"name\":") == 16 &&
      missing_report.find("\"missing_suites_truncated\":true") !=
          std::string::npos;

  {
    SuiteRegistry failed_pair_registry;
    suite = reinterpret_cast<const void*>(0x5678);
    passed = passed &&
        failed_pair_registry.acquire("Unavailable Suite", 7, &suite,
                                     &resolve_known, nullptr, nullptr) == 1 &&
        suite == nullptr &&
        failed_pair_registry.release("Unavailable Suite", 7, nullptr) == 1 &&
        failed_pair_registry.rejected_release_count() == 0 &&
        failed_pair_registry.release("Unavailable Suite", 7, nullptr) == 1 &&
        failed_pair_registry.rejected_release_count() == 1;
  }

  {
    const auto baseline = registry.snapshot();
    suite = nullptr;
    passed = passed &&
        registry.acquire("Known Suite", 1, &suite, &resolve_known,
                         nullptr, nullptr) == 0 &&
        registry.acquire("Known Suite", 1, &suite, &resolve_known,
                         nullptr, nullptr) == 0 &&
        registry.release_since(baseline, nullptr) == 2 &&
        registry.balanced();
  }

  for (int index = 0; index < 300; ++index) {
    suite = nullptr;
    passed = passed &&
        registry.acquire("Known Suite", 1, &suite, &resolve_known, nullptr,
                         nullptr) == 0 &&
        registry.release("Known Suite", 1, nullptr) == 0;
  }
  const std::string timeline_report = registry.suite_timeline_report_json();
  passed = passed &&
      count_occurrences(timeline_report, "\"sequence\":") == 512 &&
      timeline_report.find("\"suite_timeline_truncated\":true") !=
          std::string::npos &&
      registry.balanced();

  const auto& unsupported =
      unsupported_suite_slots<UnsupportedSuiteId::aegp_comp_21, 41>();
  for (std::size_t slot = 0; slot < 33; ++slot) {
    const auto unsupported_slot =
        reinterpret_cast<int32_t(__cdecl*)()>(unsupported[slot]);
    passed = passed && unsupported_slot() == 4;
  }
  const auto repeated_unsupported_slot =
      reinterpret_cast<int32_t(__cdecl*)()>(unsupported[7]);
  passed = passed && repeated_unsupported_slot() == 4;
  const std::string unsupported_report =
      aexcompat::worker_runtime::suite_registry()
          .unsupported_suite_calls_report_json();
  passed = passed &&
      count_occurrences(unsupported_report, "\"slot\":") == 32 &&
      unsupported_report.find(
          "\"version\":21,\"slot\":7,\"call_count\":2}") !=
          std::string::npos &&
      unsupported_report.find(
          "\"unsupported_suite_calls_truncated\":true") !=
          std::string::npos;

  using namespace aexcompat::worker_runtime::suite_call_slot_probe;
  passed = passed &&
      SetEnvironmentVariableW(
          L"AEXCOMPAT_SUITE_CALL_SLOT_PROBE",
          L"PF AE Private Effect Suite@3;PF AE Private Effect Suite@5") != 0 &&
      private_effect_probe3_available(nullptr) &&
      private_effect_probe5_available(nullptr);
  const void* probe_table3 = provide_private_effect_probe3(nullptr);
  const void* probe_table5 = provide_private_effect_probe5(nullptr);
  passed = passed && probe_table3 != nullptr && probe_table5 != nullptr &&
      probe_table3 != probe_table5;
  if (probe_table3) {
    const auto* slots = static_cast<void* const*>(probe_table3);
    passed = passed && slots[7] != slots[8] &&
        invoke_probe_slot(probe_table3, 7) == kProbeExceptionBase + 7;
  }
  if (probe_table5) {
    const auto* slots = static_cast<void* const*>(probe_table5);
    passed = passed && slots[9] != slots[10] &&
        invoke_probe_slot(probe_table5, 9) ==
            kProbeExceptionBase + kProbeExceptionTargetStride + 9;
  }
  const std::string probe_report = report_json();
  const std::size_t version3 = probe_report.find("\"version\":3");
  const std::size_t version5 = probe_report.find("\"version\":5");
  passed = passed &&
      probe_report.find("\"enabled\":true") != std::string::npos &&
      probe_report.find("\"slot_count\":32") != std::string::npos &&
      probe_report.find("\"maximum_targets\":8") != std::string::npos &&
      version3 != std::string::npos && version5 > version3 &&
      probe_report.find("\"slot\":7,\"call_count\":1") !=
          std::string::npos &&
      probe_report.find("\"exception_code\":3762452487") !=
          std::string::npos &&
      probe_report.find("\"slot\":9,\"call_count\":1", version5) !=
          std::string::npos &&
      probe_report.find("\"exception_code\":3762452745", version5) !=
          std::string::npos &&
      probe_report.find("\"argument_word_count\":8") !=
          std::string::npos &&
      probe_report.find("\"nonzero_word_count\":8") !=
          std::string::npos &&
      probe_report.find("\"rcx\":\"nonzero\"") !=
          std::string::npos &&
      probe_report.find("\"rdx\":\"nonzero\"") !=
          std::string::npos &&
      probe_report.find(
          "\"stack\":[\"nonzero\",\"nonzero\",\"nonzero\",\"nonzero\"]") !=
          std::string::npos &&
      probe_report.find("\"0x0000000000000011\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000022\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000033\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000044\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000055\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000066\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000077\"") == std::string::npos &&
      probe_report.find("\"0x0000000000000088\"") == std::string::npos &&
      probe_report.find("\"caller_rva\":\"0x") != std::string::npos &&
      // One per probe target. Counted against the report's own target list so
      // that registering another target does not silently fail this check.
      count_occurrences(probe_report, "\"truncated\":false") ==
          count_occurrences(probe_report, "\"version\":") &&
      probe_report.find("\"configuration_truncated\":false") !=
          std::string::npos;
  SetEnvironmentVariableW(L"AEXCOMPAT_SUITE_CALL_SLOT_PROBE", nullptr);

  SYSTEM_INFO system_info{};
  GetSystemInfo(&system_info);
  const std::size_t page_size = system_info.dwPageSize;
  auto* pages = static_cast<unsigned char*>(VirtualAlloc(
      nullptr, page_size * 2, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE));
  if (!pages) return 2;
  DWORD old_protection{};
  if (!VirtualProtect(pages + page_size, page_size, PAGE_NOACCESS,
                      &old_protection)) {
    VirtualFree(pages, 0, MEM_RELEASE);
    return 2;
  }
  char* unterminated = reinterpret_cast<char*>(pages + page_size - 96);
  std::memset(unterminated, 'B', 96);
  passed = passed && rejected_without_resolving(registry, unterminated);
  VirtualFree(pages, 0, MEM_RELEASE);

  std::cout << "{\"suite_registry_bounds\":\""
            << (passed ? "passed" : "failed")
            << "\",\"maximum_input_name_bytes\":96,"
            << "\"maximum_telemetry_name_bytes\":64,"
            << "\"maximum_version\":65535,\"maximum_timeline_events\":512,"
            << "\"fail_closed\":true" << probe_report << selector_report
            << ",\"telemetry\":{"
            << missing_report.substr(1) << unsupported_report
            << timeline_report << "}}\n";
  return passed ? 0 : 1;
}
