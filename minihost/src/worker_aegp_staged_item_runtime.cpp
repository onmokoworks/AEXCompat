#include "worker_aegp_staged_item_runtime.hpp"

#include "worker_render_receipts.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include <mutex>
#include <numeric>
#include <vector>

namespace aexcompat::aegp_staged_item_runtime {
namespace {

using render_options::ItemValue;
using render_receipts::ReceiptDraft;
using suite_abi::AegpRect;
using suite_abi::AegpTime;

struct NormalizedRational {
  int64_t numerator{};
  uint64_t denominator{};
  bool valid{};

  friend bool operator==(const NormalizedRational& left,
                         const NormalizedRational& right) {
    return left.valid == right.valid && (!left.valid ||
        (left.numerator == right.numerator && left.denominator == right.denominator));
  }
};

NormalizedRational normalize_rational(AegpTime time) noexcept {
  if (time.scale == 0) return {};
  const int64_t numerator = time.value;
  const uint64_t denominator = time.scale;
  const uint64_t magnitude = numerator < 0
      ? static_cast<uint64_t>(-numerator) : static_cast<uint64_t>(numerator);
  const uint64_t divisor = std::gcd(magnitude, denominator);
  return {numerator / static_cast<int64_t>(divisor), denominator / divisor, true};
}

struct StagedItemWorld {
  void* item{};
  uint64_t item_identity{};
  StageKind stage_kind{StageKind::final_item};
  uint64_t effect_instance{};
  AegpTime time{};
  AegpTime time_step{};
  int8_t quality{1};
  uint8_t guide_layers{};
  int32_t pixel_format{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  uint32_t project_generation{};
  bool scene_bound{};
  scene_model::Identity scene_item{};
  scene_model::Identity scene_effect{};
  scene_model::Identity scene_project{};
  uint32_t effect_order{};
  struct StageIdentity {
    void* item{};
    uint64_t item_identity{};
    StageKind stage_kind{StageKind::final_item};
    uint64_t effect_instance{};
    NormalizedRational time{};
    NormalizedRational time_step{};
    int8_t quality{};
    uint8_t guide_layers{};
    int32_t pixel_format{};
    int32_t width{};
    int32_t height{};
    int32_t rowbytes{};
    uint32_t project_generation{};
    bool scene_bound{};
    scene_model::Identity scene_item{};
    scene_model::Identity scene_effect{};
    scene_model::Identity scene_project{};
    uint32_t effect_order{};

    friend bool operator==(const StageIdentity& left, const StageIdentity& right) {
      return left.item == right.item && left.item_identity == right.item_identity &&
          left.stage_kind == right.stage_kind &&
          left.effect_instance == right.effect_instance && left.time == right.time &&
          left.time_step == right.time_step && left.quality == right.quality &&
          left.guide_layers == right.guide_layers &&
          left.pixel_format == right.pixel_format && left.width == right.width &&
          left.height == right.height && left.rowbytes == right.rowbytes &&
          left.project_generation == right.project_generation &&
          left.scene_bound == right.scene_bound &&
          left.scene_item == right.scene_item &&
          left.scene_effect == right.scene_effect &&
          left.scene_project == right.scene_project &&
          left.effect_order == right.effect_order;
    }
  } identity{};
  uint64_t stage_generation{};
  std::shared_ptr<const std::vector<std::byte>> backing;
};

struct ItemRegistration {
  void* item{};
  uint64_t stable_identity{};
  SamplingPolicy policy{SamplingPolicy::exact};
  std::vector<void*> dependencies;
  std::vector<uint64_t> effect_instances;
  bool scene_bound{};
  scene_model::Identity scene_item{};
  scene_model::Identity scene_project{};
  std::vector<scene_model::Identity> scene_dependencies;
  std::vector<OrderedSceneEffect> scene_effects;
};

struct ItemRenderStackKey {
  void* item{};
  AegpTime time{};
  uint32_t project_generation{};
};

constexpr std::size_t kMaxStagedItemWorlds = 32;
constexpr std::size_t kMaxRegisteredItems = 16;
constexpr std::size_t kMaxDependenciesPerItem = 8;
constexpr std::size_t kMaxEffectsPerItem = 8;
constexpr std::size_t kMaxResolveDepth = 8;
constexpr std::size_t kMaxResolvedStages = 24;
constexpr uint64_t kMaxSchedulerBytes = render_receipts::kMaxReceiptBytes;
constexpr auto kMaxResolveTime = std::chrono::milliseconds(250);
Hooks g_hooks{};
std::mutex g_mutex;
std::vector<StagedItemWorld> g_worlds;
std::vector<ItemRegistration> g_items;
uint64_t g_world_bytes{};
std::atomic<uint64_t> g_stage_generation{1};
thread_local std::vector<ItemRenderStackKey> g_render_stack;
std::atomic<uint32_t> g_published{};
std::atomic<uint32_t> g_cache_hits{};
std::atomic<uint32_t> g_cache_misses{};
std::atomic<uint32_t> g_cycles_rejected{};
std::atomic<uint32_t> g_generation_invalidations{};
std::atomic<uint32_t> g_evictions{};
std::atomic<uint32_t> g_exact_hits{};
std::atomic<uint32_t> g_hold_hits{};
std::atomic<uint32_t> g_nearest_hits{};
std::atomic<uint32_t> g_unavailable_frames{};
std::atomic<uint32_t> g_direct_cycles_rejected{};
std::atomic<uint32_t> g_indirect_cycles_rejected{};
std::atomic<uint32_t> g_depth_limit_rejections{};
std::atomic<uint32_t> g_stage_limit_rejections{};
std::atomic<uint32_t> g_time_limit_rejections{};
std::atomic<uint32_t> g_effect_boundary_rejections{};
std::atomic<uint32_t> g_partial_failures{};
std::atomic<uint32_t> g_cleanup_count{};
std::atomic<uint32_t> g_in_flight{};
std::atomic<uint32_t> g_max_in_flight{};
std::atomic<uint64_t> g_last_trace_hash{};
std::atomic<uint64_t> g_last_stage_identity_hash{};
std::atomic<uint32_t> g_last_resolved_stages{};
std::atomic<uint32_t> g_max_resolved_depth{};
std::atomic<uint32_t> g_typed_registrations{};
std::atomic<uint32_t> g_identity_mismatch_rejections{};
std::atomic<uint32_t> g_duplicate_identity_rejections{};
std::atomic<uint32_t> g_cross_project_rejections{};
std::atomic<uint32_t> g_registration_cycle_rejections{};
std::atomic<uint32_t> g_stale_stage_invalidations{};
std::atomic<uint32_t> g_stale_receipt_invalidations{};
std::atomic<uint32_t> g_invalid_handle_rejections{};
uint32_t g_cache_generation{};

int32_t pixel_bytes_for(int32_t pixel_format) {
  if (pixel_format == world_registry::kPixelFormatArgb32) return 4;
  if (pixel_format == world_registry::kPixelFormatArgb64) return 8;
  if (pixel_format == world_registry::kPixelFormatArgb128) return 16;
  return 0;
}

bool same_rational(const AegpTime& left, const AegpTime& right) {
  return left.scale != 0 && right.scale != 0 &&
      normalize_rational(left) == normalize_rational(right);
}

int compare_rational(AegpTime left, AegpTime right) {
  const int64_t left_scaled =
      static_cast<int64_t>(left.value) * right.scale;
  const int64_t right_scaled =
      static_cast<int64_t>(right.value) * left.scale;
  return left_scaled < right_scaled ? -1 : (left_scaled > right_scaled ? 1 : 0);
}

uint64_t signed_magnitude(int64_t value) {
  return value < 0 ? static_cast<uint64_t>(-(value + 1)) + 1
                   : static_cast<uint64_t>(value);
}

struct RationalDistance {
  uint64_t numerator{};
  uint64_t denominator{1};
};

RationalDistance rational_distance(AegpTime left, AegpTime right) {
  const int64_t left_scaled =
      static_cast<int64_t>(left.value) * right.scale;
  const int64_t right_scaled =
      static_cast<int64_t>(right.value) * left.scale;
  uint64_t difference = 0;
  if ((left_scaled < 0) == (right_scaled < 0))
    difference = signed_magnitude(left_scaled - right_scaled);
  else
    difference =
        signed_magnitude(left_scaled) + signed_magnitude(right_scaled);
  return {difference,
          static_cast<uint64_t>(left.scale) * right.scale};
}

int compare_positive_fractions(uint64_t left_numerator,
                               uint64_t left_denominator,
                               uint64_t right_numerator,
                               uint64_t right_denominator) {
  bool reversed = false;
  for (;;) {
    const uint64_t left_quotient = left_numerator / left_denominator;
    const uint64_t right_quotient = right_numerator / right_denominator;
    if (left_quotient != right_quotient) {
      const int result = left_quotient < right_quotient ? -1 : 1;
      return reversed ? -result : result;
    }
    const uint64_t left_remainder = left_numerator % left_denominator;
    const uint64_t right_remainder = right_numerator % right_denominator;
    if (left_remainder == 0 || right_remainder == 0) {
      const int result = left_remainder == right_remainder
          ? 0
          : (left_remainder == 0 ? -1 : 1);
      return reversed ? -result : result;
    }
    left_numerator = left_denominator;
    left_denominator = left_remainder;
    right_numerator = right_denominator;
    right_denominator = right_remainder;
    reversed = !reversed;
  }
}

uint64_t hash_mix(uint64_t hash, uint64_t value) noexcept {
  constexpr uint64_t kPrime = 1099511628211ULL;
  for (int index = 0; index < 8; ++index) {
    hash ^= static_cast<uint8_t>(value >> (index * 8));
    hash *= kPrime;
  }
  return hash;
}

uint64_t hash_identity(uint64_t hash,
                       scene_model::Identity identity) noexcept {
  hash = hash_mix(hash, identity.project_id);
  hash = hash_mix(hash, identity.object_id);
  hash = hash_mix(hash, identity.generation);
  return hash_mix(hash, static_cast<uint8_t>(identity.kind));
}

uint64_t stable_scene_identity(scene_model::Identity identity) noexcept {
  return hash_identity(1469598103934665603ULL, identity);
}

uint64_t stable_item_identity_locked(void* item) {
  const auto found = std::find_if(g_items.begin(), g_items.end(),
      [item](const auto& value) { return value.item == item; });
  return found == g_items.end() ? 0 : found->stable_identity;
}

StagedItemWorld::StageIdentity make_stage_identity(
    void* item, uint64_t item_identity, StageKind stage_kind, uint64_t effect_instance,
    AegpTime time, AegpTime time_step, int8_t quality, uint8_t guide_layers,
    int32_t pixel_format, int32_t width, int32_t height, int32_t rowbytes,
    uint32_t project_generation, bool scene_bound = false,
    scene_model::Identity scene_item = {},
    scene_model::Identity scene_effect = {},
    scene_model::Identity scene_project = {},
    uint32_t effect_order = 0) noexcept {
  return {item, item_identity, stage_kind, effect_instance, normalize_rational(time),
          normalize_rational(time_step), quality, guide_layers, pixel_format,
          width, height, rowbytes, project_generation, scene_bound, scene_item,
          scene_effect, scene_project, effect_order};
}

uint64_t stage_identity_hash(
    const StagedItemWorld::StageIdentity& identity) noexcept {
  uint64_t hash = 1469598103934665603ULL;
  hash = hash_mix(hash, identity.item_identity);
  hash = hash_mix(hash, static_cast<uint8_t>(identity.stage_kind));
  hash = hash_mix(hash, identity.effect_instance);
  hash = hash_mix(hash, static_cast<uint64_t>(identity.time.numerator));
  hash = hash_mix(hash, identity.time.denominator);
  hash = hash_mix(hash, static_cast<uint64_t>(identity.time_step.numerator));
  hash = hash_mix(hash, identity.time_step.denominator);
  hash = hash_mix(hash, static_cast<uint8_t>(identity.quality));
  hash = hash_mix(hash, identity.guide_layers);
  hash = hash_mix(hash, static_cast<uint32_t>(identity.pixel_format));
  hash = hash_mix(hash, static_cast<uint32_t>(identity.width));
  hash = hash_mix(hash, static_cast<uint32_t>(identity.height));
  hash = hash_mix(hash, static_cast<uint32_t>(identity.rowbytes));
  hash = hash_mix(hash, identity.project_generation);
  hash = hash_mix(hash, identity.scene_bound);
  hash = hash_identity(hash, identity.scene_item);
  hash = hash_identity(hash, identity.scene_effect);
  hash = hash_identity(hash, identity.scene_project);
  return hash_mix(hash, identity.effect_order);
}

void invalidate_generation_locked() {
  if (!g_worlds.empty() || !g_items.empty() || g_cache_generation != 0)
    ++g_generation_invalidations;
  g_worlds.clear();
  g_items.clear();
  g_world_bytes = 0;
  g_cache_generation = 0;
}

void ensure_generation(uint32_t generation) {
  std::lock_guard<std::mutex> lock(g_mutex);
  if (g_cache_generation != 0 && g_cache_generation != generation)
    invalidate_generation_locked();
  g_cache_generation = generation;
}

bool stack_contains(const ItemRenderStackKey& key) {
  return std::any_of(g_render_stack.begin(), g_render_stack.end(), [&](const auto& active) {
    return active.item == key.item && active.project_generation == key.project_generation &&
        same_rational(active.time, key.time);
  });
}

struct StackScope {
  bool entered{};
  explicit StackScope(ItemRenderStackKey key) {
    if (stack_contains(key)) return;
    g_render_stack.push_back(key);
    entered = true;
  }
  ~StackScope() { if (entered) g_render_stack.pop_back(); }
};

struct ResolveSnapshot {
  std::vector<StagedItemWorld> worlds;
  std::vector<ItemRegistration> items;
};

struct ResolveContext {
  const ResolveSnapshot& snapshot;
  const ItemValue& options;
  int32_t pixel_format{};
  uint32_t project_generation{};
  std::chrono::steady_clock::time_point deadline;
  std::vector<void*> chain;
  uint64_t trace_hash{1469598103934665603ULL};
  uint32_t resolved_stages{};
  uint32_t max_depth{};
  bool partial_recorded{};
};

struct ResolvedPlan {
  StagedItemWorld final_stage;
  SamplingPolicy policy{SamplingPolicy::exact};
  uint64_t item_identity{};
  uint64_t trace_hash{};
  uint32_t resolved_stages{};
  uint32_t resolved_depth{};
  bool registered_item{};
  bool scene_bound{};
  scene_model::Identity scene_item{};
  scene_model::Identity scene_project{};
  scene_model::Identity last_scene_effect{};
  uint32_t last_effect_order{};
  uint64_t dependency_identity_hash{};
  uint64_t effect_order_hash{};
};

template <typename Counter>
bool resolve_failure(ResolveContext& context, Counter& counter) {
  ++counter;
  if (context.resolved_stages != 0 && !context.partial_recorded) {
    ++g_partial_failures;
    context.partial_recorded = true;
  }
  return false;
}

void update_atomic_max(std::atomic<uint32_t>& target, uint32_t value) {
  uint32_t observed = target.load();
  while (observed < value && !target.compare_exchange_weak(observed, value)) {}
}

const ItemRegistration* find_registration(
    const ResolveSnapshot& snapshot, void* item) {
  const auto found = std::find_if(snapshot.items.begin(), snapshot.items.end(),
      [item](const auto& value) { return value.item == item; });
  return found == snapshot.items.end() ? nullptr : &*found;
}

bool candidate_matches(const StagedItemWorld& value, void* item, StageKind kind,
                       uint64_t effect_instance, const ItemValue& options,
                       int32_t pixel_format, uint32_t project_generation) {
  return value.item == item && value.stage_kind == kind &&
      value.effect_instance == effect_instance &&
      same_rational(value.time_step, options.time_step) &&
      value.quality == options.render_quality &&
      value.guide_layers == options.render_guide_layers &&
      value.pixel_format == pixel_format &&
      value.project_generation == project_generation && value.backing;
}

bool candidate_matches(const StagedItemWorld& value, void* item, StageKind kind,
                       uint64_t effect_instance, const ItemValue& options,
                       int32_t pixel_format, uint32_t project_generation,
                       const AegpTime& source_time) {
  if (!candidate_matches(value, item, kind, effect_instance, options,
                         pixel_format, project_generation))
    return false;
  if (source_time.scale != 0 &&
      !same_rational(value.time, source_time))
    return false;
  return true;
}

bool select_stage(const ResolveSnapshot& snapshot, void* item, StageKind kind,
                  uint64_t effect_instance, SamplingPolicy policy,
                  const ItemValue& options, int32_t pixel_format,
                  uint32_t project_generation, StagedItemWorld& selected,
                  const AegpTime& source_time = AegpTime{0, 0}) {
  const StagedItemWorld* best = nullptr;
  RationalDistance best_distance{};
  for (const auto& candidate : snapshot.worlds) {
    if (!candidate_matches(candidate, item, kind, effect_instance, options,
                           pixel_format, project_generation))
      continue;
    if (source_time.scale != 0 &&
        !same_rational(candidate.time, source_time))
      continue;
    if (policy == SamplingPolicy::exact &&
        !same_rational(candidate.time, options.time))
      continue;
    if (policy == SamplingPolicy::hold &&
        compare_rational(candidate.time, options.time) > 0)
      continue;
    const RationalDistance distance =
        rational_distance(candidate.time, options.time);
    bool replace = best == nullptr;
    if (best) {
      const int source_order = compare_rational(candidate.time, best->time);
      if (policy == SamplingPolicy::hold) {
        replace = source_order > 0;
      } else if (policy == SamplingPolicy::nearest) {
        const int distance_order = compare_positive_fractions(
            distance.numerator, distance.denominator,
            best_distance.numerator, best_distance.denominator);
        replace = distance_order < 0 ||
            (distance_order == 0 && source_order < 0);
      } else {
        replace = candidate.stage_generation > best->stage_generation;
      }
    }
    if (replace) {
      best = &candidate;
      best_distance = distance;
    }
  }
  if (!best) {
    ++g_cache_misses;
    ++g_unavailable_frames;
    return false;
  }
  selected = *best;
  ++g_cache_hits;
  if (policy == SamplingPolicy::exact) ++g_exact_hits;
  else if (policy == SamplingPolicy::hold) ++g_hold_hits;
  else ++g_nearest_hits;
  return true;
}

bool record_resolved_stage(ResolveContext& context,
                           const StagedItemWorld& stage, uint32_t depth) {
  if (std::chrono::steady_clock::now() > context.deadline)
    return resolve_failure(context, g_time_limit_rejections);
  if (context.resolved_stages >= kMaxResolvedStages)
    return resolve_failure(context, g_stage_limit_rejections);
  ++context.resolved_stages;
  context.max_depth = (std::max)(context.max_depth, depth);
  context.trace_hash = hash_mix(context.trace_hash,
                                stage_identity_hash(stage.identity));
  return true;
}

bool resolve_source_time(const ResolveSnapshot& snapshot, void* item,
                         SamplingPolicy policy, const ItemValue& options,
                         int32_t pixel_format, uint32_t project_generation,
                         AegpTime& source_time) {
  source_time = AegpTime{0, 0};
  for (const auto& candidate : snapshot.worlds) {
    if (!candidate_matches(candidate, item, StageKind::final_item, 0, options,
                           pixel_format, project_generation))
      continue;
    if (policy == SamplingPolicy::exact &&
        !same_rational(candidate.time, options.time))
      continue;
    if (policy == SamplingPolicy::hold &&
        compare_rational(candidate.time, options.time) > 0)
      continue;
    if (source_time.scale == 0) {
      source_time = candidate.time;
      continue;
    }
    const int source_order = compare_rational(candidate.time, source_time);
    if (policy == SamplingPolicy::hold) {
      if (source_order > 0) source_time = candidate.time;
    } else if (policy == SamplingPolicy::nearest) {
      const RationalDistance candidate_distance =
          rational_distance(candidate.time, options.time);
      const RationalDistance current_distance =
          rational_distance(source_time, options.time);
      const int distance_order = compare_positive_fractions(
          candidate_distance.numerator, candidate_distance.denominator,
          current_distance.numerator, current_distance.denominator);
      if (distance_order < 0 ||
          (distance_order == 0 && compare_rational(candidate.time, source_time) < 0))
        source_time = candidate.time;
    }
  }
  return source_time.scale != 0;
}

bool resolve_item(ResolveContext& context, void* item, uint32_t depth,
                  StagedItemWorld& final_stage, SamplingPolicy& final_policy,
                  bool& registered_item) {
  if (std::chrono::steady_clock::now() > context.deadline)
    return resolve_failure(context, g_time_limit_rejections);
  if (depth >= kMaxResolveDepth)
    return resolve_failure(context, g_depth_limit_rejections);
  const auto active = std::find(context.chain.begin(), context.chain.end(), item);
  if (active != context.chain.end()) {
    ++g_cycles_rejected;
    if (!context.chain.empty() && context.chain.back() == item)
      return resolve_failure(context, g_direct_cycles_rejected);
    return resolve_failure(context, g_indirect_cycles_rejected);
  }
  const ItemRegistration* registration =
      find_registration(context.snapshot, item);
  const SamplingPolicy policy =
      registration ? registration->policy : SamplingPolicy::exact;
  registered_item = registration != nullptr;
  context.chain.push_back(item);
  struct PopChain {
    std::vector<void*>& chain;
    ~PopChain() { chain.pop_back(); }
  } pop{context.chain};
  if (registration) {
    for (void* dependency : registration->dependencies) {
      StagedItemWorld dependency_final{};
      SamplingPolicy dependency_policy{};
      bool dependency_registered{};
      if (!resolve_item(context, dependency, depth + 1, dependency_final,
                        dependency_policy, dependency_registered))
        return false;
    }
  }
  AegpTime resolved_time{0, 0};
  if (policy != SamplingPolicy::exact) {
    if (!resolve_source_time(context.snapshot, item, policy, context.options,
                             context.pixel_format, context.project_generation,
                             resolved_time))
      return resolve_failure(context, g_unavailable_frames);
  }
  if (registration) {
    for (uint64_t effect_instance : registration->effect_instances) {
      for (StageKind kind : {StageKind::upstream, StageKind::all_effects,
                             StageKind::downstream}) {
        StagedItemWorld boundary{};
        if (!select_stage(context.snapshot, item, kind, effect_instance, policy,
                          context.options, context.pixel_format,
                          context.project_generation, boundary,
                          resolved_time))
          return resolve_failure(context, g_effect_boundary_rejections);
        if (!record_resolved_stage(context, boundary, depth)) return false;
      }
    }
  }
  if (!select_stage(context.snapshot, item, StageKind::final_item, 0, policy,
                    context.options, context.pixel_format,
                    context.project_generation, final_stage,
                    resolved_time))
    return false;
  if (!record_resolved_stage(context, final_stage, depth)) return false;
  final_policy = policy;
  return true;
}

bool resolve_plan(const ItemValue& options, ResolvedPlan& plan) {
  const int32_t pixel_format = options.world_type == 1
      ? world_registry::kPixelFormatArgb32
      : (options.world_type == 2 ? world_registry::kPixelFormatArgb64
                                 : (options.world_type == 3
                                        ? world_registry::kPixelFormatArgb128
                                        : 0));
  if (!pixel_format || !g_hooks.project_generation || !options.item ||
      options.time.scale == 0 || options.time_step.scale == 0 ||
      options.time_step.value <= 0 || options.render_quality < 0 ||
      options.render_quality > 1 || options.render_guide_layers > 1 ||
      options.downsample_x <= 0 || options.downsample_y <= 0 ||
      options.field < 0 || options.field > 2 ||
      options.matte < 0 || options.matte > 2 ||
      options.channel_order < 0 || options.channel_order > 1)
    return false;
  const uint32_t generation = g_hooks.project_generation();
  if (generation == 0) return false;
  ensure_generation(generation);
  ResolveSnapshot snapshot;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    snapshot.worlds = g_worlds;
    snapshot.items = g_items;
  }
  ResolveContext context{snapshot, options, pixel_format, generation,
      std::chrono::steady_clock::now() + kMaxResolveTime};
  SamplingPolicy policy{};
  bool registered{};
  if (!resolve_item(context, options.item, 0, plan.final_stage, policy,
                    registered))
    return false;
  plan.policy = policy;
  plan.item_identity = plan.final_stage.item_identity;
  plan.trace_hash = hash_mix(context.trace_hash, static_cast<uint8_t>(policy));
  const auto requested_time = normalize_rational(options.time);
  const auto requested_step = normalize_rational(options.time_step);
  plan.trace_hash = hash_mix(
      plan.trace_hash, static_cast<uint64_t>(requested_time.numerator));
  plan.trace_hash = hash_mix(plan.trace_hash, requested_time.denominator);
  plan.trace_hash = hash_mix(
      plan.trace_hash, static_cast<uint64_t>(requested_step.numerator));
  plan.trace_hash = hash_mix(plan.trace_hash, requested_step.denominator);
  plan.trace_hash = hash_mix(plan.trace_hash, options.field);
  plan.trace_hash = hash_mix(plan.trace_hash, options.world_type);
  plan.trace_hash = hash_mix(plan.trace_hash, options.downsample_x);
  plan.trace_hash = hash_mix(plan.trace_hash, options.downsample_y);
  plan.trace_hash = hash_mix(plan.trace_hash, options.roi.left);
  plan.trace_hash = hash_mix(plan.trace_hash, options.roi.top);
  plan.trace_hash = hash_mix(plan.trace_hash, options.roi.right);
  plan.trace_hash = hash_mix(plan.trace_hash, options.roi.bottom);
  plan.trace_hash = hash_mix(plan.trace_hash, options.matte);
  plan.trace_hash = hash_mix(plan.trace_hash, options.channel_order);
  plan.trace_hash = hash_mix(plan.trace_hash, options.render_guide_layers);
  plan.trace_hash = hash_mix(plan.trace_hash, options.render_quality);
  plan.resolved_stages = context.resolved_stages;
  plan.resolved_depth = context.max_depth;
  plan.registered_item = registered;
  if (const auto* registration =
          find_registration(snapshot, options.item);
      registration && registration->scene_bound) {
    plan.scene_bound = true;
    plan.scene_item = registration->scene_item;
    plan.scene_project = registration->scene_project;
    plan.dependency_identity_hash = 1469598103934665603ULL;
    for (const auto dependency : registration->scene_dependencies)
      plan.dependency_identity_hash =
          hash_identity(plan.dependency_identity_hash, dependency);
    plan.effect_order_hash = 1469598103934665603ULL;
    for (const auto& effect : registration->scene_effects) {
      plan.effect_order_hash =
          hash_identity(plan.effect_order_hash, effect.identity);
      plan.effect_order_hash =
          hash_mix(plan.effect_order_hash, effect.order);
      plan.last_scene_effect = effect.identity;
      plan.last_effect_order = effect.order;
    }
  }
  g_last_trace_hash = plan.trace_hash;
  g_last_stage_identity_hash = stage_identity_hash(plan.final_stage.identity);
  g_last_resolved_stages = plan.resolved_stages;
  update_atomic_max(g_max_resolved_depth, plan.resolved_depth);
  return true;
}

float read_channel(const std::byte* pixel, int32_t pixel_bytes, int channel) {
  if (pixel_bytes == 4)
    return static_cast<float>(reinterpret_cast<const uint8_t*>(pixel)[channel]) / 255.0f;
  if (pixel_bytes == 8)
    return static_cast<float>(reinterpret_cast<const uint16_t*>(pixel)[channel]) / 32768.0f;
  float value = 0.0f;
  std::memcpy(&value, pixel + channel * sizeof(float), sizeof(value));
  return std::isfinite(value) ? value : 0.0f;
}

void write_channel(std::byte* pixel, int32_t pixel_bytes, int channel, float value) {
  value = std::clamp(value, 0.0f, 1.0f);
  if (pixel_bytes == 4)
    reinterpret_cast<uint8_t*>(pixel)[channel] = static_cast<uint8_t>(std::lround(value * 255.0f));
  else if (pixel_bytes == 8)
    reinterpret_cast<uint16_t*>(pixel)[channel] = static_cast<uint16_t>(std::lround(value * 32768.0f));
  else
    std::memcpy(pixel + channel * sizeof(float), &value, sizeof(value));
}

int32_t transform(const StagedItemWorld& stage, const ItemValue& options,
                  const ResolvedPlan& plan,
                  std::unique_ptr<ReceiptDraft>& receipt) {
  const int32_t pixel_bytes = pixel_bytes_for(stage.pixel_format);
  if (!stage.backing || !pixel_bytes || options.downsample_x <= 0 || options.downsample_y <= 0 ||
      options.field < 0 || options.field > 2 || options.matte < 0 || options.matte > 2 ||
      options.channel_order < 0 || options.channel_order > 1) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(stage.rowbytes) * stage.height;
  if (stage.width <= 0 || stage.height <= 0 || stage.rowbytes < stage.width * pixel_bytes ||
      source_bytes != stage.backing->size()) return 4;
  const int32_t width = (stage.width + options.downsample_x - 1) / options.downsample_x;
  const int32_t height = (stage.height + options.downsample_y - 1) / options.downsample_y;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if (bytes == 0 || bytes > render_receipts::kMaxReceiptBytes) return 4;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
  } catch (...) { return 4; }
  receipt->staged_source_pin = stage.backing;
  const bool zero_roi = options.roi.left == 0 && options.roi.top == 0 &&
      options.roi.right == 0 && options.roi.bottom == 0;
  const AegpRect roi = zero_roi ? AegpRect{0, 0, stage.width, stage.height} :
      AegpRect{(std::max)(0, options.roi.left), (std::max)(0, options.roi.top),
          (std::min)(stage.width, options.roi.right), (std::min)(stage.height, options.roi.bottom)};
  const float background = 0x3030 / 65535.0f;
  for (int32_t y = 0; y < height; ++y) {
    const int32_t source_y = y * options.downsample_y;
    for (int32_t x = 0; x < width; ++x) {
      const int32_t source_x = x * options.downsample_x;
      const bool in_roi = source_x >= roi.left && source_x < roi.right &&
          source_y >= roi.top && source_y < roi.bottom;
      const bool in_field = options.field == 0 || (options.field == 1 && (source_y & 1) == 0) ||
          (options.field == 2 && (source_y & 1) != 0);
      if (!in_roi || !in_field) continue;
      const std::byte* source = stage.backing->data() +
          static_cast<std::size_t>(source_y) * stage.rowbytes +
          static_cast<std::size_t>(source_x) * pixel_bytes;
      std::array<float, 4> argb{};
      for (int c = 0; c < 4; ++c) argb[c] = read_channel(source, pixel_bytes, c);
      if (options.matte == 1)
        for (int c = 1; c < 4; ++c) argb[c] *= argb[0];
      else if (options.matte == 2)
        for (int c = 1; c < 4; ++c)
          argb[c] = argb[c] * argb[0] + background * (1.0f - argb[0]);
      const std::array<float, 4> packed = options.channel_order == 0 ? argb :
          std::array<float, 4>{{argb[3], argb[2], argb[1], argb[0]}};
      std::byte* destination = receipt->pixels.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      for (int c = 0; c < 4; ++c) write_channel(destination, pixel_bytes, c, packed[c]);
    }
  }
  const auto ceil_div = [](int32_t value, int32_t divisor) {
    return value <= 0 ? 0 : (value + divisor - 1) / divisor;
  };
  receipt->pixel_format = stage.pixel_format;
  receipt->has_render_options = true;
  receipt->render_options = options;
  receipt->rendered_region = {ceil_div(roi.left, options.downsample_x),
      ceil_div(roi.top, options.downsample_y), ceil_div(roi.right, options.downsample_x),
      ceil_div(roi.bottom, options.downsample_y)};
  receipt->render_timestamp = stage.project_generation;
  receipt->has_stage_evidence = true;
  receipt->stage_identity_hash = stage_identity_hash(stage.identity);
  receipt->item_identity = plan.item_identity;
  receipt->effect_instance = stage.effect_instance;
  receipt->trace_hash = plan.trace_hash;
  receipt->requested_time = options.time;
  receipt->source_time = stage.time;
  receipt->project_generation = stage.project_generation;
  receipt->resolved_stage_count = plan.resolved_stages;
  receipt->resolved_depth = plan.resolved_depth;
  receipt->stage_kind = static_cast<uint8_t>(stage.stage_kind);
  receipt->sampling_policy = static_cast<uint8_t>(plan.policy);
  receipt->scene_bound = plan.scene_bound;
  receipt->scene_item = plan.scene_item;
  receipt->scene_effect = plan.last_scene_effect;
  receipt->scene_project = plan.scene_project;
  receipt->effect_order = plan.last_effect_order;
  receipt->dependency_identity_hash = plan.dependency_identity_hash;
  receipt->effect_order_hash = plan.effect_order_hash;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = width * pixel_bytes;
  receipt->world.world_flags = pixel_bytes == 4 ? 0 : 1;
  receipt->world.width = width;
  receipt->world.height = height;
  receipt->world.extent_hint = {0, 0, width, height};
  receipt->world.pix_aspect_ratio = {1, 1};
  return 0;
}

}  // namespace

