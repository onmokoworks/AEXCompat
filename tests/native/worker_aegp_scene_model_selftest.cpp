#include "worker_aegp_scene_model.hpp"

#include <algorithm>
#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstdio>

using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ItemKind;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;
using aexcompat::scene_model::kBorrowedHandleCapacity;

namespace {

struct FixtureStorage {
  int item{};
  int comp{};
  std::array<int, 3> layers{};
  std::array<void*, 3> layer_handles{{
      &layers[0], &layers[1], &layers[2]}};
};

struct alignas(std::max_align_t) ForgedBorrowedToken {
  uint64_t lease_identity{};
};

bool initialize(Registry& registry, FixtureStorage& storage) {
  return registry.initialize_fixture(
      &storage.item, &storage.comp, storage.layer_handles.data(),
      storage.layer_handles.size());
}

bool aligned_token(void* handle) {
  return handle &&
      reinterpret_cast<uintptr_t>(handle) % alignof(std::max_align_t) == 0;
}

}  // namespace

int main() {
  FixtureStorage storage{};
  Registry registry;
  assert(initialize(registry, storage));
  assert(registry.project_count() == 2);
  assert(registry.live_object_count(ObjectKind::folder) == 3);
  assert(registry.live_object_count(ObjectKind::footage) == 1);
  assert(registry.live_object_count(ObjectKind::item) == 3);
  assert(registry.live_object_count(ObjectKind::composition) == 3);
  assert(registry.live_object_count(ObjectKind::layer) == 5);

  ObjectSnapshot root_a{};
  ObjectSnapshot nested_a{};
  ObjectSnapshot footage_a{};
  ObjectSnapshot first_comp_item{};
  ObjectSnapshot second_comp_item{};
  assert(registry.first_child(registry.active_project(), root_a));
  assert(root_a.identity.kind == ObjectKind::folder);
  assert(registry.first_child(root_a.identity, nested_a));
  assert(nested_a.identity.kind == ObjectKind::folder);
  assert(registry.first_child(nested_a.identity, footage_a));
  assert(footage_a.identity.kind == ObjectKind::footage);
  assert(footage_a.item_kind == ItemKind::footage);
  assert(registry.next_sibling(nested_a.identity, first_comp_item));
  assert(first_comp_item.item_kind == ItemKind::composition);
  assert(registry.next_sibling(first_comp_item.identity, second_comp_item));
  assert(second_comp_item.item_kind == ItemKind::composition);

  const Identity active_item = registry.active_item();
  assert(active_item.kind == ObjectKind::item);
  assert(active_item.project_id == 1);
  void* item_handle = registry.borrow(active_item);
  assert(aligned_token(item_handle));
  assert(registry.borrow(active_item) == item_handle);
  ObjectSnapshot item{};
  assert(registry.resolve_item(item_handle, item));
  assert(item.identity == active_item);
  assert(item.item_kind == ItemKind::composition);

  ObjectSnapshot comp{};
  assert(registry.comp_from_item(item.identity, comp));
  void* comp_handle = registry.borrow(comp.identity);
  assert(aligned_token(comp_handle));
  assert(comp_handle != item_handle);
  ObjectSnapshot resolved_comp{};
  assert(registry.resolve(
      comp_handle, ObjectKind::composition, resolved_comp, 1));
  assert(registry.layer_count(resolved_comp.identity) == 3);

  ObjectSnapshot layer{};
  assert(registry.layer_by_index(resolved_comp.identity, 2, layer));
  void* layer_handle = registry.borrow(layer.identity);
  assert(aligned_token(layer_handle));
  ObjectSnapshot resolved_layer{};
  assert(registry.resolve(layer_handle, ObjectKind::layer, resolved_layer, 1));
  assert(resolved_layer.identity.object_id == 2003);
  assert(resolved_layer.owner == resolved_comp.identity);

  const uint64_t before_rejections = registry.fingerprint();
  ObjectSnapshot unchanged{};
  unchanged.identity.object_id = 0xfeed;
  const ObjectSnapshot sentinel = unchanged;
  assert(!registry.resolve(nullptr, ObjectKind::layer, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(layer_handle, ObjectKind::composition, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(
      layer_handle, ObjectKind::layer, unchanged, 2));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(reinterpret_cast<void*>(
      static_cast<uintptr_t>(0x12345678)), ObjectKind::layer, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);

  auto* exposed_lease_identity = static_cast<uint64_t*>(layer_handle);
  const uint64_t observed_lease_identity = *exposed_lease_identity;
  assert(observed_lease_identity != 0);
  ForgedBorrowedToken forged{observed_lease_identity};
  static_assert(
      alignof(ForgedBorrowedToken) >= alignof(std::max_align_t));
  assert(aligned_token(&forged));
  assert(!registry.resolve(&forged, ObjectKind::layer, unchanged, 1));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  *exposed_lease_identity = observed_lease_identity + 1;
  assert(!registry.resolve(
      layer_handle, ObjectKind::layer, unchanged, 1));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  *exposed_lease_identity = observed_lease_identity;
  assert(registry.resolve(
      layer_handle, ObjectKind::layer, resolved_layer, 1));

  FixtureStorage foreign_storage{};
  Registry foreign_registry;
  assert(initialize(foreign_registry, foreign_storage));
  void* foreign_registry_item =
      foreign_registry.borrow(foreign_registry.active_item());
  assert(aligned_token(foreign_registry_item));
  assert(!registry.resolve_item(foreign_registry_item, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!foreign_registry.resolve_item(item_handle, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(registry.fingerprint() == before_rejections);

  Identity replacement{};
  assert(registry.invalidate(layer.identity, replacement));
  assert(replacement.generation == layer.identity.generation + 1);
  assert(!registry.resolve(
      layer_handle, ObjectKind::layer, unchanged, 1));
  void* replacement_handle = registry.borrow(replacement);
  assert(aligned_token(replacement_handle));
  assert(replacement_handle != layer_handle);
  assert(registry.resolve(
      replacement_handle, ObjectKind::layer, resolved_layer, 1));
  assert(resolved_layer.identity == replacement);

  const Identity project_b{2, 2, 1, ObjectKind::project, {}};
  ObjectSnapshot root_b{};
  assert(registry.first_child(project_b, root_b));
  assert(root_b.identity.kind == ObjectKind::folder);
  assert(root_b.identity.project_id == 2);
  ObjectSnapshot item_b{};
  assert(registry.first_child(root_b.identity, item_b));
  assert(item_b.identity.kind == ObjectKind::item);
  ObjectSnapshot comp_b{};
  assert(registry.comp_from_item(item_b.identity, comp_b));
  ObjectSnapshot layer_b{};
  assert(registry.layer_by_index(comp_b.identity, 0, layer_b));
  assert(!registry.layer_from_id(
      resolved_comp.identity, layer_b.identity.object_id, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  void* layer_b_handle = registry.borrow(layer_b.identity);
  assert(aligned_token(layer_b_handle));
  assert(!registry.resolve(
      layer_b_handle, ObjectKind::layer, unchanged, 1));
  assert(registry.resolve(
      layer_b_handle, ObjectKind::layer, resolved_layer, 2));

  FixtureStorage propagation_storage{};
  Registry propagation;
  assert(initialize(propagation, propagation_storage));
  const Identity old_project = propagation.active_project();
  const Identity old_item = propagation.active_item();
  ObjectSnapshot old_comp{};
  ObjectSnapshot old_layer0{};
  ObjectSnapshot old_layer1{};
  assert(propagation.comp_from_item(old_item, old_comp));
  assert(propagation.layer_by_index(old_comp.identity, 0, old_layer0));
  assert(propagation.layer_by_index(old_comp.identity, 1, old_layer1));
  void* old_item_handle = propagation.borrow(old_item);
  void* old_comp_handle = propagation.borrow(old_comp.identity);
  void* old_layer_handle = propagation.borrow(old_layer0.identity);

  const uint64_t before_failed_invalidation = propagation.fingerprint();
  Identity failed_replacement{
      9, 9, 9, ObjectKind::layer, {}};
  const Identity failed_replacement_sentinel = failed_replacement;
  const Identity missing{
      1, 9999, 1, ObjectKind::layer, {}};
  assert(!propagation.invalidate(missing, failed_replacement));
  assert(failed_replacement == failed_replacement_sentinel);
  assert(propagation.fingerprint() == before_failed_invalidation);

  Identity new_item{};
  assert(propagation.invalidate(old_item, new_item));
  assert(propagation.active_item() == new_item);
  assert(!propagation.resolve_item(old_item_handle, unchanged));
  ObjectSnapshot linked_comp{};
  assert(propagation.comp_from_item(new_item, linked_comp));
  assert(linked_comp.identity == old_comp.identity);
  assert(linked_comp.owner == new_item);
  assert(linked_comp.related_item == new_item);
  ObjectSnapshot linked_item{};
  assert(propagation.item_from_comp(linked_comp.identity, linked_item));
  assert(linked_item.identity == new_item);

  Identity new_comp{};
  assert(propagation.invalidate(linked_comp.identity, new_comp));
  assert(!propagation.resolve(
      old_comp_handle, ObjectKind::composition, unchanged));
  assert(propagation.item_from_comp(new_comp, linked_item));
  assert(linked_item.identity == new_item);
  assert(propagation.layer_count(new_comp) == 3);
  ObjectSnapshot linked_layer0{};
  assert(propagation.layer_by_index(new_comp, 0, linked_layer0));
  assert(linked_layer0.owner == new_comp);

  Identity new_layer0{};
  assert(propagation.invalidate(linked_layer0.identity, new_layer0));
  assert(!propagation.resolve(
      old_layer_handle, ObjectKind::layer, unchanged));
  ObjectSnapshot linked_layer1{};
  assert(propagation.layer_by_index(new_comp, 1, linked_layer1));
  assert(linked_layer1.parent_layer == new_layer0);

  Identity new_project{};
  assert(propagation.invalidate(old_project, new_project));
  assert(propagation.active_project() == new_project);
  ObjectSnapshot linked_root{};
  assert(propagation.first_child(new_project, linked_root));
  assert(linked_root.owner == new_project);

  FixtureStorage exhaustion_storage{};
  Registry exhaustion;
  assert(initialize(exhaustion, exhaustion_storage));
  ObjectSnapshot exhaustion_comp{};
  ObjectSnapshot exhaustion_layer{};
  assert(exhaustion.comp_from_item(
      exhaustion.active_item(), exhaustion_comp));
  assert(exhaustion.layer_by_index(
      exhaustion_comp.identity, 0, exhaustion_layer));
  Identity current = exhaustion_layer.identity;
  std::array<void*, kBorrowedHandleCapacity> issued{};
  for (std::size_t index = 0; index < issued.size(); ++index) {
    issued[index] = exhaustion.borrow(current);
    assert(aligned_token(issued[index]));
    assert(exhaustion.borrow(current) == issued[index]);
    assert(std::find(issued.begin(), issued.begin() + index,
                     issued[index]) == issued.begin() + index);
    Identity next{};
    assert(exhaustion.invalidate(current, next));
    current = next;
  }
  assert(exhaustion.borrow(current) == nullptr);
  for (void* stale : issued)
    assert(!exhaustion.resolve(
        stale, ObjectKind::layer, unchanged, 1));

  std::puts(
      "{\"scene_model\":\"passed\",\"projects\":2,\"folders\":3,"
      "\"footage\":1,\"items\":3,\"compositions\":3,\"layers\":5,"
      "\"aligned_tokens\":true,\"cross_registry_rejected\":true,"
      "\"forged_token_rejected\":true,\"lease_identity_checked\":true,"
      "\"token_exhaustion_rejected\":true,"
      "\"relationships_propagated\":true,\"stale_rejected\":true,"
      "\"wrong_kind_rejected\":true,\"cross_project_rejected\":true,"
      "\"foreign_rejected\":true}");
  return 0;
}
