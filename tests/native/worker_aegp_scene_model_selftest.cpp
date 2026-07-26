#include "worker_aegp_scene_model.hpp"

#include <array>
#include <cassert>
#include <cstdint>
#include <cstdio>

using aexcompat::scene_model::Identity;
using aexcompat::scene_model::ItemKind;
using aexcompat::scene_model::ObjectKind;
using aexcompat::scene_model::ObjectSnapshot;
using aexcompat::scene_model::Registry;

int main() {
  int item_storage = 0;
  int comp_storage = 0;
  std::array<int, 3> layer_storage{};
  std::array<void*, 3> layers{{
      &layer_storage[0], &layer_storage[1], &layer_storage[2]}};

  Registry registry;
  assert(registry.initialize_fixture(
      &item_storage, &comp_storage, layers.data(), layers.size()));
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
  assert(item_handle);
  ObjectSnapshot item{};
  assert(registry.resolve_item(item_handle, item));
  assert(item.identity == active_item);
  assert(item.item_kind == ItemKind::composition);

  ObjectSnapshot comp{};
  assert(registry.comp_from_item(item.identity, comp));
  void* comp_handle = registry.borrow(comp.identity);
  assert(comp_handle);
  ObjectSnapshot resolved_comp{};
  assert(registry.resolve(
      comp_handle, ObjectKind::composition, resolved_comp, 1));
  assert(registry.layer_count(resolved_comp.identity) == 3);

  ObjectSnapshot layer{};
  assert(registry.layer_by_index(resolved_comp.identity, 2, layer));
  void* layer_handle = registry.borrow(layer.identity);
  assert(layer_handle);
  ObjectSnapshot resolved_layer{};
  assert(registry.resolve(layer_handle, ObjectKind::layer, resolved_layer, 1));
  assert(resolved_layer.identity.object_id == 2003);
  assert(resolved_layer.owner == resolved_comp.identity);

  const uint64_t before_rejections = registry.fingerprint();
  ObjectSnapshot unchanged{};
  unchanged.identity.object_id = 0xfeed;
  const ObjectSnapshot sentinel = unchanged;
  assert(!registry.resolve(
      nullptr, ObjectKind::layer, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(layer_handle, ObjectKind::composition, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(
      layer_handle, ObjectKind::layer, unchanged, 2));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(!registry.resolve(reinterpret_cast<void*>(
      static_cast<uintptr_t>(0x12345678)), ObjectKind::layer, unchanged));
  assert(unchanged.identity.object_id == sentinel.identity.object_id);
  assert(registry.fingerprint() == before_rejections);

  Identity replacement{};
  assert(registry.invalidate(layer.identity, replacement));
  assert(replacement.generation == layer.identity.generation + 1);
  assert(!registry.resolve(
      layer_handle, ObjectKind::layer, unchanged, 1));
  void* replacement_handle = registry.borrow(replacement);
  assert(replacement_handle && replacement_handle != layer_handle);
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
  assert(layer_b_handle);
  assert(!registry.resolve(
      layer_b_handle, ObjectKind::layer, unchanged, 1));
  assert(registry.resolve(
      layer_b_handle, ObjectKind::layer, resolved_layer, 2));

  std::puts(
      "{\"scene_model\":\"passed\",\"projects\":2,\"folders\":3,"
      "\"footage\":1,\"items\":3,\"compositions\":3,\"layers\":5,"
      "\"stale_rejected\":true,\"wrong_kind_rejected\":true,"
      "\"cross_project_rejected\":true,\"foreign_rejected\":true}");
  return 0;
}