void configure(Hooks hooks) noexcept { g_hooks = hooks; }

void clear() noexcept {
  std::lock_guard<std::mutex> lock(g_mutex);
  invalidate_generation_locked();
}

bool has_item_registration(void* item) noexcept {
  if (!item) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  return std::any_of(g_items.begin(), g_items.end(),
      [item](const auto& value) { return value.item == item; });
}

bool registration_graph_has_cycle(const std::vector<ItemRegistration>& items,
                                  bool& direct) {
  direct = false;
  std::vector<void*> active;
  std::vector<void*> complete;
  const auto find_item = [&](void* item) -> const ItemRegistration* {
    const auto found = std::find_if(items.begin(), items.end(),
        [item](const auto& value) { return value.item == item; });
    return found == items.end() ? nullptr : &*found;
  };
  const auto visit = [&](const auto& self, void* item) -> bool {
    if (std::find(complete.begin(), complete.end(), item) != complete.end())
      return false;
    if (std::find(active.begin(), active.end(), item) != active.end())
      return true;
    active.push_back(item);
    if (const auto* registration = find_item(item)) {
      for (void* dependency : registration->dependencies) {
        if (dependency == item) {
          direct = true;
          return true;
        }
        if (find_item(dependency) && self(self, dependency)) return true;
      }
    }
    active.pop_back();
    complete.push_back(item);
    return false;
  };
  for (const auto& item : items)
    if (visit(visit, item.item)) return true;
  return false;
}

