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


def test_loaded_plugin_aegp_stream_values_pass_on_all_workers() -> None:
    for _ in _all_workers(
        "--self-test-aegp-loaded-plugin-streams",
        "aegp_loaded_plugin_effect_streams",
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
    for _ in _all_workers("--self-test-pf-private-callbacks", "pf_private_callbacks"):
        pass


def test_bee_scene_facade_is_published_behind_the_effect_layer_on_all_workers() -> None:
    """The BEE.dll-compatible scene object behind the effect layer handle
    (issue #1210). Adobe-bundled Timecode.aex acquires "AE Timecode Helper
    Suite" v1 as a host-presence gate and then reads the AEGP_LayerH from
    AEGP PF Interface Suite::AEGP_GetEffectLayer as a `BEE_AVLayer*` (parent
    comp item at +0x260, its project at item+0x38, the item tag/type/flags, and
    the vtable slots BEE.dll's exports call; docs/BEE_SCENE_OBJECT_ABI_2026-08-17.md).
    The route checks on every worker that the gate suite acquires with 32
    distinct slots (three of them called and answering the diagnosed refusal),
    that the production hand-out returns the facade object whose comp values
    match the AEGP item/comp suites, that the observed vtable slots answer as
    recorded, and that sampled unobserved slots on each object trap with a
    code naming the slot and record the calling frame. The verdict is what is
    asserted here.
    """
    for _ in _all_workers("--self-test-bee-scene-facade", "bee_scene_facade"):
        pass


def test_selector_fault_unwind_recovers_the_caller_of_a_null_call() -> None:
    """The faulting-context unwind behind ``stage:selector_seh unwind=``
    (issue #1312). A plug-in that calls through an uninitialised Adobe-library
    dispatch slot faults at instruction pointer 0, where no unwind entry
    exists, so the caller is only recoverable by reading the return address the
    CALL pushed. The route raises exactly that shape behind the production SEH
    capture and identifies the two frames behind the fault by their unwind-table
    entry, not merely by module, so a frame the walk invented or shifted by one
    fails instead of passing.
    """
    for name, report in _all_workers(
        "--self-test-selector-fault-unwind", "selector_fault_unwind"
    ):
        assert report["reference_identities_resolved"] is True, name
        assert report["fault_site_is_null"] is True, name
        assert report["call_site_frame_identified"] is True, name
        assert report["caller_frame_identified"] is True, name
        assert report["frames"] >= 3, name


def test_pf_progress_info_is_installed_behind_effect_ref_on_all_workers() -> None:
    """The PF_ProgressInfo-shaped object behind ``in_data->effect_ref``
    (issue #1275). Adobe-bundled effects and PF.dll read the effect ref as
    ``{refcon, abort, progress}`` and call the two function slots (CannedWarp's
    RENDER and PF.dll's PF_TransferRect body call the +0x10 progress slot once
    per row; Echo substitutes the +8 slot for it). The route checks on every
    worker that the production bootstrap installs the published layout at the
    effect ref the in_data carries, that both slots read at their raw offsets
    forward to the host's abort / progress callbacks, and that a plug-in edit
    (Echo's substitution) is restored at the next hand-out. The verdict is what
    is asserted here.
    """
    for _ in _all_workers("--self-test-pf-progress-info", "pf_progress_info"):
        pass


def test_pf_world_facade_is_behind_reserved_long4_on_all_workers() -> None:
    """The PF_World-compatible object around / behind every handed-out world's
    ``reserved_long4`` (issue #1276). Adobe-bundled effects and PF.dll read the
    world as AE's PF_World: Glow calls vtable slot 1 for the depth, Spill2 and
    Curl_Noise take ``world - 8`` as a PF_World and call PF_World::CopyWorld
    (slot 14), and Channel Blur writes the world's origin through
    ``reserved_long4``. The route checks on every worker that a world prepared
    and registered the way the render paths do it carries AE's embedded shape
    (vtable at ``world - 8``), that slot 1 answers the registered depth and
    slot 14 copies pixels with the bounded, same-depth semantics PF.dll
    describes, that a bare struct gets an equivalent mirror object, and that
    sampled unobserved slots trap with a code naming the slot and record it in
    the report. The verdict is what is asserted here.
    """
    for _ in _all_workers("--self-test-pf-world-facade", "pf_world_facade"):
        pass


def test_argb32f_depth_conversion_round_trips_on_all_workers() -> None:
    """The 8/16bpc <-> float32 ARGB conversion the Premiere GPU-filter route
    (VR family, ``xGPUFilterEntry``) widens its input through and narrows its
    output through when the session is not float32 (issue #1271: with the
    route gated on float32 sessions, every VR effect answered 512 at depth 8
    and 16). The route checks that every 8-bit and every 16-bit channel value
    survives the round trip exactly, that out-of-range and non-finite floats
    narrow to the depth's bounds, and that float32 passes through unchanged;
    the verdict is what is asserted here.
    """
    for _ in _all_workers(
        "--self-test-argb32f-depth-conversion", "argb32f_depth_conversion"
    ):
        pass
