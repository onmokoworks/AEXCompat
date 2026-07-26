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
};

using GenerationInvalidator = void(*)(uint64_t project_id,
                                      uint32_t valid_generation) noexcept;

std::mutex& mutation_mutex() noexcept;
void configure_generation_invalidator(GenerationInvalidator callback) noexcept;
void notify_generation_invalidated(uint64_t project_id,
                                   uint32_t valid_generation) noexcept;
void record_begin() noexcept;
void record_stage() noexcept;
void record_validate() noexcept;
void record_commit() noexcept;
void record_cancel() noexcept;
Diagnostics diagnostics() noexcept;

class AtomicSceneTransaction {
 public:
  AtomicSceneTransaction(scene_model::Registry& registry,
                         uint64_t project_id,
                         uint32_t project_generation) noexcept
      : registry_(registry),
        lock_(mutation_mutex()),
        project_id_(project_id),
        baseline_project_generation_(project_generation),
        baseline_fingerprint_(registry.fingerprint()),
        active_(project_id != 0 && project_generation != 0 &&
                project_generation != UINT32_MAX) {
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
  bool commit(uint32_t current_project_generation,
              Apply&& apply, Bump&& bump) noexcept {
    if (!active_ || !staged_ || !validated_ ||
        current_project_generation != baseline_project_generation_ ||
        registry_.fingerprint() != baseline_fingerprint_) {
      cancel();
      return false;
    }
    if (!std::forward<Apply>(apply)()) {
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
  uint32_t baseline_project_generation_{};
  uint64_t baseline_fingerprint_{};
  bool active_{};
  bool staged_{};
  bool validated_{};
  bool committed_{};
};

}  // namespace aexcompat::scene_transaction