bool register_item(void* item, uint64_t stable_identity, SamplingPolicy policy,
                   void* const* dependencies, std::size_t dependency_count,
                   const uint64_t* effect_instances, std::size_t effect_count) {
  if (!g_hooks.project_generation || !item || stable_identity == 0 ||
      static_cast<uint8_t>(policy) >
          static_cast<uint8_t>(SamplingPolicy::nearest) ||
      dependency_count > kMaxDependenciesPerItem ||
      effect_count > kMaxEffectsPerItem ||
      (dependency_count != 0 && !dependencies) ||
      (effect_count != 0 && !effect_instances))
    return false;
  std::vector<void*> dependency_copy;
  std::vector<uint64_t> effect_copy;
  try {
    if (dependency_count != 0)
      dependency_copy.assign(dependencies, dependencies + dependency_count);
    if (effect_count != 0)
      effect_copy.assign(effect_instances, effect_instances + effect_count);
  } catch (...) {
    return false;
  }
  if (std::any_of(dependency_copy.begin(), dependency_copy.end(),
                  [](void* value) { return value == nullptr; }) ||
      std::any_of(effect_copy.begin(), effect_copy.end(),
                  [](uint64_t value) { return value == 0; }))
    return false;
  auto has_duplicates = [](const auto& values) {
    for (std::size_t index = 0; index < values.size(); ++index)
      if (std::find(values.begin() + index + 1, values.end(), values[index]) !=
          values.end())
        return true;
    return false;
  };
  if (has_duplicates(dependency_copy) || has_duplicates(effect_copy))
    return false;
  const uint32_t generation = g_hooks.project_generation();
  if (generation == 0) return false;
  ensure_generation(generation);
  std::lock_guard<std::mutex> lock(g_mutex);
  if (std::any_of(g_items.begin(), g_items.end(), [&](const auto& value) {
        return value.item != item &&
            value.stable_identity == stable_identity;
      })) {
    ++g_duplicate_identity_rejections;
    return false;
  }
  const auto existing = std::find_if(g_items.begin(), g_items.end(),
      [item](const auto& value) { return value.item == item; });
  std::vector<ItemRegistration> candidate;
  try {
    candidate = g_items;
  } catch (...) {
    return false;
  }
  const auto candidate_existing = std::find_if(
      candidate.begin(), candidate.end(),
      [item](const auto& value) { return value.item == item; });
  ItemRegistration replacement{};
  replacement.item = item;
  replacement.stable_identity = stable_identity;
  replacement.policy = policy;
  replacement.dependencies = std::move(dependency_copy);
  replacement.effect_instances = std::move(effect_copy);
  if (candidate_existing != candidate.end())
    *candidate_existing = replacement;
  else {
    if (candidate.size() >= kMaxRegisteredItems) return false;
    try {
      candidate.push_back(replacement);
    } catch (...) {
      return false;
    }
  }
  bool direct_cycle = false;
  try {
    if (registration_graph_has_cycle(candidate, direct_cycle)) {
      ++g_cycles_rejected;
      ++g_registration_cycle_rejections;
      if (direct_cycle) ++g_direct_cycles_rejected;
      else ++g_indirect_cycles_rejected;
      return false;
    }
  } catch (...) {
    return false;
  }
  if (existing != g_items.end()) {
    if (existing->stable_identity != stable_identity &&
        std::any_of(g_worlds.begin(), g_worlds.end(),
                    [item](const auto& value) { return value.item == item; }))
      return false;
    *existing = std::move(replacement);
    return true;
  }
  try {
    g_items.push_back(std::move(replacement));
  } catch (...) {
    return false;
  }
  return true;
}

