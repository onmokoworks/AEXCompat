#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_render_receipts.hpp"

#include <windows.h>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <new>
#include <system_error>
#include <thread>
#include <unordered_map>

namespace aexcompat::aegp_async_layer {
namespace {
struct Request {
  uint64_t id{};
  Callback callback{};
  void* refcon{};
  render_options::LayerValue options{};
  SourceSnapshot source;
  uint64_t reserved_bytes{};
  std::atomic<int32_t> state{0};
  std::mutex gate_mutex;
  std::condition_variable gate_changed;
};
Hooks g_hooks{};
std::mutex g_mutex;
std::unordered_map<uint64_t, std::shared_ptr<Request>> g_requests;
std::vector<std::thread> g_threads;
uint64_t g_next_id{1}, g_reserved_bytes{};
bool g_accepting{true}, g_cancel_test_gate{};
uint32_t g_created{}, g_completed{}, g_canceled{}, g_callback_failures{}, g_callback_exceptions{};

}

void configure(const Hooks& hooks) noexcept { g_hooks = hooks; }
void set_cancel_test_gate(bool enabled) noexcept { g_cancel_test_gate = enabled; }

int32_t checkout(void* options, Callback callback, void* refcon, uint64_t* id) {
  if (id) *id = 0;
  if (!id || !callback || !g_hooks.render_worker || !g_hooks.render_worker() ||
      !g_hooks.snapshot_options || !g_hooks.capture_source || !g_hooks.publish) return 4;
  render_options::LayerValue snapshot{};
  SourceSnapshot source{};
  if (!g_hooks.snapshot_options(options, snapshot) || !g_hooks.capture_source(snapshot, source))
    return 4;
  const int32_t output_bpp = snapshot.world_type == 1 ? 4 :
      (snapshot.world_type == 2 ? 8 : (snapshot.world_type == 3 ? 16 : 0));
  if (!output_bpp || source.width <= 0 || source.height <= 0 || source.pixel_bytes <= 0)
    return 4;
  const uint64_t input_bytes = static_cast<uint64_t>(source.width) * source.height * source.pixel_bytes;
  const uint64_t output_bytes = static_cast<uint64_t>((source.width + snapshot.downsample_x - 1) / snapshot.downsample_x) *
      ((source.height + snapshot.downsample_y - 1) / snapshot.downsample_y) * output_bpp;
  const uint64_t bytes = (std::max)(input_bytes, output_bytes);
  if (!bytes || bytes > render_receipts::kMaxReceiptBytes || source.pixels.size() != input_bytes)
    return 4;
  std::shared_ptr<Request> request;
  try { request = std::make_shared<Request>(); request->callback = callback;
    request->refcon = refcon; request->options = snapshot; request->source = std::move(source);
    request->reserved_bytes = bytes;
  } catch (const std::bad_alloc&) { return 4; }
  std::lock_guard<std::mutex> lock(g_mutex);
  if (!g_accepting || g_requests.size() >= 32 ||
      g_reserved_bytes > render_receipts::kMaxReceiptBytes - bytes) return 4;
  request->id = g_next_id++;
  if (!request->id) return 4;
  g_reserved_bytes += bytes;
  try {
    g_requests.emplace(request->id, request); *id = request->id;
    g_threads.emplace_back([request] {
      if (g_cancel_test_gate) { std::unique_lock<std::mutex> lock(request->gate_mutex);
        request->gate_changed.wait_for(lock, std::chrono::seconds(5), [&] { return request->state.load() != 0; }); }
      int32_t expected = 0; const bool won = request->state.compare_exchange_strong(expected, 1);
      void* receipt = nullptr; int32_t error = 0; uint8_t canceled = 0;
      if (won) error = g_hooks.publish(request->source, request->options, &receipt); else canceled = 1;
      int32_t callback_error{}; uint32_t exception{};
      const int32_t invoke_error = g_hooks.invoke_callback ? g_hooks.invoke_callback(
          request->callback, request->id, canceled, error, receipt, request->refcon,
          &callback_error, &exception) : 4;
      if ((invoke_error || callback_error) && g_hooks.checkin_if_live) g_hooks.checkin_if_live(receipt);
      request->state.store(3);
      std::lock_guard<std::mutex> lock(g_mutex);
      g_reserved_bytes -= request->reserved_bytes;
      if (canceled) ++g_canceled; else ++g_completed;
      if (invoke_error || callback_error) ++g_callback_failures;
      if (exception) ++g_callback_exceptions;
      g_requests.erase(request->id);
    });
  } catch (...) { g_requests.erase(request->id); g_reserved_bytes -= bytes; *id = 0; return 4; }
  ++g_created; return 0;
}

int32_t cancel(uint64_t id) { std::lock_guard<std::mutex> lock(g_mutex);
  const auto found = g_requests.find(id); if (found == g_requests.end()) return 4;
  int32_t expected = 0; if (!found->second->state.compare_exchange_strong(expected, 2)) return 4;
  found->second->gate_changed.notify_one(); return 0; }
void drain() { std::vector<std::thread> threads; { std::lock_guard<std::mutex> lock(g_mutex);
  g_accepting = false; threads.swap(g_threads); } for (auto& t : threads) if (t.joinable()) t.join(); }
Diagnostics diagnostics() { std::lock_guard<std::mutex> lock(g_mutex); return {
  g_created, g_completed, g_canceled, g_callback_failures, g_callback_exceptions,
  g_requests.size(), g_reserved_bytes}; }
bool balanced() { const auto d = diagnostics(); return d.live == 0 && d.reserved_bytes == 0 &&
  d.created == d.completed + d.canceled; }
}  // namespace aexcompat::aegp_async_layer
