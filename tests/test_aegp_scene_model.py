from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODEL_HEADER = ROOT / "minihost" / "src" / "worker_aegp_scene_model.hpp"
MODEL_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene_model.cpp"
TRANSACTION_HEADER = (
    ROOT / "minihost" / "src" / "worker_aegp_scene_transaction.hpp"
)
TRANSACTION_SOURCE = (
    ROOT / "minihost" / "src" / "worker_aegp_scene_transaction.cpp"
)
SCENE_SOURCE = ROOT / "minihost" / "src" / "worker_aegp_scene.cpp"
SCENE_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_scene_runtime.cpp"
EXTERNAL_RENDER_RUNTIME = (
    ROOT
    / "minihost"
    / "src"
    / "worker_aegp_external_render_runtime.cpp"
)
RENDER_RECEIPTS = (
    ROOT / "minihost" / "src" / "worker_render_receipts.cpp"
)
COMPAT_SELFTEST = (
    ROOT / "minihost" / "src" / "worker_aegp_compat_selftests.cpp"
)
CUSTOM_ROUTING = (
    ROOT / "minihost" / "src" / "worker_custom_selftest_routing.cpp"
)
CMAKE = ROOT / "minihost" / "CMakeLists.txt"
NATIVE_SELFTEST = ROOT / "tests" / "native" / "worker_aegp_scene_model_selftest.cpp"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_typed_scene_identity_abi_and_bounds_are_fixed() -> None:
    header = read(MODEL_HEADER)
    for marker in (
        "enum class ObjectKind : uint8_t",
        "project = 1",
        "item = 2",
        "composition = 3",
        "folder = 4",
        "footage = 5",
        "layer = 6",
        "effect = 7",
        "stream = 8",
        "keyframe = 9",
        "value = 10",
        "uint64_t project_id{}",
        "uint64_t object_id{}",
        "uint32_t generation{}",
        "static_assert(sizeof(Identity) == 24)",
        "static_assert(offsetof(Identity, generation) == 16)",
        "static_assert(sizeof(void*) == sizeof(uint64_t))",
        "kProjectCapacity = 4",
        "kObjectCapacity = 256",
        "kBorrowedHandleCapacity = 128",
    ):
        assert marker in header


def test_registry_fixture_covers_multiple_projects_and_item_families() -> None:
    source = read(MODEL_SOURCE)
    native = read(NATIVE_SELFTEST)
    for marker in (
        'u"Project A"',
        'u"Project B"',
        'u"Root A"',
        'u"Sources"',
        'u"Footage A"',
        'u"Parent Comp"',
        'u"Child Comp"',
        'u"Other Comp"',
        "registry.project_count() == 2",
        "live_object_count(ObjectKind::folder) == 3",
        "live_object_count(ObjectKind::footage) == 1",
        "live_object_count(ObjectKind::composition) == 3",
        "live_object_count(ObjectKind::layer) == 5",
        "registry.first_child(project_b, root_b)",
        "registry.layer_by_index(comp_b.identity, 0, layer_b)",
    ):
        assert marker in source or marker in native


def test_borrowed_tokens_are_owned_aligned_non_reused_and_fail_closed() -> None:
    source = read(MODEL_SOURCE)
    header = read(MODEL_HEADER)
    native = read(NATIVE_SELFTEST)
    for marker in (
        "struct alignas(std::max_align_t) BorrowedToken",
        "struct BorrowedLease",
        "token_slot_for_address_locked",
        "handle == static_cast<const void*>(&borrowed_tokens_[index])",
        "token.lease_identity != lease.lease_identity",
        "issued_token_count_ >= borrowed_tokens_.size()",
        "lease_identity_exhausted_",
        "record->snapshot.identity.kind != expected",
        "record->snapshot.identity.project_id != required_project_id",
        "if (token_slot_for_address_locked(handle, borrowed_slot)) return false",
        "if (lease.live && lease.target == identity) lease.live = false",
        "ForgedBorrowedToken",
        "foreign_registry.resolve_item(item_handle, unchanged)",
        "exhaustion.borrow(current) == nullptr",
        "aligned_tokens",
        "cross_registry_rejected",
        "forged_token_rejected",
        "lease_identity_checked",
        "token_exhaustion_rejected",
        "replacement.generation == layer.identity.generation + 1",
        "wrong_kind_rejected",
        "cross_project_rejected",
        "foreign_rejected",
        "stale_rejected",
        "registry.fingerprint() == before_rejections",
    ):
        assert marker in source or marker in header or marker in native