bool register_scene_item(
    scene_model::Registry& registry, scene_model::Identity item,
    SamplingPolicy policy, const scene_model::Identity* dependencies,
    std::size_t dependency_count, const OrderedSceneEffect* effects,
    std::size_t effect_count) {
  if (item.kind != scene_model::ObjectKind::item ||
      dependency_count > kMaxDependenciesPerItem ||
      effect_count > kMaxEffectsPerItem ||
      (dependency_count && !dependencies) || (effect_count && !effects)) {
    ++g_invalid_handle_rejections;
    return false;
  }
  scene_model::ObjectSnapshot item_snapshot{};
  scene_model::Identity project{};
  void* item_key = nullptr;
  if (!registry.snapshot(item, item_snapshot) ||
      item_snapshot.item_kind != scene_model::ItemKind::composition ||
      !registry.project_identity(item.project_id, project) ||
      !registry.scheduler_key(item, item_key)) {
    ++g_invalid_handle_rejections;
    return false;
  }
  std::vector<void*> dependency_keys;
  std::vector<scene_model::Identity> dependency_identities;
  std::vector<uint64_t> effect_ids;
  std::vector<OrderedSceneEffect> effect_identities;
  try {
    dependency_keys.reserve(dependency_count);
    dependency_identities.reserve(dependency_count);
    effect_ids.reserve(effect_count);
    effect_identities.reserve(effect_count);
  } catch (...) {
    return false;
  }
  for (std::size_t index = 0; index < dependency_count; ++index) {
    const auto dependency = dependencies[index];
    scene_model::ObjectSnapshot snapshot{};
    void* key = nullptr;
    if (dependency.project_id != item.project_id) {
      ++g_cross_project_rejections;
      return false;
    }
    if (dependency.kind != scene_model::ObjectKind::item ||
        !registry.snapshot(dependency, snapshot) ||
        snapshot.item_kind != scene_model::ItemKind::composition ||
        !registry.scheduler_key(dependency, key)) {
      ++g_invalid_handle_rejections;
      return false;
    }
    if (std::find(dependency_identities.begin(),
                  dependency_identities.end(), dependency) !=
        dependency_identities.end()) {
      ++g_duplicate_identity_rejections;
      return false;
    }
    dependency_keys.push_back(key);
    dependency_identities.push_back(dependency);
  }
  for (std::size_t index = 0; index < effect_count; ++index) {
    const auto effect = effects[index];
    scene_model::ObjectSnapshot effect_snapshot{}, layer_snapshot{},
        comp_snapshot{};
    if (effect.identity.project_id != item.project_id) {
      ++g_cross_project_rejections;
      return false;
    }
    if (effect.order != index ||
        effect.identity.kind != scene_model::ObjectKind::effect ||
        !registry.snapshot(effect.identity, effect_snapshot) ||
        !registry.snapshot(effect_snapshot.owner, layer_snapshot) ||
        layer_snapshot.identity.kind != scene_model::ObjectKind::layer ||
        !registry.snapshot(layer_snapshot.owner, comp_snapshot) ||
        comp_snapshot.identity.kind != scene_model::ObjectKind::composition ||
        comp_snapshot.related_item != item) {
      ++g_identity_mismatch_rejections;
      return false;
    }
    if (std::any_of(effect_identities.begin(), effect_identities.end(),
                    [&](const auto& value) {
                      return value.identity == effect.identity ||
                          value.order == effect.order;
                    })) {
      ++g_duplicate_identity_rejections;
      return false;
    }
    effect_ids.push_back(stable_scene_identity(effect.identity));
    effect_identities.push_back(effect);
  }
  if (!g_hooks.project_generation) return false;
  const uint32_t generation = g_hooks.project_generation();
  if (generation == 0) return false;
  ensure_generation(generation);
  const uint64_t stable_identity = stable_scene_identity(item);
  std::lock_guard<std::mutex> lock(g_mutex);
  if (std::any_of(g_items.begin(), g_items.end(), [&](const auto& value) {
        return value.item != item_key &&
            value.stable_identity == stable_identity;
      })) {
    ++g_duplicate_identity_rejections;
    return false;
  }
  const auto existing = std::find_if(g_items.begin(), g_items.end(),
      [item_key](const auto& value) { return value.item == item_key; });
  if (existing == g_items.end() &&
      g_items.size() >= kMaxRegisteredItems)
    return false;
  if (existing != g_items.end() &&
      existing->stable_identity != stable_identity &&
      std::any_of(g_worlds.begin(), g_worlds.end(),
                  [item_key](const auto& value) {
                    return value.item == item_key;
                  })) {
    ++g_identity_mismatch_rejections;
    return false;
  }
  ItemRegistration replacement{};
  replacement.item = item_key;
  replacement.stable_identity = stable_identity;
  replacement.policy = policy;
  replacement.dependencies = std::move(dependency_keys);
  replacement.effect_instances = std::move(effect_ids);
  replacement.scene_bound = true;
  replacement.scene_item = item;
  replacement.scene_project = project;
  replacement.scene_dependencies = std::move(dependency_identities);
  replacement.scene_effects = std::move(effect_identities);
  std::vector<ItemRegistration> candidate;
  try {
    candidate = g_items;
    const auto staged = std::find_if(
        candidate.begin(), candidate.end(),
        [item_key](const auto& value) { return value.item == item_key; });
    if (staged == candidate.end())
      candidate.push_back(replacement);
    else
      *staged = replacement;
  } catch (...) {
    return false;
  }
  bool direct_cycle = false;
  try {
    if (registration_graph_has_cycle(candidate, direct_cycle)) {
      ++g_cycles_rejected;
      ++g_registration_cycle_rejections;
      if (direct_cycle) ++g_direct_cycles_rejected;
      else ++g_indirect_cycles_rejected;
      return false;
    }
  } catch (...) {
    return false;
  }
  if (existing == g_items.end()) {
    try {
      g_items.push_back(std::move(replacement));
    } catch (...) {
      return false;
    }
  } else {
    *existing = std::move(replacement);
  }
  ++g_typed_registrations;
  return true;
}

