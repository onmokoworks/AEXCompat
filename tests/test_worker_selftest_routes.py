"""Drive the worker self-test routes that lost their callers in PR #692.

Issue #798: the workers still implement these ``--self-test-*`` routes, but the
pytest files that executed them were deleted together with the source-grep
purge, leaving the routes built, linked, and never run. Each test here starts
the production workers and checks the route's own verdict plus the invariants
the route reports, which is the behavioral form issue #691 asked for.

The routes live in ``aex_worker_runtime_core``, so every production worker
exposes them; ``--self-test-aegp-layer-render-options-suite2`` is the one
render-worker-only route and its absence elsewhere is part of the contract.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")


def _run_route(worker_name: str, flag: str) -> subprocess.CompletedProcess:
    worker = BUILD / worker_name
    assert worker.exists(), f"build {worker_name} before running the native test"
    return subprocess.run(
        [str(worker), flag],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )


def _passing_report(worker_name: str, flag: str, result_key: str) -> dict:
    completed = _run_route(worker_name, flag)
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report[result_key] == "passed", completed.stdout
    return report


def _all_workers(flag: str, result_key: str):
    for name in WORKERS:
        yield name, _passing_report(name, flag, result_key)


def test_legacy_effect_compat_suites_pass_on_all_workers() -> None:
    for _ in _all_workers("--self-test-legacy-effect-compat", "legacy_effect_compat"):
        pass


def test_compute_cache_suite1_route_passes_on_all_workers() -> None:
    for _ in _all_workers("--self-test-compute-cache", "aegp_compute_cache_suite1"):
        pass


def test_aegp_layer_source_item_passes_on_all_workers() -> None:
    for name, report in _all_workers(
        "--self-test-aegp-layer-source-item", "aegp_layer_source_item"
    ):
        assert report["successful_calls"] >= 1, name
        assert report["item_type_calls"] >= 1, name


def test_aegp_scene_registry_suites_pass_on_all_workers() -> None:
    for _ in _all_workers(
        "--self-test-aegp-scene-registry-suites", "aegp_scene_registry_suites"
    ):
        pass


def test_aegp_borrowed_handle_report_passes_on_all_workers() -> None:
    for worker in WORKERS:
        completed = _run_route(worker, "--self-test-aegp-borrowed-handle-report")
        assert completed.returncode == 0, completed.stderr or completed.stdout
        report = json.loads(completed.stdout)
        assert report["stage"] == "aegp_init"
        assert report["status"] == "initialized"
        assert report["scene_registry_initialized"] is True
        assert report["borrowed_handle_issues"] == 129
        assert report["borrowed_handle_reuses"] == 1
        assert report["borrowed_handle_exhaustion_failures"] == 1
        assert report["borrowed_handle_live"] == 0
        assert report["object_record_issues"] == 768
        assert report["object_record_reuses"] == 512
        assert report["object_record_exhaustion_failures"] == 1
        assert report["object_record_live"] == 18


def test_aegp_installed_effect_catalog_passes_on_all_workers() -> None:
    for _ in _all_workers(
        "--self-test-aegp-installed-effect-catalog", "aegp_installed_effect_catalog"
    ):
        pass


def test_aegp_keyframe_mutations_pass_on_all_workers() -> None:
    for name, report in _all_workers(
        "--self-test-aegp-keyframe-mutations", "aegp_keyframe_mutations"
    ):
        assert report["mutations"] >= 1, name
        assert report["ownership_rejections"] >= 1, name
        assert report["lifetimes_balanced"] is True, name


def test_aegp_effect_param_union_suite4_passes_on_all_workers() -> None:
    for name, report in _all_workers(
        "--self-test-aegp-effect-param-union-suite4", "aegp_effect_param_union_suite4"
    ):
        assert report["successful_calls"] >= 1, name


def test_pf_batch_sampling_suite_passes_on_all_workers() -> None:
    for _ in _all_workers(
        "--self-test-pf-batch-sampling-suite", "pf_batch_sampling_suite"
    ):
        pass


def test_smart_result_skipped_passes_on_all_workers() -> None:
    for _ in _all_workers("--self-test-smart-result-skipped", "smart_result_skipped"):
        pass


def test_smart_diagnostic_auxiliary_admission_passes_on_all_workers() -> None:
    for name, report in _all_workers(
        "--self-test-smart-diagnostic-auxiliary-admission",
        "smart_diagnostic_auxiliary_admission",
    ):
        assert report["uses_effective_argc"] is True, name
        assert report["fixed_image_case_admitted"] is True, name
        assert report["commands_checked"] == 20, name


def test_smart_runtime_concurrency_passes_on_all_workers() -> None:
    for _ in _all_workers(
        "--self-test-smart-runtime-concurrency", "smart_runtime_concurrency"
    ):
        pass


def test_layer_render_options_suite2_is_a_render_worker_route() -> None:
    _passing_report(
        "aex_render_worker.exe",
        "--self-test-aegp-layer-render-options-suite2",
        "aegp_layer_render_options_suite2",
    )

    # The route needs the render worker's downstream renderer, so on the other
    # workers the flag must fall through to the usage error instead of
    # pretending to have run.
    for name in ("aex_l2_worker.exe", "aex_smart_worker.exe"):
        completed = _run_route(name, "--self-test-aegp-layer-render-options-suite2")
        assert completed.returncode != 0, name
        assert completed.stdout.strip() == "", name


def test_utility_callback_table_has_no_unwired_slot_on_all_workers() -> None:
    """The production `in_data->utils` wiring, asked of the shipping workers.

    `bindings_cover_contract_once` proves at compile time that every generated
    offset has a named source; nothing proves `make_bootstrap_abi_hooks`
    assigned it, and an unassigned one installs a null pointer that no host code
    reads. It surfaces only when a plug-in calls through it and jumps to address
    0 - #777 as a 16-bit sampling crash, #981 as three FRAME_SETUP crashes the
    SEH guard reported as error 512. The route is the only caller that can see
    the assignment list, because it lives in the worker's own translation unit.

    `_all_workers` asserts the verdict, and the route puts the offset of every
    hole on stderr; there is nothing further to assert here that would not be
    one build's constant compared against itself.
    """
    for _ in _all_workers(
        "--self-test-utility-callback-table", "utility_callback_table"
    ):
        pass


def test_pf_utils_composite_rect_is_reachable_through_in_data_utils() -> None:
    """`PF_UtilCallbacks.composite_rect` on every worker, called the way a
    plug-in calls it: the pointer is read back out of the installed
    `in_data->utils` block and invoked (issue #1252, Write_on's RENDER jumped
    to address 0 through the slot). The route checks BEHIND / COPY / IN_FRONT
    against known pixels and that malformed calls fail closed with the
    destination untouched; the verdict is what is asserted here (the route's
    metadata is a constant and would only be compared against itself).
    """
    for _ in _all_workers(
        "--self-test-pf-utils-composite-rect", "pf_utils_composite_rect"
    ):
        pass


def test_pf_utils_gaussian_kernel_is_reachable_through_in_data_utils() -> None:
    """`PF_UtilCallbacks.gaussian_kernel` on every worker, called the way a
    plug-in calls it: the pointer is read back out of the installed
    `in_data->utils` block and invoked (issue #1253, Inner/Outer Key's RENDER
    jumped to address 0 through the slot once its mask checkout succeeded).
    The route checks AE's PF_GaussianKernel values for the 1D NORMALIZED and
    2D kernels and that malformed calls fail closed with the buffer untouched;
    the verdict is what is asserted here.
    """
    for _ in _all_workers(
        "--self-test-pf-utils-gaussian-kernel", "pf_utils_gaussian_kernel"
    ):
        pass


def test_checkout_param_beyond_table_answers_like_ae_on_all_workers() -> None:
    """`PF_InteractCallbacks.checkout_param` on every worker, called through the
    installed `in_data->inter` block for slots past the published parameter
    table (issue #1251: Pixel Motion Blur checks out Timewarp's slots 29/31
    against its own 5-slot table). AE 2026 answers those with PF_Err_NONE and
    an empty layer definition and takes the checkin
    (instruments/pf-checkout-index-probe, docs/CHECKOUT_PARAM_INDEX_OBSERVATION_2026-08-17.md);
    the route checks that both the classic-context and the hosted-table path
    do the same, that the checkin balances, that an in-table slot still comes
    back with its definition, and that a negative slot and an unpublished table
    stay refused. The verdict is what is asserted here.
    """
    for _ in _all_workers(
        "--self-test-checkout-param-beyond-table", "checkout_param_beyond_table"
    ):
        pass


def test_pf_private_callbacks_answer_like_ae_on_all_workers() -> None:
    """AE's private `get_callback_addr` ids on every worker, obtained the way a
    plug-in obtains them, through the slot in the installed `in_data->utils`
    block (issue #985: Bulge asks for -5, Compound Blur / CC Cross Blur / Matte
    Choker for -2). AE 2026 answers -5 with PF.dll's PFp_GaussianValue and -2
    with FLT.dll's in-place blur, straight-alpha for request mode 1 and
    premultiplied otherwise (docs/PRIVATE_CALLBACK_IDS_OBSERVATION_2026-08-17.md,
    Frida capture of the live dispatcher); the route checks the curve values,
    the 8-bit impulse responses of both kernels and both alpha treatments
    against those captures, that an unread private id stays refused, and that
    malformed blur calls fail closed with the world untouched. The verdict is
    what is asserted here.
    """
    for _ in _all_workers(
        "--self-test-pf-private-callbacks", "pf_private_callbacks"
    ):
        pass
