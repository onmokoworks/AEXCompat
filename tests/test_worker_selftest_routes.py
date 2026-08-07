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
    for _ in _all_workers(
        "--self-test-legacy-effect-compat", "legacy_effect_compat"
    ):
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
