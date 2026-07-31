#include "aexcompat_host_core_adapter.hpp"
#include "worker_aegp_scene_model.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace {

using aexcompat::host_core::AdapterLoadStatus;
using aexcompat::host_core::AdapterV1;
using aexcompat::host_core::ApiV1;
using aexcompat::host_core::SceneIdentityInvocation;
using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;

static_assert(sizeof(Identity) == sizeof(AexHostSceneIdentity));
static_assert(alignof(Identity) == alignof(AexHostSceneIdentity));
static_assert(offsetof(Identity, project_id) ==
              offsetof(AexHostSceneIdentity, project_id));
static_assert(offsetof(Identity, object_id) ==
              offsetof(AexHostSceneIdentity, object_id));
static_assert(offsetof(Identity, generation) ==
              offsetof(AexHostSceneIdentity, generation));
static_assert(offsetof(Identity, kind) ==
              offsetof(AexHostSceneIdentity, kind));
static_assert(offsetof(Identity, reserved) ==
              offsetof(AexHostSceneIdentity, reserved));
static_assert(static_cast<uint8_t>(ObjectKind::none) ==
              AEX_HOST_SCENE_OBJECT_KIND_NONE);
static_assert(static_cast<uint8_t>(ObjectKind::project) ==
              AEX_HOST_SCENE_OBJECT_KIND_PROJECT);
static_assert(static_cast<uint8_t>(ObjectKind::item) ==
              AEX_HOST_SCENE_OBJECT_KIND_ITEM);
static_assert(static_cast<uint8_t>(ObjectKind::composition) ==
              AEX_HOST_SCENE_OBJECT_KIND_COMPOSITION);
static_assert(static_cast<uint8_t>(ObjectKind::folder) ==
              AEX_HOST_SCENE_OBJECT_KIND_FOLDER);
static_assert(static_cast<uint8_t>(ObjectKind::footage) ==
              AEX_HOST_SCENE_OBJECT_KIND_FOOTAGE);
static_assert(static_cast<uint8_t>(ObjectKind::layer) ==
              AEX_HOST_SCENE_OBJECT_KIND_LAYER);
static_assert(static_cast<uint8_t>(ObjectKind::effect) ==
              AEX_HOST_SCENE_OBJECT_KIND_EFFECT);
static_assert(static_cast<uint8_t>(ObjectKind::stream) ==
              AEX_HOST_SCENE_OBJECT_KIND_STREAM);
static_assert(static_cast<uint8_t>(ObjectKind::keyframe) ==
              AEX_HOST_SCENE_OBJECT_KIND_KEYFRAME);
static_assert(static_cast<uint8_t>(ObjectKind::value) ==
              AEX_HOST_SCENE_OBJECT_KIND_VALUE);

constexpr DWORD kSyntheticSceneIdentitySehCode = 0xE04A6281UL;

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