bool publish_stage_world_impl(
    void* item, StageKind stage_kind, uint64_t effect_instance,
    AegpTime time, AegpTime time_step, int8_t quality,
    uint8_t guide_layers, int32_t pixel_format, int32_t width,
    int32_t height, int32_t rowbytes, const void* pixels,
    uint64_t* published_identity_hash, bool scene_bound,
    scene_model::Identity scene_item, scene_model::Identity scene_effect,
    scene_model::Identity scene_project, uint32_t effect_order) {
  if (published_identity_hash) *published_identity_hash = 0;
  const int32_t pixel_bytes = pixel_bytes_for(pixel_format);
  const uint64_t tight_rowbytes = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t tight_bytes = tight_rowbytes * height;
  if (!g_hooks.project_generation || !item || time.scale == 0 || time_step.scale == 0 ||
      time_step.value <= 0 || quality < 0 || quality > 1 || guide_layers > 1 || !pixel_bytes ||
      width <= 0 || height <= 0 || width > 4096 || height > 4096 ||
      rowbytes < tight_rowbytes || !pixels || tight_bytes == 0 ||
      tight_bytes > kMaxSchedulerBytes ||
      static_cast<uint8_t>(stage_kind) >
          static_cast<uint8_t>(StageKind::final_item) ||
      (stage_kind == StageKind::final_item && effect_instance != 0) ||
      (stage_kind != StageKind::final_item && effect_instance == 0))
    return false;
  std::shared_ptr<std::vector<std::byte>> backing;
  try {
    backing = std::make_shared<std::vector<std::byte>>(static_cast<std::size_t>(tight_bytes));
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(backing->data() + static_cast<std::size_t>(y) * tight_rowbytes,
          static_cast<const std::byte*>(pixels) + static_cast<std::size_t>(y) * rowbytes,
          static_cast<std::size_t>(tight_rowbytes));
  } catch (...) { return false; }
  const uint32_t project_generation = g_hooks.project_generation();
  if (project_generation == 0) return false;
  ensure_generation(project_generation);
  const uint64_t stage_generation = g_stage_generation.fetch_add(1);
  if (stage_generation == 0) return false;
  std::lock_guard<std::mutex> lock(g_mutex);
  if (scene_bound) {
    const auto registration = std::find_if(
        g_items.begin(), g_items.end(), [&](const auto& value) {
          return value.item == item && value.scene_bound &&
              value.scene_item == scene_item &&
              value.scene_project == scene_project;
        });
    if (registration == g_items.end()) {
      ++g_identity_mismatch_rejections;
      return false;
    }
    if (stage_kind != StageKind::final_item &&
        std::none_of(registration->scene_effects.begin(),
                     registration->scene_effects.end(),
                     [&](const auto& value) {
                       return value.identity == scene_effect &&
                           value.order == effect_order;
                     })) {
      ++g_identity_mismatch_rejections;
      return false;
    }
  }
  const uint64_t item_identity = stable_item_identity_locked(item);
  const auto identity = make_stage_identity(item, item_identity, stage_kind,
      effect_instance, time, time_step, quality, guide_layers, pixel_format,
      width, height, static_cast<int32_t>(tight_rowbytes), project_generation,
      scene_bound, scene_item, scene_effect, scene_project, effect_order);
  if (!identity.time.valid || !identity.time_step.valid) return false;
  StagedItemWorld stage{};
  stage.item = item;
  stage.item_identity = item_identity;
  stage.stage_kind = stage_kind;
  stage.effect_instance = effect_instance;
  stage.time = time;
  stage.time_step = time_step;
  stage.quality = quality;
  stage.guide_layers = guide_layers;
  stage.pixel_format = pixel_format;
  stage.width = width;
  stage.height = height;
  stage.rowbytes = static_cast<int32_t>(tight_rowbytes);
  stage.project_generation = project_generation;
  stage.scene_bound = scene_bound;
  stage.scene_item = scene_item;
  stage.scene_effect = scene_effect;
  stage.scene_project = scene_project;
  stage.effect_order = effect_order;
  stage.identity = identity;
  stage.stage_generation = stage_generation;
  stage.backing = std::move(backing);
  auto same_key = [&](const auto& value) {
    return value.item == item && value.stage_kind == stage_kind &&
        value.effect_instance == effect_instance &&
        same_rational(value.time, time) &&
        same_rational(value.time_step, time_step) && value.quality == quality &&
        value.guide_layers == guide_layers && value.pixel_format == pixel_format &&
        value.width == width && value.height == height && value.rowbytes == tight_rowbytes &&
        value.project_generation == project_generation && value.identity == identity;
  };
  const auto existing = std::find_if(g_worlds.begin(), g_worlds.end(), same_key);
  if (existing != g_worlds.end()) {
    const uint64_t previous_bytes = existing->backing ? existing->backing->size() : 0;
    if (g_world_bytes - previous_bytes > kMaxSchedulerBytes - tight_bytes)
      return false;
    g_world_bytes = g_world_bytes - previous_bytes + tight_bytes;
    *existing = std::move(stage);
  } else {
    while (!g_worlds.empty() &&
           (g_worlds.size() >= kMaxStagedItemWorlds ||
            g_world_bytes > kMaxSchedulerBytes - tight_bytes)) {
      const auto oldest = std::min_element(g_worlds.begin(), g_worlds.end(),
          [](const auto& left, const auto& right) {
            return left.stage_generation < right.stage_generation;
          });
      g_world_bytes -= oldest->backing ? oldest->backing->size() : 0;
      g_worlds.erase(oldest);
      ++g_evictions;
    }
    if (g_worlds.size() >= kMaxStagedItemWorlds ||
        g_world_bytes > kMaxSchedulerBytes - tight_bytes)
      return false;
    try {
      g_worlds.push_back(std::move(stage));
      g_world_bytes += tight_bytes;
    } catch (...) {
      return false;
    }
  }
  if (published_identity_hash)
    *published_identity_hash = stage_identity_hash(identity);
  ++g_published;
  return true;
}

