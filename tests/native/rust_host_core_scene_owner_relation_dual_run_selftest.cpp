#include "aexcompat_host_core_adapter.hpp"
#include "worker_aegp_scene_model.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::host_core::ApiV1;
using aexcompat::host_core::SceneOwnerRelationInvocation;
using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;

static_assert(sizeof(AexHostSceneOwnerRelation) == 48);
static_assert(alignof(AexHostSceneOwnerRelation) == 8);
static_assert(offsetof(AexHostSceneOwnerRelation, object) == 0);
static_assert(offsetof(AexHostSceneOwnerRelation, owner) == 24);
static_assert(sizeof(Identity) == sizeof(AexHostSceneIdentity));
static_assert(alignof(Identity) == alignof(AexHostSceneIdentity));

constexpr DWORD kSyntheticOwnerRelationSehCode = 0xE04A6301UL;

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
  switch (kind) {
    case AEX_HOST_SCENE_OBJECT_KIND_PROJECT:
    case AEX_HOST_SCENE_OBJECT_KIND_ITEM:
    case AEX_HOST_SCENE_OBJECT_KIND_COMPOSITION:
    case AEX_HOST_SCENE_OBJECT_KIND_FOLDER:
    case AEX_HOST_SCENE_OBJECT_KIND_FOOTAGE:
    case AEX_HOST_SCENE_OBJECT_KIND_LAYER:
    case AEX_HOST_SCENE_OBJECT_KIND_EFFECT:
    case AEX_HOST_SCENE_OBJECT_KIND_STREAM:
    case AEX_HOST_SCENE_OBJECT_KIND_KEYFRAME:
    case AEX_HOST_SCENE_OBJECT_KIND_VALUE:
      return true;
    default:
      return false;
  }
}

bool IsZeroIdentity(const AexHostSceneIdentity &identity) noexcept {
  return identity.project_id == 0 && identity.object_id == 0 &&
      identity.generation == 0 &&
      identity.kind == AEX_HOST_SCENE_OBJECT_KIND_NONE &&
      identity.reserved[0] == 0 && identity.reserved[1] == 0 &&
      identity.reserved[2] == 0;
}

int32_t ValidateIdentity(const AexHostSceneIdentity &identity) noexcept {
  if (identity.project_id == 0 || identity.object_id == 0 ||
      identity.generation == 0 || identity.reserved[0] != 0 ||
      identity.reserved[1] != 0 || identity.reserved[2] != 0) {
    return AEX_HOST_INVALID_ARGUMENT;
  }
  return IsKnownKind(identity.kind) ? AEX_HOST_OK : AEX_HOST_WRONG_KIND;
}

int32_t MatchIdentity(const AexHostSceneIdentity &current,
                      const AexHostSceneIdentity &candidate) noexcept {
  const int32_t current_validation = ValidateIdentity(current);
  if (current_validation != AEX_HOST_OK) return current_validation;
  const int32_t candidate_validation = ValidateIdentity(candidate);
  if (candidate_validation != AEX_HOST_OK) return candidate_validation;
  if (current.project_id != candidate.project_id)
    return AEX_HOST_WRONG_OWNER;
  if (current.kind != candidate.kind) return AEX_HOST_WRONG_KIND;
  if (current.object_id != candidate.object_id)
    return AEX_HOST_INVALID_HANDLE;
  if (current.generation != candidate.generation)
    return AEX_HOST_STALE_HANDLE;
  return AEX_HOST_OK;
}

int32_t ValidateRelation(
    const AexHostSceneOwnerRelation &relation) noexcept {
  const int32_t object_validation = ValidateIdentity(relation.object);
  if (object_validation != AEX_HOST_OK) return object_validation;
  if (relation.object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT) {
    if (IsZeroIdentity(relation.owner)) return AEX_HOST_OK;
    const int32_t owner_validation = ValidateIdentity(relation.owner);
    return owner_validation == AEX_HOST_OK ? AEX_HOST_WRONG_OWNER
                                           : owner_validation;
  }
  const int32_t owner_validation = ValidateIdentity(relation.owner);
  if (owner_validation != AEX_HOST_OK) return owner_validation;
  if (relation.object.project_id != relation.owner.project_id)
    return AEX_HOST_WRONG_OWNER;
  if (relation.object.object_id == relation.owner.object_id &&
      relation.object.kind == relation.owner.kind)
    return AEX_HOST_WRONG_OWNER;
  return AEX_HOST_OK;
}

int32_t MatchOwnerRelationOracle(
    const AexHostSceneOwnerRelation *current,
    const AexHostSceneOwnerRelation *candidate) noexcept {
  if (current == nullptr || candidate == nullptr)
    return AEX_HOST_INVALID_ARGUMENT;
  const int32_t current_validation = ValidateRelation(*current);
  if (current_validation != AEX_HOST_OK) return current_validation;
  const int32_t candidate_validation = ValidateRelation(*candidate);
  if (candidate_validation != AEX_HOST_OK) return candidate_validation;
  const int32_t object_result =
      MatchIdentity(current->object, candidate->object);
  if (object_result != AEX_HOST_OK) return object_result;
  if (current->object.kind == AEX_HOST_SCENE_OBJECT_KIND_PROJECT)
    return AEX_HOST_OK;
  const int32_t owner_result =
      MatchIdentity(current->owner, candidate->owner);
  return owner_result == AEX_HOST_INVALID_HANDLE ? AEX_HOST_WRONG_OWNER
                                                 : owner_result;
}

