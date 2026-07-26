#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <string_view>

namespace aexcompat::scene_model {

enum class ObjectKind : uint8_t {
  none = 0,
  project = 1,
  item = 2,
  composition = 3,
  folder = 4,
  footage = 5,
  layer = 6,
  effect = 7,
  stream = 8,
  keyframe = 9,
  value = 10,
};

enum class ItemKind : uint8_t {
  none = 0,
  folder = 1,
  composition = 2,
  footage = 3,
};

enum class StreamValueKind : uint8_t {
  none = 0,
  scalar = 1,
  color = 2,
  layer = 3,
  mask = 4,
  arbitrary = 5,
};

struct TemporalEase {
  double speed{};
  double influence{};
};

struct StreamState {
  StreamValueKind value_kind{StreamValueKind::none};
  uint8_t dimensions{};
  uint8_t temporal_dimensions{};
  uint8_t reserved{};
  std::array<std::byte, 32> value{};
};

struct KeyframeState {
  int32_t time_value{};
  uint32_t time_scale{1};
  int32_t in_interpolation{1};
  int32_t out_interpolation{1};
  uint32_t flags{};
  int32_t label{};
  std::array<double, 4> spatial_in{};
  std::array<double, 4> spatial_out{};
  std::array<TemporalEase, 4> temporal_in{};
  std::array<TemporalEase, 4> temporal_out{};
};

struct Identity {
  uint64_t project_id{};
  uint64_t object_id{};
  uint32_t generation{};
  ObjectKind kind{ObjectKind::none};
  uint8_t reserved[3]{};

  friend constexpr bool operator==(const Identity& left,
                                   const Identity& right) noexcept {
    return left.project_id == right.project_id &&
        left.object_id == right.object_id &&
        left.generation == right.generation &&
        left.kind == right.kind;
  }
  friend constexpr bool operator!=(const Identity& left,
                                   const Identity& right) noexcept {
    return !(left == right);
  }
};
static_assert(sizeof(Identity) == 24);
static_assert(offsetof(Identity, project_id) == 0);
static_assert(offsetof(Identity, object_id) == 8);
static_assert(offsetof(Identity, generation) == 16);
static_assert(offsetof(Identity, kind) == 20);
static_assert(sizeof(void*) == sizeof(uint64_t));

struct ObjectSnapshot {
  Identity identity{};
  Identity owner{};
  Identity related_item{};
  Identity parent_layer{};
  ItemKind item_kind{ItemKind::none};
  int32_t local_index{-1};
  void* legacy_handle{};
  std::array<char16_t, 48> name{};
  StreamState stream{};
  KeyframeState keyframe{};
};

inline constexpr std::size_t kProjectCapacity = 4;
inline constexpr std::size_t kObjectCapacity = 256;
inline constexpr std::size_t kBorrowedHandleCapacity = 128;

class Registry {
 public:
  Registry() noexcept;

  bool initialize_fixture(void* primary_item, void* primary_comp,
                          void* const* primary_layers,
                          std::size_t primary_layer_count) noexcept;

  std::size_t project_count() const noexcept;
  std::size_t live_object_count(ObjectKind kind) const noexcept;
  Identity active_project() const noexcept;
  Identity active_item() const noexcept;

  bool snapshot(Identity identity, ObjectSnapshot& output) const noexcept;
  bool identity_for_legacy(void* legacy, ObjectKind expected,
                           Identity& output) const noexcept;
  bool identity_for_legacy_item(void* legacy, Identity& output) const noexcept;
  bool identity_for_object(uint64_t project_id, uint64_t object_id,
                           ObjectKind expected, Identity& output) const noexcept;

  bool can_create_child(Identity owner) const noexcept;
  bool can_create_children(Identity owner, std::size_t object_count,
                           std::size_t borrowed_count = 0) const noexcept;
  bool create_child(ObjectKind kind, Identity owner, int32_t local_index,
                    void* legacy_handle, std::u16string_view name,
                    Identity& output) noexcept;
  bool create_child_pair(
      ObjectKind kind, Identity owner,
      const std::array<int32_t, 2>& local_indices,
      const std::array<void*, 2>& legacy_handles,
      std::u16string_view name,
      std::array<Identity, 2>& outputs) noexcept;
  bool create_child_borrowed(ObjectKind kind, Identity owner,
                             int32_t local_index, void* legacy_handle,
                             std::u16string_view name, int32_t possession_id,
                             Identity& output, void*& handle) noexcept;
  bool update_local_index(Identity identity, int32_t local_index) noexcept;
  bool initialize_stream_state(Identity identity,
                               const StreamState& state) noexcept;
  bool initialize_keyframe_state(Identity identity,
                                  const KeyframeState& state) noexcept;
  bool replace_snapshot(Identity identity, const ObjectSnapshot& candidate,
                        Identity& replacement) noexcept;
  bool erase_tree(Identity identity) noexcept;