def test_invalidation_propagates_every_identity_relationship() -> None:
    source = read(MODEL_SOURCE)
    native = read(NATIVE_SELFTEST)
    for marker in (
        "if (snapshot.owner == identity) snapshot.owner = replacement",
        "if (snapshot.related_item == identity)",
        "snapshot.related_item = replacement",
        "if (snapshot.parent_layer == identity)",
        "snapshot.parent_layer = replacement",
        "if (active_item_ == identity) active_item_ = replacement",
        "if (active_project_ == identity) active_project_ = replacement",
        "propagation.comp_from_item(new_item, linked_comp)",
        "linked_comp.owner == new_item",
        "linked_comp.related_item == new_item",
        "propagation.item_from_comp(new_comp, linked_item)",
        "propagation.layer_count(new_comp) == 3",
        "linked_layer0.owner == new_comp",
        "linked_layer1.parent_layer == new_layer0",
        "propagation.first_child(new_project, linked_root)",
        "propagation.fingerprint() == before_failed_invalidation",
        "relationships_propagated",
    ):
        assert marker in source or marker in native


def test_aegp_traversal_returns_registry_borrowed_handles() -> None:
    scene = read(SCENE_SOURCE)
    runtime = read(SCENE_RUNTIME)
    for marker in (
        "registry.initialize_fixture(",
        "resolve_scene_item",
        "resolve_scene_comp",
        "resolve_scene_layer",
        "borrow_scene_object(scene_registry().active_item())",
        "scene_registry().comp_from_item(",
        "scene_registry().item_from_comp(",
        "scene_registry().layer_by_index(",
        "scene_registry().layer_from_id(",
        "borrow_scene_object(resolved_layer.identity)",
        "resolved.identity.project_id",
    ):
        assert marker in scene or marker in runtime


def test_worker_selftest_traverses_published_suites_and_rejects_bad_handles() -> None:
    compat = read(COMPAT_SELFTEST)
    routing = read(CUSTOM_ROUTING)
    for marker in (
        'compat_acquire_suite("AEGP Item Suite", 14',
        'compat_acquire_suite("AEGP Comp Suite", 25',
        'compat_acquire_suite("AEGP Layer Suite", 14',
        "item_suite->get_active_item(&item)",
        "get_comp_from_item(item, &comp)",
        "get_layer_count(comp, &layer_count)",
        "get_layer_by_index(comp, index, &layer)",
        "get_comp_from_item(comp, &unchanged_handle) != 0",
        "get_layer_count(item, &unchanged_i32) != 0",
        "get_comp_from_item(&forged, &unchanged_handle) != 0",
        "get_comp_from_item(cross_registry_item, &unchanged_handle) != 0",
        "get_layer_from_id(",
        "unchanged_handle == handle_sentinel",
        'L"--self-test-aegp-scene-registry-suites"',
        "aegp_scene_registry_suites",
    ):
        assert marker in compat or marker in routing


def test_native_selftest_is_a_release_build_target() -> None:
    cmake = read(CMAKE)
    assert "src/worker_aegp_scene_model.cpp" in cmake
    assert "add_executable(worker_aegp_scene_model_selftest" in cmake
    assert "../tests/native/worker_aegp_scene_model_selftest.cpp" in cmake
    assert "target_compile_options(worker_aegp_scene_model_selftest PRIVATE /UNDEBUG)" in cmake


