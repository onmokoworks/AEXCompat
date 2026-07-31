#include "aexcompat_host_core_adapter.hpp"
#include "worker_aegp_scene_model.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::host_core::ApiV1;
using aexcompat::host_core::SceneTopologyInvocation;
using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;

static_assert(sizeof(AexHostSceneTopologyEntry) == 56);
static_assert(sizeof(AexHostSceneTopologySnapshot) == 912);
static_assert(sizeof(AexHostSceneTopologySummary) == 32);
static_assert(sizeof(AexHostSceneTopologyAbiDescriptorV1) == 56);

constexpr DWORD kSyntheticTopologySehCode = 0xE04A6321UL;

class Verifier {
 public:
  void Require(const char *label, bool condition) noexcept {
    ++checks_;
    if (!condition) {
      std::fprintf(stderr, "%s\n", label);
      ++failures_;
    }
  }

  int failures() const noexcept { return failures_; }
  int checks() const noexcept { return checks_; }

 private:
  int failures_ = 0;
  int checks_ = 0;
};

bool IsKnownKind(uint8_t kind) noexcept {
  return kind >= AEX_HOST_SCENE_OBJECT_KIND_PROJECT &&
      kind <= AEX_HOST_SCENE_OBJECT_KIND_VALUE;
}

bool EqualIdentity(const AexHostSceneIdentity &left,
                   const AexHostSceneIdentity &right) noexcept {
  return left.project_id == right.project_id &&
      left.object_id == right.object_id &&
      left.generation == right.generation && left.kind == right.kind &&
      left.reserved[0] == right.reserved[0] &&
      left.reserved[1] == right.reserved[1] &&
      left.reserved[2] == right.reserved[2];
}

bool IsZeroIdentity(const AexHostSceneIdentity &identity) noexcept {
  const AexHostSceneIdentity zero{};
  return EqualIdentity(identity, zero);
}

int32_t ValidateIdentity(const AexHostSceneIdentity &identity) noexcept {
  if (identity.project_id == 0 || identity.object_id == 0 ||
      identity.generation == 0 || identity.reserved[0] != 0 ||
      identity.reserved[1] != 0 || identity.reserved[2] != 0)
    return AEX_HOST_INVALID_ARGUMENT;
  return IsKnownKind(identity.kind) ? AEX_HOST_OK : AEX_HOST_WRONG_KIND;
}

int32_t ValidateRelation(
    const AexHostSceneOwnerRelation &relation) noexcept {
  const int32_t object_code = ValidateIdentity(relation.object);
  if (object_code != AEX_HOST_OK) return object_code;
  if (relation.object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
    if (IsZeroIdentity(relation.owner)) return AEX_HOST_OK;
    const int32_t owner_code = ValidateIdentity(relation.owner);
    return owner_code == AEX_HOST_OK ? AEX_HOST_WRONG_OWNER : owner_code;
  }
  const int32_t owner_code = ValidateIdentity(relation.owner);
  if (owner_code != AEX_HOST_OK) return owner_code;
  if (relation.object.project_id != relation.owner.project_id ||
      (relation.object.object_id == relation.owner.object_id &&
       relation.object.kind == relation.owner.kind))
    return AEX_HOST_WRONG_OWNER;
  return AEX_HOST_OK;
}

bool IsZeroEntry(const AexHostSceneTopologyEntry &entry) noexcept {
  return IsZeroIdentity(entry.relation.object) &&
      IsZeroIdentity(entry.relation.owner) && entry.local_index == 0 &&
      entry.reserved == 0;
}

const AexHostSceneTopologyEntry *FindEntry(
    const AexHostSceneTopologySnapshot &snapshot,
    const AexHostSceneIdentity &identity) noexcept {
  const std::size_t count = static_cast<std::size_t>(snapshot.entry_count);
  for (std::size_t index = 0; index < count; ++index)
    if (EqualIdentity(snapshot.entries[index].relation.object, identity))
      return &snapshot.entries[index];
  return nullptr;
}

