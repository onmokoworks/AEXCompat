#include "worker_aegp_scene_transaction.hpp"

#include <atomic>

namespace aexcompat::scene_transaction {
namespace {

std::mutex g_mutation_mutex;
std::atomic<uint64_t> g_begun{};
std::atomic<uint64_t> g_staged{};
std::atomic<uint64_t> g_validated{};
std::atomic<uint64_t> g_committed{};
std::atomic<uint64_t> g_cancelled{};
std::atomic<uint64_t> g_rolled_back{};
std::atomic<uint64_t> g_rollback_failures{};
std::atomic<GenerationInvalidator> g_generation_invalidator{};

}  // namespace

std::mutex& mutation_mutex() noexcept { return g_mutation_mutex; }
void configure_generation_invalidator(GenerationInvalidator callback) noexcept {
  g_generation_invalidator.store(callback);
}
void notify_generation_invalidated(uint64_t project_id,
                                   uint32_t valid_generation) noexcept {
  if (const auto callback = g_generation_invalidator.load())
    callback(project_id, valid_generation);
}
void record_begin() noexcept { ++g_begun; }
void record_stage() noexcept { ++g_staged; }
void record_validate() noexcept { ++g_validated; }
void record_commit() noexcept { ++g_committed; }
void record_cancel() noexcept { ++g_cancelled; }
void record_rollback(bool restored) noexcept {
  ++g_rolled_back;
  if (!restored) ++g_rollback_failures;
}

Diagnostics diagnostics() noexcept {
  return {g_begun.load(), g_staged.load(), g_validated.load(),
          g_committed.load(), g_cancelled.load(), g_rolled_back.load(),
          g_rollback_failures.load()};
}

}  // namespace aexcompat::scene_transaction
