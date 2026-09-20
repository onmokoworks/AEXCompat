#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <type_traits>
#include <unordered_map>
#include <utility>

namespace aexcompat::worker_runtime::module_path_cache {

// A process-lifetime loader generation. The value changes after every DLL load
// or unload notification. An empty result means the native notification hook
// could not be installed; callers must use their uncached path in that case.
std::optional<uint64_t> current_loader_generation();

// Metadata is deliberately separate from a module's final audit
// classification. The cache owner can reuse expensive, generation-stable path
// resolution while rerunning mutable classification, reparse, and hash checks
// for every audit snapshot. This container is intentionally unsynchronized;
// one audit owner must serialize access (or provide its own lock). Only the
// process loader-generation tracker is accessed from loader callback threads.
template <typename Key, typename Metadata, typename Hash = std::hash<Key>>
class GenerationMetadataCache {
 public:
  using MetadataPtr = std::shared_ptr<const Metadata>;

  explicit GenerationMetadataCache(std::size_t max_entries)
      : max_entries_(max_entries) {}

  GenerationMetadataCache(const GenerationMetadataCache&) = delete;
  GenerationMetadataCache& operator=(const GenerationMetadataCache&) = delete;

  // Resolver returns std::optional<Metadata>. A failed resolution is never
  // inserted, so a later audit in the same generation can recover.
  template <typename Resolver>
  MetadataPtr resolve(uint64_t generation, const Key& key,
                      Resolver&& resolver) {
    static_assert(std::is_same_v<std::decay_t<std::invoke_result_t<Resolver>>,
                                 std::optional<Metadata>>,
                  "metadata resolver must return std::optional<Metadata>");
    prepare_generation(generation);
    if (const auto found = entries_.find(key); found != entries_.end())
      return found->second;

    std::optional<Metadata> resolved =
        std::invoke(std::forward<Resolver>(resolver));
    if (!resolved) return {};

    auto metadata = std::make_shared<const Metadata>(std::move(*resolved));
    // The audit's enumeration bound is the natural capacity. If a caller gives
    // a smaller bound, correctness still wins: return the fresh metadata for
    // this capture without growing the cache.
    if (entries_.size() < max_entries_) entries_.emplace(key, metadata);
    return metadata;
  }

  void clear() noexcept {
    entries_.clear();
    generation_.reset();
  }

  std::size_t size() const noexcept { return entries_.size(); }
  std::optional<uint64_t> generation() const noexcept { return generation_; }

 private:
  void prepare_generation(uint64_t generation) {
    if (generation_ && *generation_ == generation) return;
    entries_.clear();
    generation_ = generation;
  }

  std::size_t max_entries_{};
  std::optional<uint64_t> generation_;
  std::unordered_map<Key, MetadataPtr, Hash> entries_;
};

enum class ConsistentCaptureStatus {
  captured,
  generation_unavailable,
  generation_changed_twice,
};

template <typename Result>
struct ConsistentCaptureResult {
  ConsistentCaptureStatus status{
      ConsistentCaptureStatus::generation_unavailable};
  std::optional<Result> value;
  uint64_t generation{};
  std::size_t attempts{};

  bool captured() const noexcept {
    return status == ConsistentCaptureStatus::captured && value.has_value();
  }
};

// Runs at most two attempts. A loader change invalidates the candidate and
// retries once; another change returns no value so the caller can emit its
// existing fail-closed snapshot instead of accepting a torn module list.
template <typename GenerationReader, typename Capture>
auto capture_consistent_generation(GenerationReader&& read_generation,
                                   Capture&& capture)
    -> ConsistentCaptureResult<
        std::decay_t<std::invoke_result_t<Capture, uint64_t>>> {
  using Result = std::decay_t<std::invoke_result_t<Capture, uint64_t>>;
  ConsistentCaptureResult<Result> result;
  for (std::size_t attempt = 1; attempt <= 2; ++attempt) {
    result.attempts = attempt;
    const std::optional<uint64_t> before = std::invoke(read_generation);
    if (!before) {
      result.status = ConsistentCaptureStatus::generation_unavailable;
      return result;
    }
    Result candidate = std::invoke(capture, *before);
    const std::optional<uint64_t> after = std::invoke(read_generation);
    if (!after) {
      result.status = ConsistentCaptureStatus::generation_unavailable;
      return result;
    }
    if (*before == *after) {
      result.status = ConsistentCaptureStatus::captured;
      result.value.emplace(std::move(candidate));
      result.generation = *after;
      return result;
    }
  }
  result.status = ConsistentCaptureStatus::generation_changed_twice;
  return result;
}

}  // namespace aexcompat::worker_runtime::module_path_cache