AexHostSceneIdentity ToAbi(const Identity &identity) noexcept {
  return AexHostSceneIdentity{
      identity.project_id,
      identity.object_id,
      identity.generation,
      static_cast<uint8_t>(identity.kind),
      {0, 0, 0},
  };
}

AexHostSceneOwnerRelation ToAbi(
    const ObjectSnapshot &snapshot) noexcept {
  return AexHostSceneOwnerRelation{
      ToAbi(snapshot.identity),
      ToAbi(snapshot.owner),
  };
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticOwnerRelationSeh(
    const AexHostSceneOwnerRelation *,
    const AexHostSceneOwnerRelation *) {
  RaiseException(kSyntheticOwnerRelationSehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

void RunValueCase(Verifier &verifier, const AdapterV1 &adapter,
                  const char *label, AexHostSceneOwnerRelation current,
                  AexHostSceneOwnerRelation candidate,
                  int32_t required_code) noexcept {
  const int32_t oracle =
      MatchOwnerRelationOracle(&current, &candidate);
  const SceneOwnerRelationInvocation rust =
      adapter.MatchSceneOwnerRelation(current, candidate);
  verifier.Require(label, oracle == required_code &&
                              rust.return_code == oracle &&
                              rust.exception_code == 0);
}

void RunRawCase(Verifier &verifier, const AdapterV1 &adapter,
                const char *label,
                const AexHostSceneOwnerRelation *current,
                const AexHostSceneOwnerRelation *candidate,
                int32_t required_code) noexcept {
  const int32_t oracle = MatchOwnerRelationOracle(current, candidate);
  const auto rust = AdapterV1::InvokeSceneOwnerRelationRaw(
      adapter.api().scene_owner_relation_match, current, candidate);
  verifier.Require(label, oracle == required_code &&
                              rust.return_code == oracle &&
                              rust.exception_code == 0);
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(
        stderr,
        "usage: rust_host_core_scene_owner_relation_dual_run_selftest "
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
  verifier.Require("owner relation adapter must be fully loaded",
                   adapter.loaded());

  const AexHostSceneOwnerRelationAbiDescriptorV1 descriptor =
      adapter.scene_owner_relation_descriptor();
  verifier.Require(
      "adapter must copy the exact owner relation ABI descriptor",
      AdapterV1::IsCompatibleSceneOwnerRelationDescriptor(descriptor) &&
          descriptor.magic ==
              AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_ABI_DESCRIPTOR_MAGIC &&
          descriptor.abi_version ==
              AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_ABI_VERSION &&
          descriptor.struct_size ==
              sizeof(AexHostSceneOwnerRelationAbiDescriptorV1) &&
          descriptor.relation_size == sizeof(AexHostSceneOwnerRelation) &&
          descriptor.relation_alignment ==
              alignof(AexHostSceneOwnerRelation) &&
          descriptor.capabilities ==
              AEXCOMPAT_HOST_CORE_SCENE_OWNER_RELATION_CAPABILITY_MATCH_V1);

  const auto require_descriptor_rejection =
      [&verifier](const char *label,
                  AexHostSceneOwnerRelationAbiDescriptorV1 candidate) {
        verifier.Require(
            label,
            !AdapterV1::IsCompatibleSceneOwnerRelationDescriptor(candidate));
      };
  AexHostSceneOwnerRelationAbiDescriptorV1 incompatible = descriptor;
  ++incompatible.magic;
  require_descriptor_rejection(
      "wrong owner relation descriptor magic must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.abi_version;
  require_descriptor_rejection(
      "wrong owner relation ABI version must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.struct_size;
  require_descriptor_rejection(
      "wrong owner relation descriptor size must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.relation_size;
  require_descriptor_rejection(
      "wrong owner relation size must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.relation_alignment;
  require_descriptor_rejection(
      "wrong owner relation alignment must fail closed", incompatible);
  incompatible = descriptor;
  incompatible.capabilities = 0;
  require_descriptor_rejection(
      "missing owner relation capability must fail closed", incompatible);
  incompatible = descriptor;
  incompatible.capabilities |= UINT64_C(2);
  require_descriptor_rejection(
      "unknown owner relation capability must fail closed", incompatible);

  ApiV1 incomplete_api = adapter.api();
  incomplete_api.scene_owner_relation_match = nullptr;
  verifier.Require(
      "descriptor-compatible owner relation API must require its function",
      AdapterV1::ValidateApi(incomplete_api) ==
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

  Identity project_a{};
  Identity project_b{};
  ObjectSnapshot project_snapshot{};
  verifier.Require(
      "C++ registry must expose project-root ownership",
      registry.project_identity(1, project_a) &&
          registry.project_identity(2, project_b) &&
          registry.snapshot(project_a, project_snapshot));
  const AexHostSceneOwnerRelation project_root = ToAbi(project_snapshot);
  RunValueCase(verifier, adapter, "project root sentinel must match",
               project_root, project_root, AEX_HOST_OK);

  ObjectSnapshot item_snapshot{};
  ObjectSnapshot comp_snapshot{};
  ObjectSnapshot layer_snapshot{};
  verifier.Require(
      "C++ registry must expose an item-comp-layer owner chain",
      registry.snapshot(registry.active_item(), item_snapshot) &&
          registry.comp_from_item(item_snapshot.identity, comp_snapshot) &&
          registry.layer_by_index(comp_snapshot.identity, 0,
                                  layer_snapshot));
  const AexHostSceneOwnerRelation stale_owner = ToAbi(layer_snapshot);

  Identity replacement_comp{};
  ObjectSnapshot current_layer_snapshot{};
  verifier.Require(
      "C++ invalidation must advance and propagate owner generation",
      registry.invalidate(comp_snapshot.identity, replacement_comp) &&
          replacement_comp.generation ==
              comp_snapshot.identity.generation + 1 &&
          registry.snapshot(layer_snapshot.identity,
                            current_layer_snapshot) &&
          current_layer_snapshot.owner == replacement_comp);
  const AexHostSceneOwnerRelation current = ToAbi(current_layer_snapshot);
  RunValueCase(verifier, adapter, "current owner edge must match",
               current, current, AEX_HOST_OK);
  RunValueCase(verifier, adapter,
               "invalidated C++ owner generation must classify as stale",
               current, stale_owner, AEX_HOST_STALE_HANDLE);

  Identity child_comp{};
  Identity foreign_comp{};
  verifier.Require(
      "C++ registry must expose alternate same-kind owners",
      registry.identity_for_object(
          1, 5002, ObjectKind::composition, child_comp) &&
          registry.identity_for_object(
              2, 5101, ObjectKind::composition, foreign_comp));

  AexHostSceneOwnerRelation candidate = current;
  candidate.owner = ToAbi(child_comp);
  RunValueCase(verifier, adapter,
               "different same-project owner must classify as wrong owner",
               current, candidate, AEX_HOST_WRONG_OWNER);
  candidate = current;
  candidate.owner = ToAbi(foreign_comp);
  RunValueCase(verifier, adapter,
               "foreign C++ owner must classify as wrong owner",
               current, candidate, AEX_HOST_WRONG_OWNER);
  candidate = current;
  candidate.owner.project_id = project_b.project_id;
  RunValueCase(verifier, adapter,
               "cross-project edge must classify as wrong owner",
               current, candidate, AEX_HOST_WRONG_OWNER);
  candidate = current;
  candidate.owner.kind = AEX_HOST_SCENE_OBJECT_KIND_LAYER;
  RunValueCase(verifier, adapter,
               "wrong known owner kind must classify as wrong kind",
               current, candidate, AEX_HOST_WRONG_KIND);
  candidate = current;
  candidate.owner.kind = UINT8_MAX;
  RunValueCase(verifier, adapter,
               "unknown owner kind must fail closed",
               current, candidate, AEX_HOST_WRONG_KIND);
  candidate = current;
  candidate.owner.generation = 0;
  RunValueCase(verifier, adapter,
               "zero owner generation must fail closed",
               current, candidate, AEX_HOST_INVALID_ARGUMENT);
  candidate = current;
  candidate.owner.reserved[1] = 1;
  RunValueCase(verifier, adapter,
               "nonzero owner reserved bytes must fail closed",
               current, candidate, AEX_HOST_INVALID_ARGUMENT);
  candidate = current;
  candidate.owner = candidate.object;
  RunValueCase(verifier, adapter,
               "self-owner edge must fail closed",
               current, candidate, AEX_HOST_WRONG_OWNER);

  candidate = project_root;
  candidate.owner = ToAbi(project_b);
  RunValueCase(verifier, adapter,
               "project root with a nonzero owner must fail closed",
               project_root, candidate, AEX_HOST_WRONG_OWNER);
  RunRawCase(verifier, adapter,
             "null current owner relation must fail closed",
             nullptr, &current, AEX_HOST_INVALID_ARGUMENT);
  RunRawCase(verifier, adapter,
             "null candidate owner relation must fail closed",
             &current, nullptr, AEX_HOST_INVALID_ARGUMENT);

  const auto seh = AdapterV1::InvokeSceneOwnerRelationRaw(
      SyntheticOwnerRelationSeh, &current, &current);
  verifier.Require(
      "owner relation adapter must contain native SEH",
      seh.return_code == AEX_HOST_SEH_FAULT &&
          seh.exception_code == kSyntheticOwnerRelationSehCode);

  if (verifier.failures() != 0) {
    std::fprintf(stderr,
                 "scene owner relation dual-run failures=%d checks=%d\n",
                 verifier.failures(), verifier.checks());
    return 1;
  }
  std::printf(
      "{\"rust_host_core_scene_owner_relation_dual_run\":\"passed\","
      "\"checks\":%d,\"cpp_registry\":true,\"balanced\":true}\n",
      verifier.checks());
  return 0;
}