  void* borrow(Identity identity, int32_t possession_id = 0) noexcept;
  void* borrow_unique(Identity identity, int32_t possession_id) noexcept;
  bool resolve(void* handle, ObjectKind expected, ObjectSnapshot& output,
               uint64_t required_project_id = 0) const noexcept;
  bool resolve_possessed(void* handle, ObjectKind expected,
                         int32_t possession_id, ObjectSnapshot& output,
                         uint64_t required_project_id = 0) const noexcept;
  bool possession(void* handle, ObjectKind expected,
                  int32_t& possession_id) const noexcept;
  bool release(void* handle, ObjectKind expected,
               int32_t possession_id = 0,
               bool require_possession = false) noexcept;
  bool resolve_item(void* handle, ObjectSnapshot& output,
                    uint64_t required_project_id = 0) const noexcept;
  bool resolve_or_legacy(void* handle, ObjectKind expected,
                         ObjectSnapshot& output,
                         uint64_t required_project_id = 0) const noexcept;
  bool resolve_item_or_legacy(void* handle, ObjectSnapshot& output,
                              uint64_t required_project_id = 0) const noexcept;

  bool first_child(Identity owner, ObjectSnapshot& output) const noexcept;
  bool next_sibling(Identity identity, ObjectSnapshot& output) const noexcept;
  bool comp_from_item(Identity item, ObjectSnapshot& output) const noexcept;
  bool item_from_comp(Identity comp, ObjectSnapshot& output) const noexcept;
  std::size_t layer_count(Identity comp) const noexcept;
  bool layer_by_index(Identity comp, std::size_t index,
                      ObjectSnapshot& output) const noexcept;
  bool layer_from_id(Identity comp, uint64_t object_id,
                     ObjectSnapshot& output) const noexcept;
  bool child_by_index(Identity owner, ObjectKind kind, std::size_t index,
                      ObjectSnapshot& output) const noexcept;

  bool invalidate(Identity identity, Identity& replacement) noexcept;
  uint64_t fingerprint() const noexcept;

 private:
  struct ObjectRecord {
    ObjectSnapshot snapshot{};
    bool live{};
  };
  struct alignas(std::max_align_t) BorrowedToken {
    uint64_t lease_identity{};
  };
  struct BorrowedLease {
    Identity target{};
    uint64_t lease_identity{};
    int32_t possession_id{};
    bool issued{};
    bool live{};
  };
  static_assert(alignof(BorrowedToken) >= alignof(std::max_align_t));

  bool append(ObjectKind kind, uint64_t project_id, uint64_t object_id,
              Identity owner, Identity related_item, Identity parent_layer,
              ItemKind item_kind, int32_t local_index, void* legacy_handle,
              std::u16string_view name, Identity& output) noexcept;
  const ObjectRecord* find_locked(Identity identity) const noexcept;
  ObjectRecord* find_locked(Identity identity) noexcept;
  bool is_item_kind(ObjectKind kind) const noexcept;
  bool token_slot_for_address_locked(
      const void* handle, std::size_t& slot) const noexcept;
  bool resolve_locked(void* handle, ObjectKind expected,
                      bool item_family, ObjectSnapshot& output,
                      uint64_t required_project_id,
                      bool require_possession = false,
                      int32_t possession_id = 0) const noexcept;

  mutable std::mutex mutex_;
  std::array<ObjectRecord, kObjectCapacity> objects_{};
  std::array<BorrowedToken, kBorrowedHandleCapacity> borrowed_tokens_{};
  std::array<BorrowedLease, kBorrowedHandleCapacity> borrowed_leases_{};
  std::size_t object_count_{};
  std::size_t project_count_{};
  std::size_t issued_token_count_{};
  uint64_t next_dynamic_object_id_{100000};
  uint64_t next_lease_identity_{1};
  bool lease_identity_exhausted_{};
  Identity active_project_{};
  Identity active_item_{};
  bool initialized_{};
};

Registry& registry() noexcept;

}  // namespace aexcompat::scene_model
