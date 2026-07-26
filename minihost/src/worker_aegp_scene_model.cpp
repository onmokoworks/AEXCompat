#include "worker_aegp_scene_model.hpp"

#include <algorithm>
#include <climits>
#include <cstring>

namespace aexcompat::scene_model {
namespace {

uint64_t mix(uint64_t hash, uint64_t value) noexcept {
  hash ^= value + 0x9e3779b97f4a7c15ull + (hash << 6) + (hash >> 2);
  return hash;
}

}  // namespace

Registry::Registry() noexcept = default;

bool Registry::is_item_kind(ObjectKind kind) const noexcept {
  return kind == ObjectKind::item || kind == ObjectKind::folder ||
      kind == ObjectKind::footage;
}

bool Registry::append(ObjectKind kind, uint64_t project_id, uint64_t object_id,
                      Identity owner, Identity related_item,
                      Identity parent_layer, ItemKind item_kind,
                      int32_t local_index, void* legacy_handle,
                      std::u16string_view name, Identity& output) noexcept {
  if (kind == ObjectKind::none || project_id == 0 || object_id == 0 ||
      object_count_ >= objects_.size() || name.size() >=
          objects_[object_count_].snapshot.name.size())
    return false;
  if (kind == ObjectKind::project &&
      project_count_ >= kProjectCapacity)
    return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& identity = objects_[index].snapshot.identity;
    if (objects_[index].live && identity.project_id == project_id &&
        identity.object_id == object_id && identity.kind == kind)
      return false;
  }
  auto& record = objects_[object_count_++];
  record = {};
  record.live = true;
  record.snapshot.identity = {project_id, object_id, 1, kind, {}};
  record.snapshot.owner = owner;
  record.snapshot.related_item = related_item;
  record.snapshot.parent_layer = parent_layer;
  record.snapshot.item_kind = item_kind;
  record.snapshot.local_index = local_index;
  record.snapshot.legacy_handle = legacy_handle;
  std::copy(name.begin(), name.end(), record.snapshot.name.begin());
  if (kind == ObjectKind::project) ++project_count_;
  output = record.snapshot.identity;
  return true;
}

bool Registry::initialize_fixture(void* primary_item, void* primary_comp,
                                  void* const* primary_layers,
                                  std::size_t primary_layer_count) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  if (initialized_) return true;
  if (!primary_item || !primary_comp || !primary_layers ||
      primary_layer_count != 3)
    return false;

  Identity project_a{}, project_b{};
  Identity root_a{}, nested_a{}, footage_a{}, parent_item{}, parent_comp{};
  Identity child_item{}, child_comp{}, root_b{}, item_b{}, comp_b{};
  if (!append(ObjectKind::project, 1, 1, {}, {}, {}, ItemKind::none, -1,
              nullptr, u"Project A", project_a) ||
      !append(ObjectKind::folder, 1, 100, project_a, {}, {},
              ItemKind::folder, 0, nullptr, u"Root A", root_a) ||
      !append(ObjectKind::folder, 1, 101, root_a, {}, {},
              ItemKind::folder, 0, nullptr, u"Sources", nested_a) ||
      !append(ObjectKind::footage, 1, 200, nested_a, {}, {},
              ItemKind::footage, 0, nullptr, u"Footage A", footage_a) ||
      !append(ObjectKind::item, 1, 1001, root_a, {}, {},
              ItemKind::composition, 0, primary_item,
              u"AEXCompat Composition", parent_item) ||
      !append(ObjectKind::composition, 1, 5001, parent_item, parent_item, {},
              ItemKind::none, 0, primary_comp, u"Parent Comp", parent_comp) ||
      !append(ObjectKind::item, 1, 1002, root_a, {}, {},
              ItemKind::composition, 1, nullptr, u"Child Composition",
              child_item) ||
      !append(ObjectKind::composition, 1, 5002, child_item, child_item, {},
              ItemKind::none, 1, nullptr, u"Child Comp", child_comp))
    return false;

  Identity layer0{}, layer1{}, layer2{}, child_layer{};
  if (!append(ObjectKind::layer, 1, 2001, parent_comp, child_item, {},
              ItemKind::none, 0, primary_layers[0], u"Layer 1", layer0) ||
      !append(ObjectKind::layer, 1, 2002, parent_comp, footage_a, layer0,
              ItemKind::none, 1, primary_layers[1], u"Layer 2", layer1) ||
      !append(ObjectKind::layer, 1, 2003, parent_comp, footage_a, layer1,
              ItemKind::none, 2, primary_layers[2], u"Layer 3", layer2) ||
      !append(ObjectKind::layer, 1, 2010, child_comp, footage_a, {},
              ItemKind::none, 0, nullptr, u"Child Layer", child_layer))
    return false;

  if (!append(ObjectKind::project, 2, 2, {}, {}, {}, ItemKind::none, -1,
              nullptr, u"Project B", project_b) ||
      !append(ObjectKind::folder, 2, 1100, project_b, {}, {},
              ItemKind::folder, 0, nullptr, u"Root B", root_b) ||
      !append(ObjectKind::item, 2, 1101, root_b, {}, {},
              ItemKind::composition, 0, nullptr, u"Other Composition",
              item_b) ||
      !append(ObjectKind::composition, 2, 5101, item_b, item_b, {},
              ItemKind::none, 0, nullptr, u"Other Comp", comp_b))
    return false;
  Identity other_layer{};
  if (!append(ObjectKind::layer, 2, 2101, comp_b, {}, {},
              ItemKind::none, 0, nullptr, u"Other Layer", other_layer))
    return false;

  active_project_ = project_a;
  active_item_ = parent_item;
  initialized_ = true;
  return true;
}

