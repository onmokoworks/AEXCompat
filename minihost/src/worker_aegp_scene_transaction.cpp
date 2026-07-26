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

}  // namespace

std::mutex& mutation_mutex() noexcept { return g_mutation_mutex; }
void record_begin() noexcept { ++g_begun; }
void record_stage() noexcept { ++g_staged; }
void record_validate() noexcept { ++g_validated; }
void record_commit() noexcept { ++g_committed; }
void record_cancel() noexcept { ++g_cancelled; }

Diagnostics diagnostics() noexcept {
  return {g_begun.load(), g_staged.load(), g_validated.load(),
          g_committed.load(), g_cancelled.load()};
}

}  // namespace aexcompat::scene_transaction