int32_t MatchIdentityOracle(const AexHostSceneIdentity *current,
                            const AexHostSceneIdentity *candidate) noexcept {
  if (current == nullptr || candidate == nullptr) {
    return AEX_HOST_INVALID_ARGUMENT;
  }
  const auto canonical = [](const AexHostSceneIdentity &identity) noexcept {
    return identity.project_id != 0 && identity.object_id != 0 &&
        identity.generation != 0 && identity.reserved[0] == 0 &&
        identity.reserved[1] == 0 && identity.reserved[2] == 0;
  };
  if (!canonical(*current) || !canonical(*candidate)) {
    return AEX_HOST_INVALID_ARGUMENT;
  }
  if (!IsKnownKind(current->kind) || !IsKnownKind(candidate->kind)) {
    return AEX_HOST_WRONG_KIND;
  }
  if (current->project_id != candidate->project_id) {
    return AEX_HOST_WRONG_OWNER;
  }
  if (current->kind != candidate->kind) {
    return AEX_HOST_WRONG_KIND;
  }
  if (current->object_id != candidate->object_id) {
    return AEX_HOST_INVALID_HANDLE;
  }
  if (current->generation != candidate->generation) {
    return AEX_HOST_STALE_HANDLE;
  }
  return AEX_HOST_OK;
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

bool FindItemKind(Registry &registry, Identity project, ObjectKind kind,
                  Identity excluded, Identity &output) noexcept {
  ObjectSnapshot snapshot{};
  if (!registry.first_project_item(project, snapshot)) {
    return false;
  }
  for (std::size_t count = 0;
       count < aexcompat::scene_model::kObjectCapacity; ++count) {
    if (snapshot.identity.kind == kind && snapshot.identity != excluded) {
      output = snapshot.identity;
      return true;
    }
    const Identity previous = snapshot.identity;
    if (!registry.next_project_item(project, previous, snapshot)) {
      return false;
    }
  }
  return false;
}

int32_t AEXCOMPAT_HOST_CORE_CALL SyntheticSceneIdentitySeh(
    const AexHostSceneIdentity *, const AexHostSceneIdentity *) {
  RaiseException(kSyntheticSceneIdentitySehCode, 0, 0, nullptr);
  return AEX_HOST_OK;
}

void RunValueCase(Verifier &verifier, const AdapterV1 &adapter,
                  const char *label, AexHostSceneIdentity current,
                  AexHostSceneIdentity candidate,
                  int32_t required_code) noexcept {
  const int32_t oracle = MatchIdentityOracle(&current, &candidate);
  const SceneIdentityInvocation rust =
      adapter.MatchSceneIdentity(current, candidate);
  verifier.Require(label, oracle == required_code &&
                              rust.return_code == oracle &&
                              rust.exception_code == 0);
}

void RunRawCase(Verifier &verifier, const AdapterV1 &adapter,
                const char *label, const AexHostSceneIdentity *current,
                const AexHostSceneIdentity *candidate,
                int32_t required_code) noexcept {
  const int32_t oracle = MatchIdentityOracle(current, candidate);
  const auto rust = AdapterV1::InvokeSceneIdentityRaw(
      adapter.api().scene_identity_match, current, candidate);
  verifier.Require(label, oracle == required_code &&
                              rust.return_code == oracle &&
                              rust.exception_code == 0);
}

}  // namespace