const Registry::ObjectRecord* Registry::find_locked(
    Identity identity) const noexcept {
  for (std::size_t index = 0; index < object_count_; ++index)
    if (objects_[index].live &&
        objects_[index].snapshot.identity == identity)
      return &objects_[index];
  return nullptr;
}

Registry::ObjectRecord* Registry::find_locked(Identity identity) noexcept {
  return const_cast<ObjectRecord*>(
      static_cast<const Registry*>(this)->find_locked(identity));
}

std::size_t Registry::project_count() const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return project_count_;
}

std::size_t Registry::live_object_count(ObjectKind kind) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return static_cast<std::size_t>(std::count_if(
      objects_.begin(), objects_.begin() + object_count_,
      [kind](const auto& record) {
        return record.live && record.snapshot.identity.kind == kind;
      }));
}

Identity Registry::active_project() const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return active_project_;
}

Identity Registry::active_item() const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return active_item_;
}

bool Registry::snapshot(Identity identity, ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* record = find_locked(identity);
  if (!record) return false;
  output = record->snapshot;
  return true;
}

bool Registry::identity_for_legacy(void* legacy, ObjectKind expected,
                                   Identity& output) const noexcept {
  if (!legacy || expected == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.legacy_handle == legacy &&
        record.snapshot.identity.kind == expected) {
      output = record.snapshot.identity;
      return true;
    }
  }
  return false;
}

bool Registry::identity_for_legacy_item(void* legacy,
                                        Identity& output) const noexcept {
  if (!legacy) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.legacy_handle == legacy &&
        is_item_kind(record.snapshot.identity.kind)) {
      output = record.snapshot.identity;
      return true;
    }
  }
  return false;
}

bool Registry::identity_for_object(uint64_t project_id, uint64_t object_id,
                                   ObjectKind expected,
                                   Identity& output) const noexcept {
  if (project_id == 0 || object_id == 0 || expected == ObjectKind::none)
    return false;
  std::lock_guard<std::mutex> lock(mutex_);
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.identity.project_id == project_id &&
        record.snapshot.identity.object_id == object_id &&
        record.snapshot.identity.kind == expected) {
      output = record.snapshot.identity;
      return true;
    }
  }
  return false;
}

