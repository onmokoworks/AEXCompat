import json
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("smartfx_geometry_flags")
BUILD = ROOT / "target" / "minihost-build"


def test_smart_sources_validate_extra_pixels_and_geometry_rects() -> None:
    source = "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
    for marker in (
        # RETURNS_EXTRA_PIXELS (PF_PreRenderOutput flags bit 0x1) is read and
        # containment against the output request is validated; an overrun
        # without the flag is an explicit diagnostic.
        "(read<uint16_t>(dispatch_state.pre_output, 34) & 0x1u) != 0;",
        "render::smart_rect_contained(smart_bounds.result_rect, expected_request);",
        "result.extra_pixels_contract_violation = result.rects_valid &&",
        # A legally empty result_rect skips the render selector instead of
        # dispatching into a zero-sized world or failing the run; one
        # predicate drives the dispatch, the GPU transport, and reporting.
        "result.empty_result_rect = result.rects_valid && smart_bounds.empty_result;",
        "if (result.empty_result_rect && result.pre_error == 0) {",
        "const bool will_dispatch = result.pre_error == 0 && result.rects_valid &&",
        "result.gpu_render_dispatched = render_selector == kSmartRenderGpu && will_dispatch;",
        "} else if (will_dispatch && transport_ready) {",
        # Rect validation is a named, self-testable function with an absolute
        # coordinate bound.
        "bool smart_geometry_rect_valid(const std::array<int32_t, 4>& rect)",
        "constexpr int32_t kMaxSmartRectMagnitude = 1 << 24;",
        # The output world extent_hint is read back from the world block the
        # plug-in saw; the empty answer reports the empty extent.
        "std::memcpy(result.output_extent_hint.data(), r.output_world->data() + 44,",
        "if (result.empty_result_rect) result.output_extent_hint = {0, 0, 0, 0};",
        'L"--self-test-pf-smart-geometry-rects"',
        # The smart report exposes the new geometry diagnostics and reflects
        # the real selector dispatch decision.
        '\\"returns_extra_pixels\\"',
        '\\"result_within_request\\"',
        '\\"extra_pixels_contract_violation\\"',
        '\\"empty_result_rect\\"',
        "smart.selector_dispatched, false},",
    ):
        assert marker in source


def test_native_geometry_rect_self_test_passes_all_three_workers() -> None:
    expected = {"pf_smart_geometry_rects": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-pf-smart-geometry-rects"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected
