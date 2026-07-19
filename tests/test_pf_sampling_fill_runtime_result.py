import json
import hashlib
import math
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_SAMPLING_FILL_RUNTIME_RESULT_2026-07-16.json"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"
WORLD_TRANSFORM = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_sampling_runtime_measurement_is_fixed():
    sampling = result()["sampling_probe"]
    report = json.loads((ROOT / sampling["report"]).read_text(encoding="utf-8"))
    assert sampling["artifact"] == "target/pf-sampling-probe-build/Release/pf_sampling_probe.aex"
    artifact = ROOT / sampling["artifact"]
    assert sampling["size_bytes"] == artifact.stat().st_size
    assert sampling["sha256"] == hashlib.sha256(artifact.read_bytes()).hexdigest()
    assert (sampling["width"], sampling["height"]) == (37, 23)
    assert sampling["exit_code"] == sampling["render_error"] == 0
    assert sampling["status"] == "render_completed"
    assert sampling["output_sha256"] == "5776731f99b55ef990d41cc4a17a8e342e059d06b669155bdf9298fbcaf007f4"
    assert sampling["guard_bytes_intact"] is True
    assert sampling["suite_leases_balanced"] is True
    assert sampling["suite_acquires"] == sampling["suite_releases"] == 1
    assert sampling["last_seh_exception_code"] == 0
    assert sampling["last_seh_exception_address"] == 0
    assert sampling["last_seh_exception_module"] == ""
    assert report["status"] == sampling["status"]
    assert report["render_error"] == sampling["render_error"]
    assert report["output_sha256"] == sampling["output_sha256"]
    assert report["guard_bytes_intact"] is sampling["guard_bytes_intact"]
    assert report["suite_leases_balanced"] is sampling["suite_leases_balanced"]
    assert report["suite_acquires"] == sampling["suite_acquires"]
    assert report["suite_releases"] == sampling["suite_releases"]
    assert report["last_seh_exception_code"] == sampling["last_seh_exception_code"]
    assert report["last_seh_exception_address"] == sampling["last_seh_exception_address"]
    assert report["last_seh_exception_module"] == sampling["last_seh_exception_module"]


def test_current_sampling_artifacts_are_authenticated():
    artifacts = result()["authenticated_current_sampling_artifacts"]
    for artifact in artifacts.values():
        path = ROOT / artifact["path"]
        assert path.is_file(), artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_fill_premultiply_runtime_measurement_is_fixed():
    fill = result()["fill_premultiply_probe"]
    report = json.loads((ROOT / fill["report"]).read_text(encoding="utf-8"))
    assert fill["artifact"] == "target/pf-fill-premultiply-probe/pf_fill_premultiply_probe.aex"
    artifact = ROOT / fill["artifact"]
    assert fill["size_bytes"] == artifact.stat().st_size
    assert fill["sha256"] == hashlib.sha256(artifact.read_bytes()).hexdigest()
    assert fill["exit_code"] == fill["render_error"] == 0
    assert fill["status"] == "render_completed"
    assert fill["output_sha256"] == "aa040b86e3ed808472e2400316674b2186c0748782f2ba26fe1a116837ba335b"
    assert fill["guard_bytes_intact"] is True
    assert fill["suite_leases_balanced"] is True
    assert fill["suite_acquires"] == fill["suite_releases"] == 2
    assert fill["last_seh_exception_code"] == 0
    assert fill["last_seh_exception_address"] == 0
    assert fill["last_seh_exception_module"] == ""
    assert report["status"] == "render_completed"
    assert report["render_error"] == fill["render_error"]
    assert report["output_sha256"] == fill["output_sha256"]
    assert report["guard_bytes_intact"] is fill["guard_bytes_intact"]
    assert report["suite_leases_balanced"] is fill["suite_leases_balanced"]
    assert report["suite_acquires"] == fill["suite_acquires"]
    assert report["suite_releases"] == fill["suite_releases"]
    assert report["last_seh_exception_code"] == fill["last_seh_exception_code"]
    assert report["last_seh_exception_address"] == fill["last_seh_exception_address"]
    assert report["last_seh_exception_module"] == fill["last_seh_exception_module"]


