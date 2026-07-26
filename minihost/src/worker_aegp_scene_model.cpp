#include "worker_aegp_scene_model.hpp"

#include <algorithm>
#include <climits>
#include <cstring>

namespace aexcompat::scene_model {
namespace {

constexpr uintptr_t kBorrowedTag = 0xAu;
constexpr uintptr_t kBorrowedTagMask = 0xFu;
constexpr unsigned kBorrowedSlotShift = 4;
constexpr unsigned kBorrowedGenerationShift = 12;
constexpr uintptr_t kBorrowedSlotMask = 0xffu;
constexpr uint32_t kMaxLeaseGeneration = 0x7fffffffu;

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

void* Registry::encode_handle(std::size_t slot,
                              uint32_t generation) const noexcept {
  if (slot >= borrowed_.size() || generation == 0 ||
      generation > kMaxLeaseGeneration)
    return nullptr;
  const uintptr_t value =
      (static_cast<uintptr_t>(generation) << kBorrowedGenerationShift) |
      (static_cast<uintptr_t>(slot) << kBorrowedSlotShift) |
      kBorrowedTag;
  return reinterpret_cast<void*>(value);
}

bool Registry::decode_handle(void* handle, std::size_t& slot,
                             uint32_t& generation) const noexcept {
  const uintptr_t value = reinterpret_cast<uintptr_t>(handle);
  if (!handle || (value & kBorrowedTagMask) != kBorrowedTag) return false;
  slot = static_cast<std::size_t>(
      (value >> kBorrowedSlotShift) & kBorrowedSlotMask);
  generation = static_cast<uint32_t>(value >> kBorrowedGenerationShift);
  return slot < borrowed_.size() && generation != 0;
}

void* Registry::borrow(Identity identity) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!find_locked(identity)) return nullptr;
  for (std::size_t slot = 0; slot < borrowed_.size(); ++slot) {
    const auto& lease = borrowed_[slot];
    if (lease.live && lease.target == identity)
      return encode_handle(slot, lease.lease_generation);
  }
  for (std::size_t slot = 0; slot < borrowed_.size(); ++slot) {
    auto& lease = borrowed_[slot];
    if (lease.live || lease.exhausted) continue;
    if (lease.lease_generation >= kMaxLeaseGeneration) {
      lease.exhausted = true;
      continue;
    }
    ++lease.lease_generation;
    lease.target = identity;
    lease.live = true;
    return encode_handle(slot, lease.lease_generation);
  }
  return nullptr;
}

bool Registry::resolve_locked(void* handle, ObjectKind expected,
                              bool item_family, ObjectSnapshot& output,
                              uint64_t required_project_id) const noexcept {
  std::size_t slot = 0;
  uint32_t generation = 0;
  if (!decode_handle(handle, slot, generation)) return false;
  const auto& lease = borrowed_[slot];
  if (!lease.live || lease.lease_generation != generation) return false;
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
  uint32_t borrowed_generation = 0;
  if (decode_handle(handle, borrowed_slot, borrowed_generation)) return false;
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
  uint32_t borrowed_generation = 0;
  if (decode_handle(handle, borrowed_slot, borrowed_generation)) return false;
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

bool Registry::invalidate(Identity identity, Identity& replacement) noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  auto* record = find_locked(identity);
  if (!record || identity.generation == UINT32_MAX) return false;
  for (auto& lease : borrowed_)
    if (lease.live && lease.target == identity) lease.live = false;
  ++record->snapshot.identity.generation;
  replacement = record->snapshot.identity;
  if (active_item_ == identity) active_item_ = replacement;
  if (active_project_ == identity) active_project_ = replacement;
  return true;
}

uint64_t Registry::fingerprint() const noexcept {
  std::lock_guard<std::mutex> lock(mutex_);
  uint64_t hash = 0xcbf29ce484222325ull;
  hash = mix(hash, object_count_);
  hash = mix(hash, project_count_);
  for (std::size_t index = 0; index < object_count_; ++index) {
    const auto& record = objects_[index];
    hash = mix(hash, record.live);
    hash = mix(hash, record.snapshot.identity.project_id);
    hash = mix(hash, record.snapshot.identity.object_id);
    hash = mix(hash, record.snapshot.identity.generation);
    hash = mix(hash, static_cast<uint8_t>(record.snapshot.identity.kind));
    hash = mix(hash, record.snapshot.owner.object_id);
    hash = mix(hash, record.snapshot.related_item.object_id);
    hash = mix(hash, record.snapshot.parent_layer.object_id);
  }
  for (const auto& lease : borrowed_) {
    hash = mix(hash, lease.live);
    hash = mix(hash, lease.lease_generation);
    hash = mix(hash, lease.target.object_id);
    hash = mix(hash, static_cast<uint8_t>(lease.target.kind));
  }
  return hash;
}

Registry& registry() noexcept {
  static Registry value;
  return value;
}

}  // namespace aexcompat::scene_model