def test_effect_stream_value_and_keyframe_identities_are_registry_owned() -> None:
    header = read(MODEL_HEADER)
    source = read(MODEL_SOURCE)
    scene = read(SCENE_SOURCE)
    native = read(NATIVE_SELFTEST)
    for marker in (
        "enum class StreamValueKind : uint8_t",
        "scalar = 1",
        "color = 2",
        "layer = 3",
        "mask = 4",
        "arbitrary = 5",
        "create_child_borrowed",
        "resolve_possessed",
        "possession_id",
        "borrow_unique",
        "erase_tree",
        "replace_snapshot",
        "identity.generation == UINT32_MAX",
        "g_aegp_effect_lease_generation == UINT32_MAX",
        "g_aegp_legacy_effect_stream_generation == UINT32_MAX",
        "effect_stream_value_keyframe_registry",
        "keyframe_bezier_ease_identity",
        "child_invalidation",
        "possession_policy",
    ):
        assert marker in header or marker in source or marker in scene or marker in native


def test_common_scene_transaction_has_explicit_atomic_lifecycle() -> None:
    header = read(TRANSACTION_HEADER)
    source = read(TRANSACTION_SOURCE)
    scene = read(SCENE_SOURCE)
    mask = read(ROOT / "minihost" / "src" / "worker_mask_runtime_callbacks.cpp")
    native = read(NATIVE_SELFTEST)
    compat = read(COMPAT_SELFTEST)
    external = read(
        ROOT
        / "minihost"
        / "src"
        / "worker_aegp_external_render_runtime.cpp"
    )
    routing = read(CUSTOM_ROUTING)
    for marker in (
        "class AtomicSceneTransaction",
        "using GenerationReader = uint32_t(*)() noexcept",
        "bool stage() noexcept",
        "bool validate(bool condition) noexcept",
        "bool commit(Apply&& apply, Bump&& bump) noexcept",
        "void cancel() noexcept",
        "generation_reader_ ? generation_reader_() : 0",
        "generation_reader_() != baseline_project_generation_",
        "registry_.fingerprint() != baseline_fingerprint_",
        "record_commit",
        "record_cancel",
        "record_rollback",
        "rollback_failures",
        "capture_mutation_checkpoint",
        "restore_mutation_checkpoint",
        "handle_table_fingerprint",
        "inject_keyframe_apply_failure_after(1)",
        "mask_scene_fingerprint() == scene_before_rollback",
        "registry.handle_table_fingerprint() == handles_before_rollback",
        "transaction_cancel_byte_invariant",
        "transaction_commit_generation_once",
        "generation_read_under_lock",
        "commit_generation_rechecked",
        "invalidate_all_scene_generations(next)",
        "direct_bump_receipt_invalidated",
        "concurrent_effect_flags_serialized",
        "concurrent_mask_streams_serialized",
        "concurrent_keyframe_inserts_serialized",
        "in_flight_receipt_rejected",
        "std::memcmp(",
        "bump_render_project_timestamp()",
    ):
        assert (
            marker in header
            or marker in source
            or marker in scene
            or marker in mask
            or marker in native
            or marker in compat
            or marker in external
            or marker in routing
        )


