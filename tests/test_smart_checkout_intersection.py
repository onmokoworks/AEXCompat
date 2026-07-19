import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
BUILD = ROOT / "target" / "minihost-build"


def test_l2_source_intersects_checkout_requests() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    for marker in (
        # The checkout answer is the request rect intersected with the layer
        # extent; the raw request stays recorded separately.
        "CheckoutRequestState parse_checkout_request(const void* request",
        "std::array<int32_t, 4> intersect_checkout_rect(const std::array<int32_t, 4>& rect",
        "g_input_checkout_result_rect = answer_rect(g_smart_width, g_smart_height);",
        "g_map_checkout_result_rect = answer_rect(g_smart_map_width, g_smart_map_height);",
        "hosted->checkout_rect = answer_rect(hosted->width, hosted->height);",
        # Malformed (inverted) request rects fail closed with a diagnostic
        # counter instead of being silently accepted.
        "if (request_state == CheckoutRequestState::Malformed) {\n"
        "    ++g_malformed_checkout_requests;\n"
        "    return 4;\n"
        "  }",
        # Checkout view worlds carry the intersected answer via extent_hint;
        # pixel checkout on an empty answer is refused fail-closed.
        "void write_world_extent_hint(void* world, const std::array<int32_t, 4>& rect)",
        "bool checkout_promised_no_pixels(const std::array<int32_t, 4>& rect)",
        "++g_empty_checkout_pixel_denials;",
        'L"--self-test-pf-checkout-intersection"',
        # The smart report exposes the intersected answers and counters.
        '\\"input_checkout_result_rect\\"',
        '\\"map_checkout_result_rect\\"',
        '\\"malformed_checkout_request_count\\"',
        '\\"empty_checkout_pixel_denial_count\\"',
    ):
        assert marker in source
    # The pre-checkout success paths must answer through the intersected
    # rects; the legacy full-world-only writer is gone.
    assert "void write_checkout_result(void* destination" not in source


def test_native_intersection_self_test_passes_all_three_workers() -> None:
    expected = {"pf_checkout_intersection": "passed"}
    for name in ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe"):
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = subprocess.run(
            [str(worker), "--self-test-pf-checkout-intersection"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert completed.returncode == 0, completed.stderr
        assert json.loads(completed.stdout) == expected