bool publish_stage_world(void* item, StageKind stage_kind,
                         uint64_t effect_instance, AegpTime time,
                         AegpTime time_step, int8_t quality,
                         uint8_t guide_layers, int32_t pixel_format,
                         int32_t width, int32_t height, int32_t rowbytes,
                         const void* pixels,
                         uint64_t* published_identity_hash) {
  return publish_stage_world_impl(
      item, stage_kind, effect_instance, time, time_step, quality,
      guide_layers, pixel_format, width, height, rowbytes, pixels,
      published_identity_hash, false, {}, {}, {}, 0);
}

bool publish_scene_stage_world(
    scene_model::Registry& registry, scene_model::Identity item,
    StageKind stage_kind, scene_model::Identity effect, AegpTime time,
    AegpTime time_step, int8_t quality, uint8_t guide_layers,
    int32_t pixel_format, int32_t width, int32_t height, int32_t rowbytes,
    const void* pixels, uint64_t* published_identity_hash) {
  if (published_identity_hash) *published_identity_hash = 0;
  scene_model::ObjectSnapshot item_snapshot{};
  scene_model::Identity project{};
  void* item_key = nullptr;
  if (item.kind != scene_model::ObjectKind::item ||
      !registry.snapshot(item, item_snapshot) ||
      item_snapshot.item_kind != scene_model::ItemKind::composition ||
      !registry.project_identity(item.project_id, project) ||
      !registry.scheduler_key(item, item_key)) {
    ++g_invalid_handle_rejections;
    return false;
  }
  uint64_t effect_instance = 0;
  uint32_t effect_order = 0;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto registration = std::find_if(
        g_items.begin(), g_items.end(), [&](const auto& value) {
          return value.item == item_key && value.scene_bound &&
              value.scene_item == item;
        });
    if (registration == g_items.end()) {
      ++g_identity_mismatch_rejections;
      return false;
    }
    if (stage_kind == StageKind::final_item) {
      if (effect.kind != scene_model::ObjectKind::none) {
        ++g_identity_mismatch_rejections;
        return false;
      }
    } else {
      const auto ordered = std::find_if(
          registration->scene_effects.begin(),
          registration->scene_effects.end(),
          [&](const auto& value) { return value.identity == effect; });
      if (ordered == registration->scene_effects.end()) {
        ++g_identity_mismatch_rejections;
        return false;
      }
      effect_instance = stable_scene_identity(effect);
      effect_order = ordered->order;
    }
  }
  return publish_stage_world_impl(
      item_key, stage_kind, effect_instance, time, time_step, quality,
      guide_layers, pixel_format, width, height, rowbytes, pixels,
      published_identity_hash, true, item, effect, project, effect_order);
}

