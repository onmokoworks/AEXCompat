import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis/PF_SAMPLING_DEPTH_MATRIX_RESULT_2026-07-17.json"


def _expected_output(raw, width, height):
    def pixel(x, y):
        if x < 0 or y < 0 or x >= width or y >= height:
            return (0, 0, 0, 0)
        offset = (y * width + x) * 4
        return tuple(raw[offset:offset + 4])

    x, y = width // 2, height // 2
    neighbors = [pixel(x, y), pixel(x + 1, y),
                 pixel(x, y + 1), pixel(x + 1, y + 1)]
    subpixel = tuple(round(sum(p[c] for p in neighbors) / 4) for c in range(4))
    alpha_sum = sum(p[3] for p in neighbors)
    area = tuple(round(sum(p[c] * p[3] for p in neighbors) / alpha_sum)
                 for c in range(3)) + (round(alpha_sum / 4),)
    samples = [pixel(x, y), pixel(x + 1, y + 1), subpixel, area,
               pixel(0, y), (0, 0, 0, 0)]
    return bytes(channel for _row in range(height) for column in range(width)
                 for channel in samples[column % len(samples)])


def test_sampling_depth_matrix_evidence_is_authenticated_and_complete():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["result"] == "pf_sampling_8_16_float_numeric_depth_matrix_passed"
    assert [run["depth"] for run in evidence["runs"]] == [8, 16, 32]
    assert evidence["artifacts"]["worker"]["path"] == "target/minihost-build/aex_render_worker.exe"
    input_path = ROOT / evidence["artifacts"]["input"]["path"]
    expected = _expected_output(input_path.read_bytes(), 37, 23)
    assert len(expected) // 4 == 851
    for artifact in evidence["artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]
    for run in evidence["runs"]:
        assert run["exit_code"] == run["render_error"] == run["last_seh_exception_code"] == 0
        assert run["status"] == "render_completed"
        assert run["suite_acquires"] == run["suite_releases"] == 1
        assert run["guard_bytes_intact"] is True
        assert (ROOT / run["output"]["path"]).read_bytes() == expected
        for name in ("output", "report"):
            artifact = run[name]
            path = ROOT / artifact["path"]
            assert path.stat().st_size == artifact["size_bytes"]
            assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]
        report = json.loads((ROOT / run["report"]["path"]).read_text(encoding="utf-8"))
        assert report["status"] == run["status"]
        assert report["render_error"] == run["render_error"]
        assert report["pixel_format"] == run["pixel_format"]
        assert report["input_sha256"] == run["input_sha256"]
        assert report["output_sha256"] == run["internal_output_sha256"]
        assert report["suite_acquires"] == report["suite_releases"] == 1
        assert report["guard_bytes_intact"] is True
        assert report["last_seh_exception_code"] == 0
