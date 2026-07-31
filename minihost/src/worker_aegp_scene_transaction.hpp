#pragma once

#include "worker_aegp_scene_model.hpp"

#include <cstdint>
#include <mutex>
#include <utility>

namespace aexcompat::scene_transaction {

struct Diagnostics {
  uint64_t begun{};
  uint64_t staged{};
  uint64_t validated{};
  uint64_t committed{};
  uint64_t cancelled{};
  uint64_t rolled_back{};
  uint64_t rollback_failures{};
};

using GenerationInvalidator = void(*)(uint64_t project_id,
                                      uint32_t valid_generation) noexcept;
using GenerationReader = uint32_t(*)() noexcept;

std::mutex& mutation_mutex() noexcept;
void configure_generation_invalidator(GenerationInvalidator callback) noexcept;
void notify_generation_invalidated(uint64_t project_id,
                                   uint32_t valid_generation) noexcept;
void record_begin() noexcept;
void record_stage() noexcept;
void record_validate() noexcept;
void record_commit() noexcept;
void record_cancel() noexcept;
void record_rollback(bool restored) noexcept;
Diagnostics diagnostics() noexcept;

class MutationLock {
 public:
  MutationLock() noexcept;
  MutationLock(const MutationLock&) = delete;
  MutationLock& operator=(const MutationLock&) = delete;
  MutationLock(MutationLock&&) noexcept = default;
  MutationLock& operator=(MutationLock&&) noexcept = default;

 private:
  friend class AtomicSceneTransaction;
  std::unique_lock<std::mutex> lock_;
};

uint64_t waiting_mutations() noexcept;

class AtomicSceneTransaction {
 public:
  AtomicSceneTransaction(scene_model::Registry& registry,
                         uint64_t project_id,
                         GenerationReader generation_reader) noexcept
      : AtomicSceneTransaction(
            registry, project_id, generation_reader, MutationLock{}) {}

  AtomicSceneTransaction(scene_model::Registry& registry,
                         uint64_t project_id,
                         GenerationReader generation_reader,
                         MutationLock&& mutation_lock) noexcept
      : registry_(registry),
        lock_(std::move(mutation_lock.lock_)),
        project_id_(project_id),
        generation_reader_(generation_reader) {
    if (!lock_.owns_lock()) {
      record_begin();
      return;
    }
    baseline_project_generation_ =
        generation_reader_ ? generation_reader_() : 0;
    baseline_fingerprint_ = registry_.fingerprint();
    active_ = project_id_ != 0 && baseline_project_generation_ != 0 &&
        baseline_project_generation_ != UINT32_MAX;
    record_begin();
  }

  AtomicSceneTransaction(const AtomicSceneTransaction&) = delete;
  AtomicSceneTransaction& operator=(const AtomicSceneTransaction&) = delete;

  ~AtomicSceneTransaction() {
    if (active_) cancel();
  }

  bool stage() noexcept {
    if (!active_ || staged_) return false;
    staged_ = true;
    record_stage();
    return true;
  }

  bool validate(bool condition) noexcept {
    if (!active_ || !staged_ || validated_) return false;
    if (!condition) {
      cancel();
      return false;
    }
    validated_ = true;
    record_validate();
    return true;
  }

  template <typename Apply, typename Bump>
  bool commit(Apply&& apply, Bump&& bump) noexcept {
    return commit(
        std::forward<Apply>(apply),
        []() noexcept { return true; }, std::forward<Bump>(bump));
  }

  template <typename Apply, typename Rollback, typename Bump>
  bool commit(Apply&& apply, Rollback&& rollback, Bump&& bump) noexcept {
    if (!active_ || !staged_ || !validated_ ||
        !generation_reader_ ||
        generation_reader_() != baseline_project_generation_ ||
        registry_.fingerprint() != baseline_fingerprint_) {
      cancel();
      return false;
    }
    if (!std::forward<Apply>(apply)()) {
      const bool restored = std::forward<Rollback>(rollback)() &&
          registry_.fingerprint() == baseline_fingerprint_;
      record_rollback(restored);
      cancel();
      return false;
    }
    notify_generation_invalidated(project_id_,
                                  baseline_project_generation_ + 1);
    std::forward<Bump>(bump)();
    active_ = false;
    committed_ = true;
    record_commit();
    lock_.unlock();
    return true;
  }

  void cancel() noexcept {
    if (!active_) return;
    active_ = false;
    record_cancel();
    lock_.unlock();
  }

  uint64_t project_id() const noexcept { return project_id_; }
  uint32_t baseline_project_generation() const noexcept {
    return baseline_project_generation_;
  }
  bool committed() const noexcept { return committed_; }

 private:
  scene_model::Registry& registry_;
  std::unique_lock<std::mutex> lock_;
  uint64_t project_id_{};
  GenerationReader generation_reader_{};
  uint32_t baseline_project_generation_{};
  uint64_t baseline_fingerprint_{};
  bool active_{};
  bool staged_{};
  bool validated_{};
  bool committed_{};
};

}  // namespace aexcompat::scene_transaction