bool Registry::can_create_child(Identity owner) const noexcept {
  return can_create_children(owner, 1);
}

bool Registry::can_create_children(Identity owner,
                                   std::size_t requested_objects,
                                   std::size_t requested_borrowed) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return find_locked(owner) && requested_objects != 0 &&
      requested_objects <= objects_.size() - object_count_ &&
      requested_borrowed <= borrowed_tokens_.size() - issued_token_count_ &&
      next_dynamic_object_id_ != 0 &&
      next_dynamic_object_id_ <=
          UINT64_MAX - requested_objects &&
      (requested_borrowed == 0 ||
       (!lease_identity_exhausted_ && next_lease_identity_ != 0 &&
        next_lease_identity_ <= UINT64_MAX - requested_borrowed));
}

bool Registry::create_child(ObjectKind kind, Identity owner,
                            int32_t local_index, void* legacy_handle,
                            std::u16string_view name,
                            Identity& output) noexcept {
  if (kind == ObjectKind::none || kind == ObjectKind::project)
    return false;
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(owner) || object_count_ >= objects_.size() ||
      next_dynamic_object_id_ == 0 ||
      next_dynamic_object_id_ == UINT64_MAX)
    return false;
  const uint64_t object_id = next_dynamic_object_id_;
  if (!append(kind, owner.project_id, object_id, owner, {}, {},
              ItemKind::none, local_index, legacy_handle, name, output))
    return false;
  ++next_dynamic_object_id_;
  return true;
}

bool Registry::create_child_borrowed(
    ObjectKind kind, Identity owner, int32_t local_index,
    void* legacy_handle, std::u16string_view name, int32_t possession_id,
    Identity& output, void*& handle) noexcept {
  if (kind == ObjectKind::none || kind == ObjectKind::project ||
      possession_id <= 0)
    return false;
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(owner) || object_count_ >= objects_.size() ||
      next_dynamic_object_id_ == 0 ||
      next_dynamic_object_id_ == UINT64_MAX ||
      issued_token_count_ >= borrowed_tokens_.size() ||
      lease_identity_exhausted_ || next_lease_identity_ == 0)
    return false;
  const uint64_t object_id = next_dynamic_object_id_;
  if (!append(kind, owner.project_id, object_id, owner, {}, {},
              ItemKind::none, local_index, legacy_handle, name, output))
    return false;
  ++next_dynamic_object_id_;
  const std::size_t slot = issued_token_count_++;
  auto& token = borrowed_tokens_[slot];
  auto& lease = borrowed_leases_[slot];
  token.lease_identity = next_lease_identity_;
  lease.target = output;
  lease.lease_identity = next_lease_identity_;
  lease.possession_id = possession_id;
  lease.issued = true;
  lease.live = true;
  if (next_lease_identity_ == UINT64_MAX)
    lease_identity_exhausted_ = true;
  else
    ++next_lease_identity_;
  handle = &token;
  return true;
}

bool Registry::update_local_index(Identity identity,
                                  int32_t local_index) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record) return false;
  record->snapshot.local_index = local_index;
  return true;
}

bool Registry::initialize_stream_state(Identity identity,
                                       const StreamState& state) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record || identity.kind != ObjectKind::stream ||
      record->snapshot.stream.value_kind != StreamValueKind::none ||
      state.value_kind == StreamValueKind::none)
    return false;
  record->snapshot.stream = state;
  return true;
}

bool Registry::initialize_keyframe_state(
    Identity identity, const KeyframeState& state) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record || identity.kind != ObjectKind::keyframe ||
      record->snapshot.keyframe.time_scale != 1 ||
      record->snapshot.keyframe.time_value != 0 ||
      state.time_scale == 0)
    return false;
  record->snapshot.keyframe = state;
  return true;
}

