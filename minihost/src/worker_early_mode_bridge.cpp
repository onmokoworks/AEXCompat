#include "worker_early_mode_bridge.hpp"

#include "worker_handle_runtime.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_session.hpp"

#include <cstdint>
#include <cstring>

// Early-mode hook adapters moved from worker_main (issue #165): every
// adapter reads the bridge and forwards into the same worker-entry owned
// helpers as before.
namespace aexcompat::l2_detail {

using namespace aexcompat::worker_runtime::parameter_execution;
using namespace aexcompat::worker_runtime::handles;

// Worker-entry owned selector invokers and the L2 report emitter stay with
// their owners; the adapters read them cross-TU.
int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr);
int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output);
void report(const char* status, int32_t global_error, int32_t params_error,
            int32_t setdown_error, const std::array<std::byte, 408>& output,
            const std::string& about_message, const std::array<int32_t, 5>& lifecycle_errors,
            bool lifecycle_data_null);

namespace {
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kOutSequenceData = 56;
constexpr std::size_t kOutFlags = 96;
constexpr std::size_t kOutMessage = 100;
constexpr std::size_t kOutSize = 408;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kDoDialog = 9;
constexpr int32_t kGetExternalDependencies = 16;
using aexcompat::worker_runtime::invoke_entry_seh;

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}
}  // namespace

uint32_t early_mode_out_flags(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return read<uint32_t>(*b.output, kOutFlags);
}
void early_mode_copy_sequence_data_to_input(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  write<void**>(*b.input, kInSequenceData, read<void**>(*b.output, kOutSequenceData));
}
int32_t early_mode_sequence_setup(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_sequence_selector(b.entry, kSequenceSetup, b.input->data(), b.output->data(), exception);
}
int32_t early_mode_sequence_setdown(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_sequence_selector(b.entry, kSequenceSetdown, b.input->data(), b.output->data(), exception);
}
int32_t early_mode_do_dialog(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_entry_seh(b.entry, kDoDialog, b.input->data(), b.output->data(),
                          nullptr, nullptr, nullptr, exception);
}
int32_t early_mode_global_setdown(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_global_setdown(b.entry, b.input->data(), b.output->data());
}
std::string early_mode_return_message(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  const char* message = reinterpret_cast<const char*>(b.output->data() + kOutMessage);
  return {message, strnlen_s(message, kOutSize - kOutMessage)};
}
bool early_mode_handle_lifetimes_balanced(void*) { return handle_lifetimes_balanced(); }
bool early_mode_prepare_protocol_report(void* opaque) {
  return static_cast<EarlyModeBridge*>(opaque)->session->prepare_protocol_report();
}
void* early_mode_external_dependencies(void* opaque, int32_t check_type,
                                       int32_t* error, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  std::array<std::byte, 16> extra{};
  write<int32_t>(extra, 0, check_type);
  *error = invoke_entry_seh(b.entry, kGetExternalDependencies, b.input->data(), b.output->data(),
                            nullptr, nullptr, extra.data(), exception);
  return read<void**>(extra, 8);
}
bool early_mode_handle_is_live(void*, void* handle) { return host_handle_is_live(static_cast<void**>(handle)); }
uint64_t early_mode_handle_size(void*, void* handle) { return handle_size(static_cast<void**>(handle)); }
void* early_mode_lock_handle(void*, void* handle) { return lock_handle(static_cast<void**>(handle)); }
void early_mode_unlock_handle(void*, void* handle) { unlock_handle(static_cast<void**>(handle)); }
void early_mode_dispose_handle(void*, void* handle) { dispose_handle(static_cast<void**>(handle)); }
aexcompat::l2mode::HandleStatistics early_mode_handle_statistics(void*) {
  const auto s = statistics(); return {s.created, s.disposed};
}
bool early_mode_dispose_arbitrary_defaults(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return dispose_arbitrary_defaults(b.entry, *b.input, *b.output);
}
void early_mode_report_parameters(void* opaque, const char* status, int32_t global_error,
                                  int32_t params_error, int32_t setdown_error) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  report(status, global_error, params_error, setdown_error, *b.output, *b.about_message,
         {-1, -1, -1, -1, -1}, true);
}

const aexcompat::l2mode::Hooks& early_mode_hooks() {
  static const aexcompat::l2mode::Hooks hooks{
      early_mode_out_flags, early_mode_copy_sequence_data_to_input,
      early_mode_sequence_setup, early_mode_sequence_setdown, early_mode_do_dialog,
      early_mode_global_setdown, early_mode_return_message,
      early_mode_handle_lifetimes_balanced, early_mode_prepare_protocol_report,
      early_mode_external_dependencies, early_mode_handle_is_live,
      early_mode_handle_size, early_mode_lock_handle, early_mode_unlock_handle,
      early_mode_dispose_handle, early_mode_handle_statistics,
      early_mode_dispose_arbitrary_defaults, early_mode_report_parameters};
  return hooks;
}

}  // namespace aexcompat::l2_detail