bool publish_world(void* item, AegpTime time, AegpTime time_step,
                   int8_t quality, uint8_t guide_layers,
                   int32_t pixel_format, int32_t width, int32_t height,
                   int32_t rowbytes, const void* pixels) {
  return publish_stage_world(item, StageKind::final_item, 0, time, time_step,
      quality, guide_layers, pixel_format, width, height, rowbytes, pixels);
}

int32_t publish_snapshot(const ItemValue& snapshot, void** receipt,
                         bool allow_test_synthetic) {
  if (receipt) *receipt = nullptr;
  if (!receipt || !g_hooks.project_generation) return 4;
  const uint32_t in_flight = g_in_flight.fetch_add(1) + 1;
  update_atomic_max(g_max_in_flight, in_flight);
  struct Cleanup {
    ~Cleanup() {
      --g_in_flight;
      ++g_cleanup_count;
    }
  } cleanup;
  const ItemRenderStackKey key{
      snapshot.item, snapshot.time, g_hooks.project_generation()};
  if (stack_contains(key)) {
    ++g_cycles_rejected;
    ++g_direct_cycles_rejected;
    return 4;
  }
  StackScope scope(key);
  if (!scope.entered) return 4;
  bool registered = false;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    registered = std::any_of(g_items.begin(), g_items.end(),
        [&](const auto& value) { return value.item == snapshot.item; });
  }
  ResolvedPlan plan{};
  if (resolve_plan(snapshot, plan)) {
    std::unique_ptr<ReceiptDraft> draft;
    if (transform(plan.final_stage, snapshot, plan, draft) != 0) return 4;
    return render_receipts::register_receipt(std::move(draft), receipt);
  }
  if (registered || !allow_test_synthetic ||
      !g_hooks.synthetic_receipts_enabled ||
      !g_hooks.synthetic_receipts_enabled() || snapshot.matte == 2 ||
      !g_hooks.publish_synthetic)
    return 4;
  const int32_t pixel_format = snapshot.world_type == 1
      ? world_registry::kPixelFormatArgb32
      : (snapshot.world_type == 2 ? world_registry::kPixelFormatArgb64
                                  : (snapshot.world_type == 3
                                         ? world_registry::kPixelFormatArgb128
                                         : 0));
  return pixel_format
      ? g_hooks.publish_synthetic(pixel_format, receipt, &snapshot)
      : 4;
}