uint64_t MixU64(uint64_t hash, uint64_t value) noexcept {
  for (unsigned shift = 0; shift < 64; shift += 8) {
    hash ^= (value >> shift) & UINT64_C(0xff);
    hash *= UINT64_C(0x00000100000001b3);
  }
  return hash;
}

uint64_t MixIdentity(uint64_t hash,
                     const AexHostSceneIdentity &identity) noexcept {
  hash = MixU64(hash, identity.project_id);
  hash = MixU64(hash, identity.object_id);
  hash = MixU64(hash, identity.generation);
  return MixU64(hash, identity.kind);
}

int32_t SummarizeOracle(const AexHostSceneTopologySnapshot *snapshot,
                        AexHostSceneTopologySummary *summary) noexcept {
  if (summary == nullptr) return AEX_HOST_INVALID_ARGUMENT;
  *summary = {};
  if (snapshot == nullptr) return AEX_HOST_INVALID_ARGUMENT;
  if (snapshot->project_id == 0 || snapshot->entry_count == 0 ||
      snapshot->reserved != 0)
    return AEX_HOST_INVALID_ARGUMENT;
  if (snapshot->entry_count > AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY)
    return AEX_HOST_CAPACITY_EXCEEDED;

  const std::size_t count =
      static_cast<std::size_t>(snapshot->entry_count);
  for (std::size_t index = count;
       index < AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY; ++index)
    if (!IsZeroEntry(snapshot->entries[index]))
      return AEX_HOST_INVALID_ARGUMENT;

  uint32_t roots = 0;
  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot->entries[index];
    if (entry.reserved != 0) return AEX_HOST_INVALID_ARGUMENT;
    const int32_t relation_code = ValidateRelation(entry.relation);
    if (relation_code != AEX_HOST_OK) return relation_code;
    if (entry.relation.object.project_id != snapshot->project_id)
      return AEX_HOST_WRONG_OWNER;
    if (entry.relation.object.kind ==
        AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
      if (entry.local_index != -1) return AEX_HOST_INVALID_ARGUMENT;
      ++roots;
    } else if (entry.local_index < 0) {
      return AEX_HOST_INVALID_ARGUMENT;
    }
  }
  if (roots != 1) return AEX_HOST_INVALID_STATE;

  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot->entries[index];
    for (std::size_t previous = 0; previous < index; ++previous) {
      const auto &candidate = snapshot->entries[previous];
      if (candidate.relation.object.project_id ==
              entry.relation.object.project_id &&
          candidate.relation.object.object_id ==
              entry.relation.object.object_id)
        return AEX_HOST_INVALID_ARGUMENT;
    }
    if (entry.relation.object.kind ==
        AEX_HOST_SCENE_OBJECT_KIND_PROJECT)
      continue;
    if (FindEntry(*snapshot, entry.relation.owner) == nullptr)
      return AEX_HOST_WRONG_OWNER;
    for (std::size_t previous = 0; previous < index; ++previous) {
      const auto &candidate = snapshot->entries[previous];
      if (EqualIdentity(candidate.relation.owner, entry.relation.owner) &&
          candidate.relation.object.kind == entry.relation.object.kind &&
          candidate.local_index == entry.local_index)
        return AEX_HOST_INVALID_ARGUMENT;
    }
  }

  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot->entries[index];
    if (entry.relation.object.kind ==
        AEX_HOST_SCENE_OBJECT_KIND_PROJECT)
      continue;
    AexHostSceneIdentity owner = entry.relation.owner;
    std::size_t hops = 0;
    while (owner.kind != AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
      if (owner.project_id == entry.relation.object.project_id &&
          owner.object_id == entry.relation.object.object_id)
        return AEX_HOST_WRONG_OWNER;
      const auto *owner_entry = FindEntry(*snapshot, owner);
      if (owner_entry == nullptr) return AEX_HOST_WRONG_OWNER;
      owner = owner_entry->relation.owner;
      if (++hops >= count) return AEX_HOST_WRONG_OWNER;
    }
  }

  std::array<std::size_t,
             AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY> order{};
  for (std::size_t index = 0; index < count; ++index) order[index] = index;
  std::sort(order.begin(), order.begin() + count,
            [snapshot](std::size_t left, std::size_t right) noexcept {
              const auto &a = snapshot->entries[left].relation.object;
              const auto &b = snapshot->entries[right].relation.object;
              if (a.project_id != b.project_id)
                return a.project_id < b.project_id;
              if (a.object_id != b.object_id)
                return a.object_id < b.object_id;
              if (a.generation != b.generation)
                return a.generation < b.generation;
              return a.kind < b.kind;
            });

  const uint32_t edges = snapshot->entry_count - roots;
  uint64_t fingerprint = UINT64_C(0xcbf29ce484222325);
  fingerprint = MixU64(fingerprint, snapshot->project_id);
  fingerprint = MixU64(fingerprint, snapshot->entry_count);
  fingerprint = MixU64(fingerprint, edges);
  for (std::size_t sorted = 0; sorted < count; ++sorted) {
    const auto &entry = snapshot->entries[order[sorted]];
    fingerprint = MixIdentity(fingerprint, entry.relation.object);
    fingerprint = MixIdentity(fingerprint, entry.relation.owner);
    fingerprint = MixU64(
        fingerprint, static_cast<uint32_t>(entry.local_index));
  }
  *summary = AexHostSceneTopologySummary{
      snapshot->project_id, fingerprint, snapshot->entry_count,
      edges, roots, 0};
  return AEX_HOST_OK;
}

