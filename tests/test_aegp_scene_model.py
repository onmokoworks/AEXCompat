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








def test_native_selftest_is_a_release_build_target() -> None:
    cmake = read(CMAKE)
    assert "src/worker_aegp_scene_model.cpp" in cmake
    assert "add_executable(worker_aegp_scene_model_selftest" in cmake
    assert "../tests/native/worker_aegp_scene_model_selftest.cpp" in cmake
    assert "target_compile_options(worker_aegp_scene_model_selftest PRIVATE /UNDEBUG)" in cmake








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