int32_t publish_registered_receipt(const ItemValue& options, void** receipt) {
  return publish_snapshot(options, receipt, false);
}

int32_t publish_receipt(void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  ItemValue snapshot{};
  if (!receipt || !render_options::snapshot_item(options, snapshot)) return 4;
  return publish_snapshot(snapshot, receipt, true);
}

bool verify_recursion_guard(void* item, AegpTime time, void* options, Checkout checkout) {
  if (!item || !options || !checkout || !g_hooks.project_generation) return false;
  void* rejected = reinterpret_cast<void*>(1);
  const ItemRenderStackKey direct{item, time, g_hooks.project_generation()};
  StackScope outer(direct);
  if (!outer.entered || checkout(options, nullptr, nullptr, &rejected) == 0 || rejected) return false;
  StackScope nested_other({reinterpret_cast<void*>(0x7770), time, g_hooks.project_generation()});
  return nested_other.entered && stack_contains(direct);
}

void invalidate_scene_generation(uint64_t project_id,
                                 uint32_t valid_generation) noexcept {
  if (project_id == 0 || valid_generation == 0) return;
  uint32_t invalidated_stages = 0;
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    const auto old_world_count = g_worlds.size();
    g_worlds.erase(std::remove_if(g_worlds.begin(), g_worlds.end(),
        [&](const auto& value) {
          if (!value.scene_bound ||
              value.scene_item.project_id != project_id ||
              value.project_generation == valid_generation)
            return false;
          g_world_bytes -= value.backing ? value.backing->size() : 0;
          return true;
        }), g_worlds.end());
    invalidated_stages =
        static_cast<uint32_t>(old_world_count - g_worlds.size());
    g_items.erase(std::remove_if(g_items.begin(), g_items.end(),
        [&](const auto& value) {
          return value.scene_bound &&
              value.scene_item.project_id == project_id;
        }), g_items.end());
    if (invalidated_stages != 0) {
      g_cache_generation = valid_generation;
      ++g_generation_invalidations;
      g_stale_stage_invalidations += invalidated_stages;
    }
  }
  const auto receipts =
      render_receipts::invalidate_scene_generation(project_id,
                                                   valid_generation);
  g_stale_receipt_invalidations += static_cast<uint32_t>(receipts);
}

Diagnostics diagnostics() noexcept {
  Diagnostics result{};
  result.published = g_published.load();
  result.cache_hits = g_cache_hits.load();
  result.cache_misses = g_cache_misses.load();
  result.cycles_rejected = g_cycles_rejected.load();
  result.generation_invalidations = g_generation_invalidations.load();
  result.evictions = g_evictions.load();
  result.exact_hits = g_exact_hits.load();
  result.hold_hits = g_hold_hits.load();
  result.nearest_hits = g_nearest_hits.load();
  result.unavailable_frames = g_unavailable_frames.load();
  result.direct_cycles_rejected = g_direct_cycles_rejected.load();
  result.indirect_cycles_rejected = g_indirect_cycles_rejected.load();
  result.depth_limit_rejections = g_depth_limit_rejections.load();
  result.stage_limit_rejections = g_stage_limit_rejections.load();
  result.time_limit_rejections = g_time_limit_rejections.load();
  result.effect_boundary_rejections = g_effect_boundary_rejections.load();
  result.partial_failures = g_partial_failures.load();
  result.cleanup_count = g_cleanup_count.load();
  result.in_flight = g_in_flight.load();
  result.max_in_flight = g_max_in_flight.load();
  result.last_trace_hash = g_last_trace_hash.load();
  result.last_stage_identity_hash = g_last_stage_identity_hash.load();
  result.last_resolved_stages = g_last_resolved_stages.load();
  result.max_resolved_depth = g_max_resolved_depth.load();
  result.typed_registrations = g_typed_registrations.load();
  result.identity_mismatch_rejections =
      g_identity_mismatch_rejections.load();
  result.duplicate_identity_rejections =
      g_duplicate_identity_rejections.load();
  result.cross_project_rejections = g_cross_project_rejections.load();
  result.registration_cycle_rejections =
      g_registration_cycle_rejections.load();
  result.stale_stage_invalidations =
      g_stale_stage_invalidations.load();
  result.stale_receipt_invalidations =
      g_stale_receipt_invalidations.load();
  result.invalid_handle_rejections =
      g_invalid_handle_rejections.load() +
      static_cast<uint32_t>(
          render_receipts::statistics().invalid_handle_operations);
  {
    std::lock_guard<std::mutex> lock(g_mutex);
    result.registered_items = static_cast<uint32_t>(g_items.size());
    result.cached_stages = static_cast<uint32_t>(g_worlds.size());
    result.cached_bytes = g_world_bytes;
  }
  return result;
}

/*
  The scheduler intentionally retains no callback that can recursively invoke
  an effect while resolving an item. Producers publish immutable stage worlds;
  resolution validates the complete dependency/boundary plan before a receipt
  is registered.
*/

}  // namespace aexcompat::aegp_staged_item_runtime