bool Registry::replace_snapshot(Identity identity,
                                const ObjectSnapshot& candidate,
                                Identity& replacement) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record || identity.generation == UINT32_MAX ||
      candidate.identity != identity ||
      candidate.owner != record->snapshot.owner ||
      candidate.related_item != record->snapshot.related_item ||
      candidate.parent_layer != record->snapshot.parent_layer ||
      candidate.legacy_handle != record->snapshot.legacy_handle)
    return false;
  for (auto& lease : borrowed_leases_)
    if (lease.live && lease.target == identity) lease.live = false;
  record->snapshot = candidate;
  ++record->snapshot.identity.generation;
  replacement = record->snapshot.identity;
  for (std::size_t index = 0; index < object_count_; ++index) {
    auto& snapshot = objects_[index].snapshot;
    if (!objects_[index].live) continue;
    if (snapshot.owner == identity) snapshot.owner = replacement;
    if (snapshot.related_item == identity)
      snapshot.related_item = replacement;
    if (snapshot.parent_layer == identity)
      snapshot.parent_layer = replacement;
  }
  return true;
}

bool Registry::erase_tree(Identity identity) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(identity)) return false;
  std::array<bool, kObjectCapacity> erase{};
  for (std::size_t index = 0; index < object_count_; ++index)
    erase[index] = objects_[index].live &&
        objects_[index].snapshot.identity == identity;
  bool changed = true;
  while (changed) {
    changed = false;
    for (std::size_t index = 0; index < object_count_; ++index) {
      if (!objects_[index].live || erase[index]) continue;
      for (std::size_t owner_index = 0;
           owner_index < object_count_; ++owner_index) {
        if (erase[owner_index] &&
            objects_[index].snapshot.owner ==
                objects_[owner_index].snapshot.identity) {
          erase[index] = true;
          changed = true;
          break;
        }
      }
    }
  }
  for (std::size_t index = 0; index < object_count_; ++index)
    if (erase[index] &&
        objects_[index].snapshot.identity.generation == UINT32_MAX)
      return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    if (!erase[index]) continue;
    const Identity erased = objects_[index].snapshot.identity;
    for (auto& lease : borrowed_leases_)
      if (lease.live && lease.target == erased) lease.live = false;
    ++objects_[index].snapshot.identity.generation;
    objects_[index].live = false;
  }
  return true;
}

bool Registry::token_slot_for_address_locked(
    const void* handle, std::size_t& slot) const noexcept {
  if (!handle) return false;
  for (std::size_t index = 0; index < borrowed_tokens_.size(); ++index) {
    if (handle == static_cast<const void*>(&borrowed_tokens_[index])) {
      slot = index;
      return true;
    }
  }
  return false;
}

void* Registry::borrow(Identity identity, int32_t possession_id) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(identity)) return nullptr;
  for (std::size_t index = 0; index < issued_token_count_; ++index) {
    const auto& lease = borrowed_leases_[index];
    if (lease.live && lease.target == identity &&
        lease.possession_id == possession_id)
      return &borrowed_tokens_[index];
  }
  if (issued_token_count_ >= borrowed_tokens_.size() ||
      lease_identity_exhausted_ || next_lease_identity_ == 0)
    return nullptr;
  const std::size_t slot = issued_token_count_++;
  auto& token = borrowed_tokens_[slot];
  auto& lease = borrowed_leases_[slot];
  token.lease_identity = next_lease_identity_;
  lease.target = identity;
  lease.lease_identity = next_lease_identity_;
  lease.possession_id = possession_id;
  lease.issued = true;
  lease.live = true;
  if (next_lease_identity_ == UINT64_MAX)
    lease_identity_exhausted_ = true;
  else
    ++next_lease_identity_;
  return &token;
}

void* Registry::borrow_unique(Identity identity,
                              int32_t possession_id) noexcept {
  if (possession_id <= 0) return nullptr;
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(identity) ||
      issued_token_count_ >= borrowed_tokens_.size() ||
      lease_identity_exhausted_ || next_lease_identity_ == 0)
    return nullptr;
  const std::size_t slot = issued_token_count_++;
  auto& token = borrowed_tokens_[slot];
  auto& lease = borrowed_leases_[slot];
  token.lease_identity = next_lease_identity_;
  lease.target = identity;
  lease.lease_identity = next_lease_identity_;
  lease.possession_id = possession_id;
  lease.issued = true;
  lease.live = true;
  if (next_lease_identity_ == UINT64_MAX)
    lease_identity_exhausted_ = true;
  else
    ++next_lease_identity_;
  return &token;
}