def _expected_fill_color_output(depth):
    maximum = {8: 255, 16: 32768, 32: 1.0}[depth]
    alpha = [0.0, 1.0 / 255.0, 0.5, 128.0 / 255.0, 1.0]
    red = [1.0, 1.0, 1.0, 127.0 / 255.0, 1.0]
    matte = [0.25, 0.8, 0.4, 0.6]

    def round_half_up(value):
        return math.floor(value + 0.5)

    result = bytearray()
    for y in range(23):
        for x in range(37):
            index = x % 5
            normalized = [alpha[index], red[index], 0.25 if y & 1 else 0.75,
                          0.501 if index & 1 else 0.499]
            if depth == 32:
                source = normalized
                typed_matte = matte
            else:
                source = [round_half_up(value * maximum) for value in normalized]
                typed_matte = [round_half_up(value * maximum) for value in matte]
            pixel_alpha = source[0] / maximum
            typed = [source[0]] + [
                source[channel] * pixel_alpha + typed_matte[channel] * (1 - pixel_alpha)
                for channel in range(1, 4)
            ]
            result.extend(round_half_up(max(0, min(maximum, typed[channel])) * 255 / maximum)
                          for channel in (1, 2, 3, 0))
    return bytes(result)


def test_fill_color_depth_matrix_has_independent_numeric_oracle():
    runs = result()["fill_color_depth_matrix"]["runs"]
    assert [run["depth"] for run in runs] == [8, 16, 32]
    assert [run["operation"] for run in runs] == [
        "premultiply_color8", "premultiply_color16", "premultiply_color_float"
    ]
    for run in runs:
        assert run["exit_code"] == run["render_error"] == run["last_seh_exception_code"] == 0
        assert run["status"] == "render_completed"
        assert run["suite_acquires"] == run["suite_releases"] == 2
        assert run["guard_bytes_intact"] is True
        assert (ROOT / run["output"]["path"]).read_bytes() == _expected_fill_color_output(run["depth"])
        for name in ("output", "report"):
            artifact = run[name]
            path = ROOT / artifact["path"]
            assert path.stat().st_size == artifact["size_bytes"]
            assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]
        report = json.loads((ROOT / run["report"]["path"]).read_text(encoding="utf-8"))
        assert report["pixel_format"] == run["pixel_format"]
        assert report["input_sha256"] == run["input_sha256"]
        assert report["output_sha256"] == run["internal_output_sha256"]


def test_sampling_area_callbacks_occupy_the_frozen_suite_slots():
    worker = WORKER.read_text(encoding="utf-8")
    expected = {
        "g_sampling8_suite1[2]": "area_sample8",
        "g_sampling16_suite1[2]": "area_sample16",
        "g_sampling_float_suite1[2]": "area_sample_float",
    }
    assert result()["source_contract"]["sampling_area_suite_slot"] == 2
    assert result()["source_contract"]["sampling_area_callbacks"] == list(expected.values())
    for slot, callback in expected.items():
        assert f"{slot} = reinterpret_cast<void*>(&{callback});" in worker


def test_fill_premultiply_callbacks_occupy_the_frozen_suite_slots():
    worker = WORLD_TRANSFORM.read_text(encoding="utf-8")
    callbacks = [
        "premultiply_world8",
        "premultiply_color8",
        "premultiply_color16",
        "premultiply_color_float",
    ]
    contract = result()["source_contract"]
    assert contract["fill_premultiply_callbacks"] == callbacks
    assert contract["fill_premultiply_suite_slots"] == [3, 4, 5, 6]
    provider = worker.index("const void* provide_fill_matte2(")
    aggregate = worker[worker.index("void* callbacks[] = {", provider) : worker.index(
        "std::copy(std::begin(callbacks)", provider
    )]
    for callback in callbacks:
        assert f"reinterpret_cast<void*>(&{callback})" in aggregate
    assert aggregate.index("&premultiply_world8") < aggregate.index("&premultiply_color8")
    assert aggregate.index("&premultiply_color8") < aggregate.index("&premultiply_color16")
    assert aggregate.index("&premultiply_color16") < aggregate.index("&premultiply_color_float")


def test_evidence_is_runtime_success_with_ae_pixel_oracle_pending():
    scope = result()["scope"]
    assert scope["aexcompat_runtime_success"] is True
    assert scope["ae_pixel_oracle_comparison"] == "pending"
    assert "AEXCompat runtime success only" in scope["note"]
    assert "After Effects pixel oracle is pending" in scope["note"]