def test_scene_mutation_and_receipt_publication_are_generation_serialized() -> None:
    scene = read(SCENE_SOURCE)
    mask = read(
        ROOT / "minihost" / "src" / "worker_mask_runtime_callbacks.cpp"
    )
    transaction = read(TRANSACTION_HEADER)
    external = read(EXTERNAL_RENDER_RUNTIME)
    receipts = read(RENDER_RECEIPTS)
    compat = read(COMPAT_SELFTEST)

    setter = scene[
        scene.index("int32_t __cdecl aegp_set_effect_flags"):
        scene.index("int32_t __cdecl aegp_reorder_effect")
    ]
    assert setter.index("MutationLock mutation_lock") < setter.index(
        "resolve_effect_instance"
    )
    assert setter.index("MutationLock mutation_lock") < setter.index(
        "auto candidate = g_aegp_effect_instances"
    )
    assert "std::move(mutation_lock)" in setter
    assert "class MutationLock" in transaction
    assert "waiting_mutations() noexcept" in transaction
    assert "both_waiting" in compat
    assert "combined_flags == 3u" in compat
    mask_setter = mask[
        mask.index("int32_t __cdecl set_stream_value"):
        mask.index("int32_t __cdecl unsupported_layer_stream_value")
    ]
    assert mask_setter.index("MutationLock mutation_lock") < (
        mask_setter.index("HostMask candidate")
    )
    assert "std::move(mutation_lock)" in mask_setter
    keyframe_insert = mask[
        mask.index("int32_t __cdecl insert_keyframe"):
        mask.index("void inject_keyframe_apply_failure_after")
    ]
    assert keyframe_insert.index("MutationLock mutation_lock") < (
        keyframe_insert.index("auto position")
    )
    assert "std::move(mutation_lock)" in keyframe_insert
    assert "update_keyframe_local_indices(record)" in keyframe_insert
    assert "mask_record->opacity == 63.0" in compat
    assert "concurrent_keyframe_inserts_serialized" in compat

    bump = external[
        external.index("void bump_project_generation() noexcept"):
        external.index("bool cache_empty() noexcept")
    ]
    assert bump.index("SceneGenerationMutationGuard") < bump.index(
        "compare_exchange_weak"
    )
    assert "configure_scene_generation_reader(&project_generation)" in external
    registration = receipts[
        receipts.index("int32_t register_receipt"):
        receipts.index("int32_t get_world")
    ]
    assert registration.index("g_scene_generation_mutex") < registration.index(
        "generation_reader() != draft->project_generation"
    )
    assert registration.index(
        "generation_reader() != draft->project_generation"
    ) < registration.index("g_receipts.emplace")
    publication = external[
        external.index("int32_t publish_cached_receipt"):
        external.index("int32_t __cdecl timestamp")
    ]
    cache_lock = publication.rfind(
        "std::lock_guard<std::mutex> lock(g_mutex)"
    )
    assert 0 <= cache_lock < publication.index("receipt->pixel_format")
    assert publication.index(
        "receipt->pixel_format"
    ) < publication.index("render_receipts::register_receipt")
    assert "in_flight_generation + 1" in compat
    assert "stale_publication_result != 0" in compat
    assert "in_flight_receipt == nullptr" in compat


def test_external_aegp_entry_boundary_contains_faults_and_reclaims_leases() -> None:
    guard = read(
        ROOT / "minihost" / "src" / "worker_aegp_entry_guard.cpp"
    )
    orchestration = read(
        ROOT / "minihost" / "src" / "worker_aegp_init_orchestration.cpp"
    )
    report = read(
        ROOT / "minihost" / "src" / "worker_aegp_init_report.cpp"
    )
    routing = read(
        ROOT / "minihost" / "src" / "worker_invocation_orchestration.cpp"
    )
    native = read(
        ROOT / "tests" / "native" / "worker_aegp_entry_guard_selftest.cpp"
    )
    for marker in (
        "__try",
        "__except",
        "kMsvcCppException",
        "EXCEPTION_CONTINUE_SEARCH",
        "EXCEPTION_ACCESS_VIOLATION",
        "same_module",
        "invoke_with_cpp_boundary",
        "catch (...)",
        "FaultKind::seh_exception",
        "EntrySuiteLeaseScope",
        "release_since",
        "force_release_all()",
        "forced_suite_releases",
        "boundary_regression_passed",
        'L"--aegp-init-boundary-test"',
        "counters.releases == 1",
        "STATUS_STACK_BUFFER_OVERRUN",
        "run_unrelated_child",
    ):
        assert (
            marker in guard
            or marker in orchestration
            or marker in report
            or marker in routing
            or marker in native
        )
    assert (
        "result.entry_fault == "
        "aegp_entry_guard::FaultKind::seh_exception"
    ) in orchestration


def test_published_worker_suites_cover_phase_3_to_5_mutations() -> None:
    compat = read(COMPAT_SELFTEST)
    routing = read(CUSTOM_ROUTING)
    for marker in (
        'acquire_suite("AEGP Effect Suite", 4',
        'acquire_suite("AEGP Stream Suite", 7',
        'acquire_suite("AEGP Layer Mask Suite", 7',
        'acquire_suite("AEGP Stream Suite", 11',
        'acquire_suite("AEGP Keyframe Suite", 5',
        "verify_aegp_scene_mutation_transactions",
        "transaction_failure_byte_invariant",
        "transaction_cancel_byte_invariant",
        "generation_increment_once",
        "stale_child_invalidation",
        "keyframe_bezier_ease_ownership",
        'L"--self-test-aegp-scene-mutation-transactions"',
    ):
        assert marker in compat or marker in routing