bool Registry::resolve_locked(void* handle, ObjectKind expected,
                              bool item_family, ObjectSnapshot& output,
                              uint64_t required_project_id,
                              bool require_possession,
                              int32_t possession_id) const noexcept {
  std::size_t slot = 0;
  if (!token_slot_for_address_locked(handle, slot)) return false;
  const auto& token = borrowed_tokens_[slot];
  const auto& lease = borrowed_leases_[slot];
  if (!lease.issued || !lease.live || lease.lease_identity == 0 ||
      token.lease_identity != lease.lease_identity ||
      (require_possession && lease.possession_id != possession_id))
    return false;
  const auto* record = find_locked(lease.target);
  if (!record ||
      (item_family ? !is_item_kind(record->snapshot.identity.kind)
                   : record->snapshot.identity.kind != expected) ||
      (required_project_id != 0 &&
       record->snapshot.identity.project_id != required_project_id))
    return false;
  output = record->snapshot;
  return true;
}

bool Registry::resolve(void* handle, ObjectKind expected,
                       ObjectSnapshot& output,
                       uint64_t required_project_id) const noexcept {
  if (expected == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  return resolve_locked(handle, expected, false, output, required_project_id);
}

bool Registry::resolve_possessed(void* handle, ObjectKind expected,
                                 int32_t possession_id,
                                 ObjectSnapshot& output,
                                 uint64_t required_project_id) const noexcept {
  if (expected == ObjectKind::none || possession_id <= 0) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  return resolve_locked(handle, expected, false, output,
                        required_project_id, true, possession_id);
}

bool Registry::possession(void* handle, ObjectKind expected,
                          int32_t& possession_id) const noexcept {
  if (!handle || expected == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  std::size_t slot = 0;
  if (!token_slot_for_address_locked(handle, slot)) return false;
  const auto& token = borrowed_tokens_[slot];
  const auto& lease = borrowed_leases_[slot];
  const auto* record = find_locked(lease.target);
  if (!lease.issued || !lease.live || lease.lease_identity == 0 ||
      token.lease_identity != lease.lease_identity || !record ||
      record->snapshot.identity.kind != expected ||
      lease.possession_id <= 0)
    return false;
  possession_id = lease.possession_id;
  return true;
}

bool Registry::release(void* handle, ObjectKind expected,
                       int32_t possession_id,
                       bool require_possession) noexcept {
  if (!handle || expected == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  std::size_t slot = 0;
  if (!token_slot_for_address_locked(handle, slot)) return false;
  const auto& token = borrowed_tokens_[slot];
  auto& lease = borrowed_leases_[slot];
  const auto* record = find_locked(lease.target);
  if (!lease.issued || !lease.live || lease.lease_identity == 0 ||
      token.lease_identity != lease.lease_identity || !record ||
      record->snapshot.identity.kind != expected ||
      (require_possession && lease.possession_id != possession_id))
    return false;
  lease.live = false;
  return true;
}

bool Registry::resolve_item(void* handle, ObjectSnapshot& output,
                            uint64_t required_project_id) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  return resolve_locked(handle, ObjectKind::item, true, output,
                        required_project_id);
}

bool Registry::resolve_or_legacy(void* handle, ObjectKind expected,
                                 ObjectSnapshot& output,
                                 uint64_t required_project_id) const noexcept {
  if (!handle || expected == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  if (resolve_locked(handle, expected, false, output, required_project_id))
    return true;
  std::size_t borrowed_slot = 0;
  if (token_slot_for_address_locked(handle, borrowed_slot)) return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.legacy_handle == handle &&
        record.snapshot.identity.kind == expected &&
        (required_project_id == 0 ||
         record.snapshot.identity.project_id == required_project_id)) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::resolve_item_or_legacy(
    void* handle, ObjectSnapshot& output,
    uint64_t required_project_id) const noexcept {
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  if (resolve_locked(handle, ObjectKind::item, true, output,
                     required_project_id))
    return true;
  std::size_t borrowed_slot = 0;
  if (token_slot_for_address_locked(handle, borrowed_slot)) return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.legacy_handle == handle &&
        is_item_kind(record.snapshot.identity.kind) &&
        (required_project_id == 0 ||
         record.snapshot.identity.project_id == required_project_id)) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::first_child(Identity owner,
                           ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(owner)) return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.owner == owner) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::next_sibling(Identity identity,
                            ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* current = find_locked(identity);
  if (!current) return false;
  bool seen = false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (!record.live) continue;
    if (record.snapshot.identity == identity) {
      seen = true;
      continue;
    }
    if (seen && record.snapshot.owner == current->snapshot.owner) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::comp_from_item(Identity item,
                              ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* source = find_locked(item);
  if (!source || source->snapshot.item_kind != ItemKind::composition)
    return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live &&
        record.snapshot.identity.kind == ObjectKind::composition &&
        record.snapshot.related_item == item) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::item_from_comp(Identity comp,
                              ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* source = find_locked(comp);
  if (!source || source->snapshot.identity.kind != ObjectKind::composition)
    return false;
  const auto* item = find_locked(source->snapshot.related_item);
  if (!item || item->snapshot.item_kind != ItemKind::composition)
    return false;
  output = item->snapshot;
  return true;
}

std::size_t Registry::layer_count(Identity comp) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* source = find_locked(comp);
  if (!source || source->snapshot.identity.kind != ObjectKind::composition)
    return 0;
  return static_cast<std::size_t>(std::count_if(
      objects_.begin(), objects_.begin() + object_count_,
      [comp](const auto& record) {
        return record.live &&
            record.snapshot.identity.kind == ObjectKind::layer &&
            record.snapshot.owner == comp;
      }));
}

bool Registry::layer_by_index(Identity comp, std::size_t requested,
                              ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* source = find_locked(comp);
  if (!source || source->snapshot.identity.kind != ObjectKind::composition)
    return false;
  std::size_t observed = 0;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live &&
        record.snapshot.identity.kind == ObjectKind::layer &&
        record.snapshot.owner == comp) {
      if (observed++ == requested) {
        output = record.snapshot;
        return true;
      }
    }
  }
  return false;
}