AexHostSceneIdentity ToAbi(const Identity &identity) noexcept {
  return AexHostSceneIdentity{
      identity.project_id, identity.object_id, identity.generation,
      static_cast<uint8_t>(identity.kind), {0, 0, 0}};
}

AexHostSceneTopologyEntry ToAbi(
    const ObjectSnapshot &snapshot) noexcept {
  return AexHostSceneTopologyEntry{
      {ToAbi(snapshot.identity), ToAbi(snapshot.owner)},
      snapshot.local_index, 0};
}

struct ObjectSpec {
  uint64_t object_id;
  ObjectKind kind;
};

constexpr std::array<ObjectSpec, 12> kProjectAObjects{{
    {1, ObjectKind::project},
    {100, ObjectKind::folder},
    {101, ObjectKind::folder},
    {200, ObjectKind::footage},
    {1001, ObjectKind::item},
    {5001, ObjectKind::composition},
    {1002, ObjectKind::item},
    {5002, ObjectKind::composition},
    {2001, ObjectKind::layer},
    {2002, ObjectKind::layer},
    {2003, ObjectKind::layer},
    {2010, ObjectKind::layer},
}};

bool BuildSnapshot(Registry &registry, bool reverse,
                   AexHostSceneTopologySnapshot &output) noexcept {
  output = {};
  output.project_id = 1;
  output.entry_count =
      static_cast<uint32_t>(kProjectAObjects.size());
  for (std::size_t index = 0; index < kProjectAObjects.size(); ++index) {
    const std::size_t source =
        reverse ? kProjectAObjects.size() - 1 - index : index;
    const auto spec = kProjectAObjects[source];
    Identity identity{};
    if (spec.kind == ObjectKind::project) {
      if (!registry.project_identity(1, identity)) return false;
    } else if (!registry.identity_for_object(
                   1, spec.object_id, spec.kind, identity)) {
      return false;
    }
    ObjectSnapshot snapshot{};
    if (!registry.snapshot(identity, snapshot)) return false;
    output.entries[index] = ToAbi(snapshot);
  }
  return true;
}

std::size_t FindObjectIndex(const AexHostSceneTopologySnapshot &snapshot,
                            uint64_t object_id) noexcept {
  for (std::size_t index = 0; index < snapshot.entry_count; ++index)
    if (snapshot.entries[index].relation.object.object_id == object_id)
      return index;
  return AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY;
}

bool EqualSummary(const AexHostSceneTopologySummary &left,
                  const AexHostSceneTopologySummary &right) noexcept {
  return left.project_id == right.project_id &&
      left.fingerprint == right.fingerprint &&
      left.object_count == right.object_count &&
      left.edge_count == right.edge_count &&
      left.root_count == right.root_count &&
      left.reserved == right.reserved;
}