int wmain(int argc, wchar_t **argv) {
  if (argc != 2) {
    std::fprintf(
        stderr,
        "usage: rust_host_core_scene_identity_dual_run_selftest <dll-path>\n");
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
  verifier.Require("scene identity adapter must be fully loaded",
                   adapter.loaded());

  const AexHostSceneIdentityAbiDescriptorV1 descriptor =
      adapter.scene_identity_descriptor();
  verifier.Require(
      "adapter must copy the exact scene identity ABI descriptor",
      AdapterV1::IsCompatibleSceneIdentityDescriptor(descriptor) &&
          descriptor.magic ==
              AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_ABI_DESCRIPTOR_MAGIC &&
          descriptor.abi_version ==
              AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_ABI_VERSION &&
          descriptor.struct_size ==
              sizeof(AexHostSceneIdentityAbiDescriptorV1) &&
          descriptor.identity_size == sizeof(AexHostSceneIdentity) &&
          descriptor.identity_alignment == alignof(AexHostSceneIdentity) &&
          descriptor.capabilities ==
              AEXCOMPAT_HOST_CORE_SCENE_IDENTITY_CAPABILITY_MATCH_V1);

  const auto require_descriptor_rejection =
      [&verifier](const char *label,
                  AexHostSceneIdentityAbiDescriptorV1 candidate) {
        verifier.Require(
            label,
            !AdapterV1::IsCompatibleSceneIdentityDescriptor(candidate));
      };
  AexHostSceneIdentityAbiDescriptorV1 incompatible = descriptor;
  ++incompatible.magic;
  require_descriptor_rejection(
      "wrong scene identity descriptor magic must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.abi_version;
  require_descriptor_rejection(
      "wrong scene identity descriptor ABI version must fail closed",
      incompatible);
  incompatible = descriptor;
  ++incompatible.struct_size;
  require_descriptor_rejection(
      "wrong scene identity descriptor size must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.identity_size;
  require_descriptor_rejection(
      "wrong scene identity size must fail closed", incompatible);
  incompatible = descriptor;
  ++incompatible.identity_alignment;
  require_descriptor_rejection(
      "wrong scene identity alignment must fail closed", incompatible);
  incompatible = descriptor;
  incompatible.capabilities = 0;
  require_descriptor_rejection(
      "missing scene identity capability must fail closed", incompatible);
  incompatible = descriptor;
  incompatible.capabilities |= UINT64_C(2);
  require_descriptor_rejection(
      "unknown scene identity capability must fail closed", incompatible);

  ApiV1 incomplete_api = adapter.api();
  incomplete_api.scene_identity_match = nullptr;
  verifier.Require(
      "descriptor-compatible scene API missing its function must fail closed",
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

  const Identity original = registry.active_item();
  Identity project_a{};
  Identity project_b{};
  verifier.Require("C++ registry must expose both project identities",
                   registry.project_identity(1, project_a) &&
                       registry.project_identity(2, project_b));

  Identity different_object{};
  Identity foreign_project{};
  verifier.Require(
      "C++ registry must expose a same-kind second object",
      FindItemKind(registry, project_a, original.kind, original,
                   different_object));
  verifier.Require(
      "C++ registry must expose a same-kind foreign-project object",
      FindItemKind(registry, project_b, original.kind, {}, foreign_project));

  Identity replacement{};
  verifier.Require("C++ registry invalidation must advance generation",
                   registry.invalidate(original, replacement) &&
                       replacement.project_id == original.project_id &&
                       replacement.object_id == original.object_id &&
                       replacement.kind == original.kind &&
                       replacement.generation == original.generation + 1);

  const AexHostSceneIdentity current = ToAbi(replacement);
  RunValueCase(verifier, adapter, "current identity must match", current,
               current, AEX_HOST_OK);
  RunValueCase(verifier, adapter,
               "invalidated C++ identity must classify as stale", current,
               ToAbi(original), AEX_HOST_STALE_HANDLE);
  RunValueCase(verifier, adapter,
               "foreign C++ project identity must classify as wrong owner",
               current, ToAbi(foreign_project), AEX_HOST_WRONG_OWNER);
  RunValueCase(verifier, adapter,
               "different C++ object must classify as invalid handle", current,
               ToAbi(different_object), AEX_HOST_INVALID_HANDLE);

  AexHostSceneIdentity candidate = current;
  candidate.kind = AEX_HOST_SCENE_OBJECT_KIND_EFFECT;
  RunValueCase(verifier, adapter,
               "different known kind must classify as wrong kind", current,
               candidate, AEX_HOST_WRONG_KIND);
  candidate = current;
  candidate.kind = AEX_HOST_SCENE_OBJECT_KIND_NONE;
  RunValueCase(verifier, adapter,
               "none kind sentinel must classify as wrong kind", current,
               candidate, AEX_HOST_WRONG_KIND);
  candidate = current;
  candidate.kind = UINT8_MAX;
  RunValueCase(verifier, adapter,
               "unknown integer kind must fail closed without an enum cast",
               current, candidate, AEX_HOST_WRONG_KIND);
  candidate = current;
  candidate.project_id = 0;
  RunValueCase(verifier, adapter, "zero project id must fail closed", current,
               candidate, AEX_HOST_INVALID_ARGUMENT);
  candidate = current;
  candidate.object_id = 0;
  RunValueCase(verifier, adapter, "zero object id must fail closed", current,
               candidate, AEX_HOST_INVALID_ARGUMENT);
  candidate = current;
  candidate.generation = 0;
  RunValueCase(verifier, adapter, "zero generation must fail closed", current,
               candidate, AEX_HOST_INVALID_ARGUMENT);
  candidate = current;
  candidate.reserved[2] = 1;
  RunValueCase(verifier, adapter,
               "nonzero reserved identity bytes must fail closed", current,
               candidate, AEX_HOST_INVALID_ARGUMENT);
  RunRawCase(verifier, adapter, "null current identity must fail closed",
             nullptr, &current, AEX_HOST_INVALID_ARGUMENT);
  RunRawCase(verifier, adapter, "null candidate identity must fail closed",
             &current, nullptr, AEX_HOST_INVALID_ARGUMENT);

  const auto seh = AdapterV1::InvokeSceneIdentityRaw(
      SyntheticSceneIdentitySeh, &current, &current);
  verifier.Require(
      "scene identity adapter must contain native SEH",
      seh.return_code == AEX_HOST_SEH_FAULT &&
          seh.exception_code == kSyntheticSceneIdentitySehCode);

  if (verifier.failures() != 0) {
    std::fprintf(stderr, "scene identity dual-run failures=%d checks=%d\n",
                 verifier.failures(), verifier.checks());
    return 1;
  }
  std::printf(
      "{\"rust_host_core_scene_identity_dual_run\":\"passed\","
      "\"checks\":%d,\"cpp_registry\":true,\"balanced\":true}\n",
      verifier.checks());
  return 0;
}