bool Registry::layer_from_id(Identity comp, uint64_t object_id,
                             ObjectSnapshot& output) const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  const auto* source = find_locked(comp);
  if (!source || source->snapshot.identity.kind != ObjectKind::composition)
    return false;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live &&
        record.snapshot.identity.kind == ObjectKind::layer &&
        record.snapshot.owner == comp &&
        record.snapshot.identity.object_id == object_id) {
      output = record.snapshot;
      return true;
    }
  }
  return false;
}

bool Registry::child_by_index(Identity owner, ObjectKind kind,
                              std::size_t requested,
                              ObjectSnapshot& output) const noexcept {
  if (kind == ObjectKind::none) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(owner)) return false;
  std::size_t observed = 0;
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    if (record.live && record.snapshot.owner == owner &&
        record.snapshot.identity.kind == kind) {
      if (observed++ == requested) {
        output = record.snapshot;
        return true;
      }
    }
  }
  return false;
}

bool Registry::invalidate(Identity identity, Identity& replacement) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record || identity.generation == UINT32_MAX) return false;
  for (auto& lease : borrowed_leases_)
    if (lease.live && lease.target == identity) lease.live = false;
  ++record->snapshot.identity.generation;
  replacement = record->snapshot.identity;
  for (std::size_t index = 0; index < object_count_; ++index) {
    auto& snapshot = objects_[index].snapshot;
    if (!objects_[index].live) continue;
    if (snapshot.owner == identity) snapshot.owner = replacement;
    if (snapshot.related_item == identity)
      snapshot.related_item = replacement;
    if (snapshot.parent_layer == identity)
      snapshot.parent_layer = replacement;
  }
  if (active_item_ == identity) active_item_ = replacement;
  if (active_project_ == identity) active_project_ = replacement;
  return true;
}

