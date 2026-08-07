import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def test_visual_audio_admission_and_audio_only_exemption():
    data = json.loads((ROOT / "analysis" / "PF_VISUAL_AUDIO_ADMISSION_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["cases"]}
    advertised = cases["pf_visual_audio_advertised_probe"]
    denied = cases["pf_visual_audio_unadvertised_probe"]
    audio_only = cases["Adobe SDK SDK_Backwards.aex"]
    sidecar = cases["pf_visual_audio_sidecar_probe"]
    assert data["sdk_constant"]["PF_OutFlag_I_USE_AUDIO"] == 1048576
    assert advertised["audio_checkout_allowed"] is True
    assert advertised["audio_source_available"] is False
    assert advertised["rejected_unadvertised_audio_checkouts"] == 0
    assert denied["audio_checkout_allowed"] is False
    assert denied["rejected_unadvertised_audio_checkouts"] == 1
    assert audio_only["audio_usage_advertised"] is False
    assert audio_only["audio_checkout_allowed"] is True
    assert audio_only["audio_checkout_calls"] == audio_only["audio_checkin_calls"] == 1
    assert sidecar["audio_checkout_calls"] == sidecar["audio_get_data_calls"] == sidecar["audio_checkin_calls"] == 1
    assert sidecar["invalid_audio_operations"] == 0
    assert [sidecar["last_audio_checkout_start_time"], sidecar["last_audio_checkout_duration"], sidecar["last_audio_checkout_time_scale"]] == [4, 6, 44100]
    assert sidecar["last_audio_window_sample_count"] == 6
    assert sidecar["audio_lifetimes_balanced"] is True
    assert sidecar["worker_classification"] == "ok"
    assert data["visual_audio_sidecar_transport_implemented"] is True

def test_audio_checkout_windows_are_bounded_owned_and_time_scaled():
    data = json.loads((ROOT / "analysis" / "PF_AUDIO_CHECKOUT_WINDOW_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["cases"]}
    boundary = cases["pf_visual_audio_boundary_probe"]
    sdk = cases["Adobe SDK SDK_Backwards.aex"]
    assert data["conversion"]["maximum_returned_samples"] == 10_000_000
    assert [item["sample_count"] for item in boundary["verified_windows"]] == [20, 8, 0, 2]
    assert boundary["verified_windows"][0]["silence_samples"] == 4
    assert boundary["verified_windows"][1]["silence_samples"] == 6
    assert boundary["checkout_calls"] == boundary["get_data_calls"] == boundary["checkin_calls"] == 4
    assert boundary["invalid_audio_operations"] == 0
    assert boundary["audio_lifetimes_balanced"] is True
    assert sdk["request"]["time_scale"] == 44100
    assert sdk["bitwise_exact_reverse"] is True

def test_audio_checkout_converts_requested_sdk_formats():
    data = json.loads((ROOT / "analysis" / "PF_AUDIO_FORMAT_CONVERSION_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    matrix = data["supported_request_matrix"]
    fixture = data["fixture"]
    assert matrix["channels"] == [1, 2]
    assert matrix["signed_pcm_bytes"] == [1, 2, 4]
    assert matrix["signed_float_bytes"] == [4]
    assert fixture["checkout_calls"] == fixture["get_data_calls"] == fixture["checkin_calls"] == 3
    assert fixture["invalid_audio_operations"] == 0
    assert fixture["rejected_audio_format_requests"] == 1
    assert fixture["audio_lifetimes_balanced"] is True
    assert fixture["verified_requests"][1]["first_interleaved_samples"] == [0, 0, 4096, 4096]
    assert fixture["verified_requests"][2]["samples"] == [128, 143, 159, 175]

def test_audio_handles_support_bounded_overlapping_lifetimes():
    data = json.loads((ROOT / "analysis" / "PF_AUDIO_MULTI_HANDLE_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    run = data["run"]
    assert data["maximum_live_handles"] == 16
    assert run["checkout_calls"] == run["get_data_calls"] == run["checkin_calls"] == 2
    assert run["peak_live_audio_handles"] == 2
    assert run["audio_handle_exhaustions"] == 0
    assert run["invalid_audio_operations"] == 0
    assert run["reverse_order_checkin"] is True
    assert run["audio_lifetimes_balanced"] is True
    assert data["negative"]["callback_rejected"] is True
    assert data["negative"]["status"] == "render_failed"
    assert data["negative"]["worker_exit_code"] == 21

def test_audio_data_includes_the_sdk_trailing_silent_frame():
    data = json.loads((ROOT / "analysis" / "PF_AUDIO_SENTINEL_FRAME_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["verified_cases"] if "format" in case}
    assert data["host_contract"]["requested_window_frames_excludes_sentinel"] is True
    assert cases["pf_visual_audio_sidecar_probe"]["returned_frames"] == 7
    assert cases["pf_visual_audio_boundary_probe"]["returned_frames"] == 1
    format_cases = [case for case in data["verified_cases"] if case["fixture"] == "pf_visual_audio_format_probe"]
    assert {case["format"] for case in format_cases} == {"unsigned_pcm8", "signed_pcm16_stereo"}
    assert data["sdk_backwards_regression"]["status"] == "render_completed"
