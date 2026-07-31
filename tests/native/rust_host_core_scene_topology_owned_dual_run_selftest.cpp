#include "aexcompat_host_core_adapter.hpp"
#include "worker_aegp_scene_model.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <thread>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;

constexpr DWORD kSyntheticCreateSehCode = 0xE04A6341UL;
constexpr DWORD kSyntheticQuerySehCode = 0xE04A6342UL;
constexpr DWORD kSyntheticSummarySehCode = 0xE04A6343UL;
constexpr DWORD kSyntheticDestroySehCode = 0xE04A6344UL;

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

AexHostCallContext Context(uint64_t owner, uint64_t thread_token) noexcept {
  return AexHostCallContext{AEXCOMPAT_HOST_CORE_ABI_VERSION,
                            sizeof(AexHostCallContext), owner, thread_token};
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

bool EqualEntry(const AexHostSceneTopologyEntry &left,
                const AexHostSceneTopologyEntry &right) noexcept {
  return EqualIdentity(left.relation.object, right.relation.object) &&
      EqualIdentity(left.relation.owner, right.relation.owner) &&
      left.local_index == right.local_index &&
      left.reserved == right.reserved;
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

bool IsZeroIdentity(const AexHostSceneIdentity &identity) noexcept {
  return EqualIdentity(identity, AexHostSceneIdentity{});
}

bool IsZeroEntry(const AexHostSceneTopologyEntry &entry) noexcept {
  return IsZeroIdentity(entry.relation.object) &&
      IsZeroIdentity(entry.relation.owner) && entry.local_index == 0 &&
      entry.reserved == 0;
}

bool IsKnownKind(uint8_t kind) noexcept {
  return kind >= AEX_HOST_SCENE_OBJECT_KIND_PROJECT &&
      kind <= AEX_HOST_SCENE_OBJECT_KIND_VALUE;
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

const AexHostSceneTopologyEntry *FindEntry(
    const AexHostSceneTopologySnapshot &snapshot,
    const AexHostSceneIdentity &identity) noexcept {
  for (std::size_t index = 0; index < snapshot.entry_count; ++index)
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

int32_t CanonicalizeOracle(
    const AexHostSceneTopologySnapshot &snapshot,
    std::array<AexHostSceneTopologyEntry,
               AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY> &entries,
    AexHostSceneTopologySummary &summary) noexcept {
  entries = {};
  summary = {};
  if (snapshot.project_id == 0 || snapshot.entry_count == 0 ||
      snapshot.reserved != 0)
    return AEX_HOST_INVALID_ARGUMENT;
  if (snapshot.entry_count > AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY)
    return AEX_HOST_CAPACITY_EXCEEDED;
  const std::size_t count = static_cast<std::size_t>(snapshot.entry_count);
  for (std::size_t index = count;
       index < AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY; ++index)
    if (!IsZeroEntry(snapshot.entries[index]))
      return AEX_HOST_INVALID_ARGUMENT;

  uint32_t roots = 0;
  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot.entries[index];
    if (entry.reserved != 0) return AEX_HOST_INVALID_ARGUMENT;
    const int32_t relation_code = ValidateRelation(entry.relation);
    if (relation_code != AEX_HOST_OK) return relation_code;
    if (entry.relation.object.project_id != snapshot.project_id)
      return AEX_HOST_WRONG_OWNER;
    if (entry.relation.object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
      if (entry.local_index != -1) return AEX_HOST_INVALID_ARGUMENT;
      ++roots;
    } else if (entry.local_index < 0) {
      return AEX_HOST_INVALID_ARGUMENT;
    }
  }
  if (roots != 1) return AEX_HOST_INVALID_STATE;

  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot.entries[index];
    for (std::size_t previous = 0; previous < index; ++previous) {
      const auto &candidate = snapshot.entries[previous];
      if (candidate.relation.object.project_id ==
              entry.relation.object.project_id &&
          candidate.relation.object.object_id ==
              entry.relation.object.object_id)
        return AEX_HOST_INVALID_ARGUMENT;
    }
    if (entry.relation.object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT)
      continue;
    if (FindEntry(snapshot, entry.relation.owner) == nullptr)
      return AEX_HOST_WRONG_OWNER;
    for (std::size_t previous = 0; previous < index; ++previous) {
      const auto &candidate = snapshot.entries[previous];
      if (EqualIdentity(candidate.relation.owner, entry.relation.owner) &&
          candidate.relation.object.kind == entry.relation.object.kind &&
          candidate.local_index == entry.local_index)
        return AEX_HOST_INVALID_ARGUMENT;
    }
  }

  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = snapshot.entries[index];
    if (entry.relation.object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT)
      continue;
    AexHostSceneIdentity owner = entry.relation.owner;
    std::size_t hops = 0;
    while (owner.kind != AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
      if (owner.project_id == entry.relation.object.project_id &&
          owner.object_id == entry.relation.object.object_id)
        return AEX_HOST_WRONG_OWNER;
      const auto *owner_entry = FindEntry(snapshot, owner);
      if (owner_entry == nullptr) return AEX_HOST_WRONG_OWNER;
      owner = owner_entry->relation.owner;
      if (++hops >= count) return AEX_HOST_WRONG_OWNER;
    }
  }

  for (std::size_t index = 0; index < count; ++index)
    entries[index] = snapshot.entries[index];
  std::sort(entries.begin(), entries.begin() + count,
            [](const AexHostSceneTopologyEntry &left,
               const AexHostSceneTopologyEntry &right) noexcept {
              const auto &a = left.relation.object;
              const auto &b = right.relation.object;
              if (a.project_id != b.project_id)
                return a.project_id < b.project_id;
              if (a.object_id != b.object_id)
                return a.object_id < b.object_id;
              if (a.generation != b.generation)
                return a.generation < b.generation;
              return a.kind < b.kind;
            });

  const uint32_t edges = snapshot.entry_count - roots;
  uint64_t fingerprint = UINT64_C(0xcbf29ce484222325);
  fingerprint = MixU64(fingerprint, snapshot.project_id);
  fingerprint = MixU64(fingerprint, snapshot.entry_count);
  fingerprint = MixU64(fingerprint, edges);
  for (std::size_t index = 0; index < count; ++index) {
    const auto &entry = entries[index];
    fingerprint = MixIdentity(fingerprint, entry.relation.object);
    fingerprint = MixIdentity(fingerprint, entry.relation.owner);
    fingerprint =
        MixU64(fingerprint, static_cast<uint32_t>(entry.local_index));
  }
  summary = AexHostSceneTopologySummary{snapshot.project_id, fingerprint,
                                        snapshot.entry_count, edges, roots, 0};
  return AEX_HOST_OK;
}

AexHostSceneIdentity ToAbi(const Identity &identity) noexcept {
  return AexHostSceneIdentity{
      identity.project_id, identity.object_id, identity.generation,
      static_cast<uint8_t>(identity.kind), {0, 0, 0}};
}

AexHostSceneTopologyEntry ToAbi(const ObjectSnapshot &snapshot) noexcept {
  return AexHostSceneTopologyEntry{
      {ToAbi(snapshot.identity), ToAbi(snapshot.owner)},
      snapshot.local_index, 0};
}

struct ObjectSpec {
  uint64_t object_id;
  ObjectKind kind;
};

constexpr std::array<ObjectSpec, 12> kObjects{{
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
  output.entry_count = static_cast<uint32_t>(kObjects.size());
  for (std::size_t index = 0; index < kObjects.size(); ++index) {
    const std::size_t source = reverse ? kObjects.size() - 1 - index : index;
    const auto spec = kObjects[source];
    Identity identity{};
    if (spec.kind == ObjectKind::project) {
      if (!registry.project_identity(1, identity)) return false;
    } else if (!registry.identity_for_object(1, spec.object_id, spec.kind,
                                              identity)) {
      return false;
    }
    ObjectSnapshot snapshot{};
    if (!registry.snapshot(identity, snapshot)) return false;
    output.entries[index] = ToAbi(snapshot);
  }
  return true;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticCreateSeh(
    const AexHostCallContext *, const AexHostSceneTopologySnapshot *,
    AexHostOpaqueHandle *) {
  RaiseException(kSyntheticCreateSehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticQuerySeh(
    const AexHostCallContext *, AexHostOpaqueHandle, uint32_t,
    AexHostSceneTopologyEntry *) {
  RaiseException(kSyntheticQuerySehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticSummarySeh(
    const AexHostCallContext *, AexHostOpaqueHandle,
    AexHostSceneTopologySummary *) {
  RaiseException(kSyntheticSummarySehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticDestroySeh(
    const AexHostCallContext *, AexHostOpaqueHandle) {
  RaiseException(kSyntheticDestroySehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(stderr,
                 "usage: rust_host_core_scene_topology_owned_dual_run_"
                 "selftest <dll-path>\n");
    return 2;
  }

  Verifier verifier;
  AdapterV1 adapter;
  verifier.Require("host-core adapter must load",
                   AdapterV1::Load(argv[1], &adapter) ==
                       AdapterLoadStatus::kOk &&
                       adapter.loaded());
  const auto descriptor = adapter.scene_topology_descriptor();
  verifier.Require(
      "owned snapshot capability must be advertised before function casts",
      AdapterV1::IsCompatibleSceneTopologyDescriptor(descriptor) &&
          descriptor.capabilities ==
              (AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_SUMMARY_V1 |
               AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPABILITY_OWNED_SNAPSHOT_V1));

  int item_token = 0;
  int comp_token = 0;
  int layer_tokens[3]{};
  void *layers[3]{&layer_tokens[0], &layer_tokens[1], &layer_tokens[2]};
  Registry registry;
  verifier.Require("C++ Registry fixture must initialize",
                   registry.initialize_fixture(&item_token, &comp_token,
                                               layers, 3));

  AexHostSceneTopologySnapshot baseline{};
  AexHostSceneTopologySnapshot reversed{};
  verifier.Require("Registry snapshot must be extracted",
                   BuildSnapshot(registry, false, baseline));
  verifier.Require("Registry reverse snapshot must be extracted",
                   BuildSnapshot(registry, true, reversed));
  std::array<AexHostSceneTopologyEntry,
             AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY>
      oracle_entries{};
  std::array<AexHostSceneTopologyEntry,
             AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY>
      reverse_entries{};
  AexHostSceneTopologySummary oracle_summary{};
  AexHostSceneTopologySummary reverse_summary{};
  verifier.Require("independent C++ oracle must canonicalize Registry state",
                   CanonicalizeOracle(baseline, oracle_entries,
                                      oracle_summary) == AEX_HOST_OK);
  verifier.Require("independent oracle must canonicalize reverse state",
                   CanonicalizeOracle(reversed, reverse_entries,
                                      reverse_summary) == AEX_HOST_OK);
  verifier.Require("oracle canonical entries must ignore enumeration order",
                   std::equal(oracle_entries.begin(),
                              oracle_entries.begin() + baseline.entry_count,
                              reverse_entries.begin(), EqualEntry));
  verifier.Require("oracle summaries must ignore enumeration order",
                   EqualSummary(oracle_summary, reverse_summary));

  const AexHostCallContext context = Context(63401, 63402);
  const auto created = adapter.CreateSceneTopologySnapshot(context, baseline);
  const auto created_reversed =
      adapter.CreateSceneTopologySnapshot(context, reversed);
  verifier.Require("Rust must create two non-pointer snapshot handles",
                   created.return_code == AEX_HOST_OK &&
                       created.exception_code == 0 && created.handle.value != 0 &&
                       created_reversed.return_code == AEX_HOST_OK &&
                       created_reversed.exception_code == 0 &&
                       created_reversed.handle.value != 0 &&
                       created.handle.value != created_reversed.handle.value);

  std::reverse(baseline.entries,
               baseline.entries + static_cast<std::size_t>(baseline.entry_count));
  bool reordered_entries_match = true;
  for (uint32_t index = 0; index < oracle_summary.object_count; ++index) {
    const auto queried =
        adapter.QuerySceneTopologySnapshot(context, created.handle, index);
    reordered_entries_match = reordered_entries_match &&
        queried.return_code == AEX_HOST_OK && queried.exception_code == 0 &&
        EqualEntry(queried.entry, oracle_entries[index]);
  }
  verifier.Require("canonical queries must survive caller buffer reorder",
                   reordered_entries_match);

  baseline = {};
  reversed = {};
  bool all_entries_match = true;
  for (uint32_t index = 0; index < oracle_summary.object_count; ++index) {
    const auto queried =
        adapter.QuerySceneTopologySnapshot(context, created.handle, index);
    const auto queried_reversed = adapter.QuerySceneTopologySnapshot(
        context, created_reversed.handle, index);
    all_entries_match = all_entries_match &&
        queried.return_code == AEX_HOST_OK && queried.exception_code == 0 &&
        EqualEntry(queried.entry, oracle_entries[index]) &&
        queried_reversed.return_code == AEX_HOST_OK &&
        queried_reversed.exception_code == 0 &&
        EqualEntry(queried_reversed.entry, oracle_entries[index]);
  }
  verifier.Require(
      "canonical queries must survive caller buffer overwrite",
      all_entries_match);
  const auto stored =
      adapter.StoredSceneTopologySummary(context, created.handle);
  const auto stored_reversed =
      adapter.StoredSceneTopologySummary(context, created_reversed.handle);
  verifier.Require("stored Rust summaries must retain independent parity",
                   stored.return_code == AEX_HOST_OK &&
                       stored.exception_code == 0 &&
                       EqualSummary(stored.summary, oracle_summary) &&
                       stored_reversed.return_code == AEX_HOST_OK &&
                       stored_reversed.exception_code == 0 &&
                       EqualSummary(stored_reversed.summary, oracle_summary));

  const auto out_of_range = adapter.QuerySceneTopologySnapshot(
      context, created.handle, oracle_summary.object_count);
  verifier.Require("out-of-range query must fail and clear its output",
                   out_of_range.return_code == AEX_HOST_INVALID_ARGUMENT &&
                       out_of_range.exception_code == 0 &&
                       IsZeroEntry(out_of_range.entry));
  const auto wrong_owner = adapter.StoredSceneTopologySummary(
      Context(63403, 63402), created.handle);
  verifier.Require("foreign owner must fail and clear summary",
                   wrong_owner.return_code == AEX_HOST_WRONG_OWNER &&
                       EqualSummary(wrong_owner.summary, {}));
  const auto wrong_logical = adapter.QuerySceneTopologySnapshot(
      Context(63401, 63404), created.handle, 0);
  verifier.Require("wrong logical thread must fail and clear entry",
                   wrong_logical.return_code == AEX_HOST_WRONG_THREAD &&
                       IsZeroEntry(wrong_logical.entry));

  int32_t foreign_destroy_code = AEX_HOST_INVALID_STATE;
  uint32_t foreign_destroy_exception = UINT32_MAX;
  std::thread foreign_thread([&]() {
    const auto result =
        adapter.DestroySceneTopologySnapshot(context, created.handle);
    foreign_destroy_code = result.return_code;
    foreign_destroy_exception = result.exception_code;
  });
  foreign_thread.join();
  verifier.Require("actual foreign thread destroy must fail closed",
                   foreign_destroy_code == AEX_HOST_WRONG_THREAD &&
                       foreign_destroy_exception == 0);
  verifier.Require(
      "foreign-thread rejection must not destroy the Rust-owned record",
      adapter.QuerySceneTopologySnapshot(context, created.handle, 0)
                  .return_code == AEX_HOST_OK);

  const auto session = adapter.Create(context);
  verifier.Require("session namespace fixture must be created",
                   session.return_code == AEX_HOST_OK &&
                       session.created_handle.value != 0);
  const auto wrong_namespace_query = adapter.QuerySceneTopologySnapshot(
      context, session.created_handle, 0);
  const auto wrong_namespace_destroy = adapter.DestroySceneTopologySnapshot(
      context, session.created_handle);
  verifier.Require(
      "session handle must be rejected by scene query without destruction",
      wrong_namespace_query.return_code == AEX_HOST_INVALID_HANDLE &&
          wrong_namespace_query.exception_code == 0 &&
          IsZeroEntry(wrong_namespace_query.entry) &&
          wrong_namespace_destroy.return_code == AEX_HOST_INVALID_HANDLE &&
          wrong_namespace_destroy.exception_code == 0 &&
          adapter.Open(context, session.created_handle).return_code ==
              AEX_HOST_OK);
  verifier.Require(
      "scene handle must be rejected by session namespace without destruction",
      adapter.Open(context, created.handle).return_code ==
              AEX_HOST_INVALID_HANDLE &&
          adapter.QuerySceneTopologySnapshot(context, created.handle, 0)
                  .return_code == AEX_HOST_OK);
  verifier.Require("session fixture must close and dispose normally",
                   adapter.Close(context, session.created_handle).return_code ==
                           AEX_HOST_OK &&
                       adapter.Dispose(context, session.created_handle)
                               .return_code == AEX_HOST_OK);

  AexHostSceneTopologySnapshot overflow{};
  overflow.project_id = 1;
  overflow.entry_count =
      AEXCOMPAT_HOST_CORE_SCENE_TOPOLOGY_CAPACITY + 1;
  AexHostOpaqueHandle rejected{UINT64_MAX};
  const auto overflow_result = AdapterV1::InvokeSceneTopologySnapshotCreateRaw(
      adapter.api().scene_topology_snapshot_create, &context, &overflow,
      &rejected);
  verifier.Require("capacity overflow must not truncate or publish a handle",
                   overflow_result.return_code ==
                           AEX_HOST_CAPACITY_EXCEEDED &&
                       overflow_result.exception_code == 0 &&
                       rejected.value == 0);

  verifier.Require("first destroy must release the owned snapshot",
                   adapter.DestroySceneTopologySnapshot(context, created.handle)
                               .return_code == AEX_HOST_OK);
  const auto stale_query =
      adapter.QuerySceneTopologySnapshot(context, created.handle, 0);
  verifier.Require("query after destroy must be stale and zeroed",
                   stale_query.return_code == AEX_HOST_STALE_HANDLE &&
                       IsZeroEntry(stale_query.entry));
  verifier.Require("double destroy must be stale",
                   adapter.DestroySceneTopologySnapshot(context, created.handle)
                               .return_code == AEX_HOST_STALE_HANDLE);
  verifier.Require("second independent handle must destroy normally",
                   adapter.DestroySceneTopologySnapshot(
                              context, created_reversed.handle)
                               .return_code == AEX_HOST_OK);

  AexHostOpaqueHandle seh_handle{UINT64_MAX};
  const auto create_seh = AdapterV1::InvokeSceneTopologySnapshotCreateRaw(
      SyntheticCreateSeh, &context, &overflow, &seh_handle);
  AexHostSceneTopologyEntry seh_entry = oracle_entries[0];
  const auto query_seh = AdapterV1::InvokeSceneTopologySnapshotQueryRaw(
      SyntheticQuerySeh, &context, {}, 0, &seh_entry);
  AexHostSceneTopologySummary seh_summary = oracle_summary;
  const auto summary_seh = AdapterV1::InvokeSceneTopologySnapshotSummaryRaw(
      SyntheticSummarySeh, &context, {}, &seh_summary);
  const auto destroy_seh = AdapterV1::InvokeSceneTopologySnapshotDestroyRaw(
      SyntheticDestroySeh, &context, {});
  verifier.Require("create SEH must be contained and clear handle",
                   create_seh.return_code == AEX_HOST_SEH_FAULT &&
                       create_seh.exception_code == kSyntheticCreateSehCode &&
                       seh_handle.value == 0);
  verifier.Require("query SEH must be contained and clear entry",
                   query_seh.return_code == AEX_HOST_SEH_FAULT &&
                       query_seh.exception_code == kSyntheticQuerySehCode &&
                       IsZeroEntry(seh_entry));
  verifier.Require("summary SEH must be contained and clear output",
                   summary_seh.return_code == AEX_HOST_SEH_FAULT &&
                       summary_seh.exception_code ==
                           kSyntheticSummarySehCode &&
                       EqualSummary(seh_summary, {}));
  verifier.Require("destroy SEH must be contained",
                   destroy_seh.return_code == AEX_HOST_SEH_FAULT &&
                       destroy_seh.exception_code ==
                           kSyntheticDestroySehCode);

  if (verifier.failures() != 0) {
    std::fprintf(stderr, "owned topology failures=%d checks=%d\n",
                 verifier.failures(), verifier.checks());
    return 1;
  }
  std::printf(
      "{\"rust_host_core_scene_topology_owned_dual_run\":\"passed\","
      "\"checks\":%d,\"cpp_registry\":true,\"rust_owned\":true,"
      "\"buffer_detached\":true,\"thread_bound\":true,"
      "\"namespace_fail_closed\":true}\n",
      verifier.checks());
  return 0;
}