uint64_t Registry::fingerprint() const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  uint64_t hash = 0xcbf29ce484222325ull;
  hash = mix(hash, object_count_);
  hash = mix(hash, project_count_);
  hash = mix(hash, issued_token_count_);
  hash = mix(hash, next_dynamic_object_id_);
  hash = mix(hash, next_lease_identity_);
  hash = mix(hash, lease_identity_exhausted_);
  const auto mix_identity = [&hash](const Identity& identity) {
    hash = mix(hash, identity.project_id);
    hash = mix(hash, identity.object_id);
    hash = mix(hash, identity.generation);
    hash = mix(hash, static_cast<uint8_t>(identity.kind));
  };
  mix_identity(active_project_);
  mix_identity(active_item_);
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    hash = mix(hash, record.live);
    mix_identity(record.snapshot.identity);
    mix_identity(record.snapshot.owner);
    mix_identity(record.snapshot.related_item);
    mix_identity(record.snapshot.parent_layer);
    hash = mix(hash, static_cast<uint8_t>(
        record.snapshot.stream.value_kind));
    hash = mix(hash, record.snapshot.stream.dimensions);
    hash = mix(hash, record.snapshot.stream.temporal_dimensions);
    for (const auto byte : record.snapshot.stream.value)
      hash = mix(hash, static_cast<uint8_t>(byte));
    hash = mix(hash, static_cast<uint32_t>(
        record.snapshot.keyframe.time_value));
    hash = mix(hash, record.snapshot.keyframe.time_scale);
    hash = mix(hash, static_cast<uint32_t>(
        record.snapshot.keyframe.in_interpolation));
    hash = mix(hash, static_cast<uint32_t>(
        record.snapshot.keyframe.out_interpolation));
    hash = mix(hash, record.snapshot.keyframe.flags);
    hash = mix(hash, static_cast<uint32_t>(
        record.snapshot.keyframe.label));
    for (double value : record.snapshot.keyframe.spatial_in) {
      uint64_t bits = 0;
      std::memcpy(&bits, &value, sizeof(bits));
      hash = mix(hash, bits);
    }
    for (double value : record.snapshot.keyframe.spatial_out) {
      uint64_t bits = 0;
      std::memcpy(&bits, &value, sizeof(bits));
      hash = mix(hash, bits);
    }
    for (const auto& ease : record.snapshot.keyframe.temporal_in) {
      uint64_t speed = 0;
      uint64_t influence = 0;
      std::memcpy(&speed, &ease.speed, sizeof(speed));
      std::memcpy(&influence, &ease.influence, sizeof(influence));
      hash = mix(hash, speed);
      hash = mix(hash, influence);
    }
    for (const auto& ease : record.snapshot.keyframe.temporal_out) {
      uint64_t speed = 0;
      uint64_t influence = 0;
      std::memcpy(&speed, &ease.speed, sizeof(speed));
      std::memcpy(&influence, &ease.influence, sizeof(influence));
      hash = mix(hash, speed);
      hash = mix(hash, influence);
    }
  }
  for (std::size_t index = 0; index < borrowed_tokens_.size(); ++index) {
    const auto& token = borrowed_tokens_[index];
    const auto& lease = borrowed_leases_[index];
    hash = mix(hash, token.lease_identity);
    hash = mix(hash, lease.issued);
    hash = mix(hash, lease.live);
    hash = mix(hash, lease.lease_identity);
    hash = mix(hash, static_cast<uint32_t>(lease.possession_id));
    mix_identity(lease.target);
  }
  return hash;
}

Registry& registry() noexcept {
  static Registry value;
  return value;
}

}  // namespace aexcompat::scene_model