void RunCase(Verifier &verifier, const AdapterV1 &adapter,
             const char *label, AexHostSceneTopologySnapshot snapshot,
             int32_t expected_code,
             AexHostSceneTopologySummary *observed = nullptr) noexcept {
  AexHostSceneTopologySummary oracle{};
  const int32_t oracle_code = SummarizeOracle(&snapshot, &oracle);
  const SceneTopologyInvocation rust =
      adapter.SummarizeSceneTopology(snapshot);
  const bool summaries_match =
      expected_code == AEX_HOST_OK
          ? EqualSummary(oracle, rust.summary)
          : EqualSummary(rust.summary, AexHostSceneTopologySummary{});
  verifier.Require(label, oracle_code == expected_code &&
                              rust.return_code == oracle_code &&
                              rust.exception_code == 0 &&
                              summaries_match);
  if (observed != nullptr && rust.return_code == AEX_HOST_OK)
    *observed = rust.summary;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticTopologySeh(
    const AexHostSceneTopologySnapshot *,
    AexHostSceneTopologySummary *) {
  RaiseException(kSyntheticTopologySehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(
        stderr,
        "usage: rust_host_core_scene_topology_snapshot_dual_run_selftest "
        "<dll-path>\n");
    return 2;
  }

  Verifier verifier;
  AdapterV1 adapter;
  const AdapterLoadStatus load_status = AdapterV1::Load(argv[1], &adapter);
  if (load_status != AdapterLoadStatus::kOk) {
    std::fprintf(stderr,
                 "host-core adapter load failed: status=%u native=%lu\n",
                 static_cast<unsigned>(load_status), adapter.native_error());
    return 2;
  }
  verifier.Require("topology adapter must be fully loaded",
                   adapter.loaded());

  const auto descriptor = adapter.scene_topology_descriptor();
  verifier.Require(
      "published topology descriptor must match the compiled ABI",
      AdapterV1::IsCompatibleSceneTopologyDescriptor(descriptor));
  const auto reject_descriptor =
      [&verifier](const char *label,
                  AexHostSceneTopologyAbiDescriptorV1 candidate) {
        verifier.Require(
            label,
            !AdapterV1::IsCompatibleSceneTopologyDescriptor(candidate));
      };
  auto incompatible = descriptor;
  ++incompatible.magic;
  reject_descriptor("wrong topology magic must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.abi_version;
  reject_descriptor("wrong topology ABI version must fail closed",
                    incompatible);
  incompatible = descriptor;
  ++incompatible.struct_size;
  reject_descriptor("wrong topology descriptor size must fail closed",
                    incompatible);
  incompatible = descriptor;
  ++incompatible.entry_size;
  reject_descriptor("wrong topology entry size must fail closed",
                    incompatible);
  incompatible = descriptor;
  ++incompatible.snapshot_alignment;
  reject_descriptor("wrong topology snapshot alignment must fail closed",
                    incompatible);
  incompatible = descriptor;
  ++incompatible.summary_size;
  reject_descriptor("wrong topology summary size must fail closed",
                    incompatible);
  incompatible = descriptor;
  ++incompatible.capacity;
  reject_descriptor("wrong topology capacity must fail closed",
                    incompatible);
  incompatible = descriptor;
  incompatible.reserved = 1;
  reject_descriptor("nonzero topology descriptor reserved must fail closed",
                    incompatible);
  incompatible = descriptor;
  incompatible.capabilities |= UINT64_C(4);
  reject_descriptor("unknown topology capability must fail closed",
                    incompatible);

  ApiV1 incomplete = adapter.api();
  incomplete.scene_topology_summarize = nullptr;
  verifier.Require(
      "missing topology function must leave the API incomplete",
      AdapterV1::ValidateApi(incomplete) ==
          AdapterLoadStatus::kMissingExport);

  int primary_item_token = 0;
  int primary_comp_token = 0;
  int primary_layer_tokens[3]{};
  void *primary_layers[3]{
      &primary_layer_tokens[0],
      &primary_layer_tokens[1],
      &primary_layer_tokens[2],
  };
  Registry registry;
  verifier.Require(
      "C++ scene registry fixture must initialize",
      registry.initialize_fixture(&primary_item_token, &primary_comp_token,
                                  primary_layers, 3));

  AexHostSceneTopologySnapshot baseline{};
  AexHostSceneTopologySnapshot reversed{};
  verifier.Require("Registry topology snapshot must be extractable",
                   BuildSnapshot(registry, false, baseline));
  verifier.Require("Registry topology reverse snapshot must be extractable",
                   BuildSnapshot(registry, true, reversed));
  AexHostSceneTopologySummary baseline_summary{};
  AexHostSceneTopologySummary reversed_summary{};
  RunCase(verifier, adapter, "baseline Registry topology must summarize",
          baseline, AEX_HOST_OK, &baseline_summary);
  RunCase(verifier, adapter,
          "topology summary must not depend on enumeration order",
          reversed, AEX_HOST_OK, &reversed_summary);
  verifier.Require(
      "reordered Registry enumeration must keep the canonical summary",
      EqualSummary(baseline_summary, reversed_summary));

  const std::size_t comp_index = FindObjectIndex(baseline, 5001);
  const std::size_t layer0_index = FindObjectIndex(baseline, 2001);
  const std::size_t layer1_index = FindObjectIndex(baseline, 2002);
  verifier.Require("required Registry topology objects must be present",
                   comp_index < baseline.entry_count &&
                       layer0_index < baseline.entry_count &&
                       layer1_index < baseline.entry_count);

  AexHostSceneTopologySnapshot malformed = baseline;
  malformed.entry_count =
      AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY + 1;
  RunCase(verifier, adapter,
          "capacity overflow must fail without truncation", malformed,
          AEX_HOST_CAPACITY_EXCEEDED);

  malformed = baseline;
  malformed.entries[baseline.entry_count] = baseline.entries[comp_index];
  malformed.entries[baseline.entry_count].local_index = 9;
  ++malformed.entry_count;
  RunCase(verifier, adapter, "duplicate identity must fail closed",
          malformed, AEX_HOST_INVALID_ARGUMENT);

  malformed = baseline;
  malformed.entries[layer0_index].relation.owner.object_id = 9999;
  RunCase(verifier, adapter, "missing owner must fail closed", malformed,
          AEX_HOST_WRONG_OWNER);

  malformed = baseline;
  malformed.entries[layer0_index].relation.owner =
      malformed.entries[layer1_index].relation.object;
  malformed.entries[layer1_index].relation.owner =
      malformed.entries[layer0_index].relation.object;
  RunCase(verifier, adapter, "ownership cycle must fail closed", malformed,
          AEX_HOST_WRONG_OWNER);

  malformed = baseline;
  malformed.entries[layer0_index].relation.owner.project_id = 2;
  RunCase(verifier, adapter, "cross-project owner must fail closed",
          malformed, AEX_HOST_WRONG_OWNER);

  malformed = baseline;
  malformed.entries[baseline.entry_count] =
      malformed.entries[layer0_index];
  malformed.entries[baseline.entry_count].relation.object.object_id = 2999;
  ++malformed.entry_count;
  RunCase(verifier, adapter,
          "duplicate sibling local index must fail closed", malformed,
          AEX_HOST_INVALID_ARGUMENT);

  malformed = baseline;
  malformed.entries[layer0_index].reserved = 1;
  RunCase(verifier, adapter,
          "nonzero topology entry reserved must fail closed", malformed,
          AEX_HOST_INVALID_ARGUMENT);

  malformed = baseline;
  malformed.entries[layer0_index].local_index = -1;
  RunCase(verifier, adapter,
          "negative non-root local index must fail closed", malformed,
          AEX_HOST_INVALID_ARGUMENT);

  Identity comp{};
  verifier.Require(
      "Registry must resolve the composition selected by the snapshot",
      registry.identity_for_object(
          1, 5001, ObjectKind::composition, comp));
  Identity replacement_comp{};
  verifier.Require(
      "Registry invalidation must advance composition generation",
      registry.invalidate(comp, replacement_comp) &&
          replacement_comp.generation == comp.generation + 1);
  AexHostSceneTopologySnapshot invalidated{};
  verifier.Require("invalidated Registry topology must be extractable",
                   BuildSnapshot(registry, false, invalidated));
  AexHostSceneTopologySummary invalidated_summary{};
  RunCase(verifier, adapter,
          "invalidated Registry topology must retain summary parity",
          invalidated, AEX_HOST_OK, &invalidated_summary);
  verifier.Require(
      "generation invalidation must change canonical topology state",
      invalidated_summary.fingerprint != baseline_summary.fingerprint);
  const std::size_t invalidated_layer =
      FindObjectIndex(invalidated, 2001);
  verifier.Require(
      "Registry invalidation must propagate the owner generation",
      invalidated_layer < invalidated.entry_count &&
          EqualIdentity(
              invalidated.entries[invalidated_layer].relation.owner,
              ToAbi(replacement_comp)));

  Identity layer2{};
  verifier.Require(
      "Registry must resolve the reordered layer",
      registry.identity_for_object(1, 2003, ObjectKind::layer, layer2));
  verifier.Require("Registry local-index reorder must succeed",
                   registry.update_local_index(layer2, 7));
  AexHostSceneTopologySnapshot reordered{};
  verifier.Require("reordered Registry topology must be extractable",
                   BuildSnapshot(registry, false, reordered));
  AexHostSceneTopologySummary reordered_summary{};
  RunCase(verifier, adapter,
          "reordered Registry topology must retain summary parity",
          reordered, AEX_HOST_OK, &reordered_summary);
  verifier.Require(
      "local-index reorder must change canonical topology state",
      reordered_summary.fingerprint != invalidated_summary.fingerprint);

  const Identity missing{
      1, 999999, 1, ObjectKind::layer, {0, 0, 0}};
  verifier.Require("missing Registry mutation must be rejected",
                   !registry.update_local_index(missing, 3));
  AexHostSceneTopologySnapshot after_rejection{};
  verifier.Require("post-rejection Registry topology must be extractable",
                   BuildSnapshot(registry, false, after_rejection));
  AexHostSceneTopologySummary after_rejection_summary{};
  RunCase(verifier, adapter,
          "rejected mutation state must retain summary parity",
          after_rejection, AEX_HOST_OK, &after_rejection_summary);
  verifier.Require(
      "rejected mutation must preserve canonical topology state",
      EqualSummary(reordered_summary, after_rejection_summary));

  AexHostSceneTopologySummary raw_summary{};
  const auto null_snapshot = AdapterV1::InvokeSceneTopologyRaw(
      adapter.api().scene_topology_summarize, nullptr, &raw_summary);
  verifier.Require(
      "null topology snapshot must fail closed and clear output",
      null_snapshot.return_code == AEX_HOST_INVALID_ARGUMENT &&
          null_snapshot.exception_code == 0 &&
          EqualSummary(raw_summary, AexHostSceneTopologySummary{}));
  const auto null_summary = AdapterV1::InvokeSceneTopologyRaw(
      adapter.api().scene_topology_summarize, &baseline, nullptr);
  verifier.Require("null topology summary must fail closed",
                   null_summary.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       null_summary.exception_code == 0);

  raw_summary = baseline_summary;
  const auto seh = AdapterV1::InvokeSceneTopologyRaw(
      SyntheticTopologySeh, &baseline, &raw_summary);
  verifier.Require(
      "topology adapter must contain native SEH and clear output",
      seh.return_code == AEX_HOST_SEH_FAULT &&
          seh.exception_code == kSyntheticTopologySehCode &&
          EqualSummary(raw_summary, AexHostSceneTopologySummary{}));

  if (verifier.failures() != 0) {
    std::fprintf(stderr,
                 "scene topology dual-run failures=%d checks=%d\n",
                 verifier.failures(), verifier.checks());
    return 1;
  }
  std::printf(
      "{\"rust_host_core_scene_topology_snapshot_dual_run\":\"passed\","
      "\"checks\":%d,\"cpp_registry\":true,\"canonical\":true,"
      "\"overflow_fail_closed\":true}\n",
      verifier.checks());
  return 0;
}
