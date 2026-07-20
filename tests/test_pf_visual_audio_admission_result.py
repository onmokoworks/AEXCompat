import json
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]


def worker_source():
    return (source_owners.worker_text() + "\n" +
            (ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp").read_text(encoding="utf-8") + "\n" +
            (ROOT / "minihost" / "src" / "host_audio_runtime.hpp").read_text(encoding="utf-8") + "\n" +
            (ROOT / "minihost" / "src" / "host_audio_runtime.cpp").read_text(encoding="utf-8"))


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


def test_worker_checks_admission_before_source_availability():
    source = worker_source()
    admission = source.index("if (!telemetry_.checkout_allowed)")
    source_check = source.index("!source_", admission)
    assert admission < source_check
    assert "audio_only_mode || usage_advertised" in source
    assert 'L"--render-image-audio"' in source
    fixture = (ROOT / "instruments" / "pf-visual-audio-probe" / "pf_visual_audio_probe.cpp").read_text(encoding="utf-8")
    assert "PF_CHECKOUT_LAYER_AUDIO" in fixture
    assert "PF_GET_AUDIO_DATA" in fixture
    broker = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(encoding="utf-8")
    harness = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text(encoding="utf-8")
    assert "render_experimental_image_with_audio_sidecar" in broker
    assert "audio sidecar contains a non-finite sample" in broker
    assert "--render-experimental-image-audio-sidecar" in harness
    assert "Select visual audio sidecar (.f32, optional)" in harness
    assert "Audio sidecar requires plain classic ARGB8 rendering." in harness


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

    worker = worker_source()
    assert "std::vector<unsigned char> samples;" in worker
    assert "kMaxCheckoutSamples = 10'000'000" in worker
    assert "handle.samples.assign" in worker
    assert "const bool sentinel_frame = frame == window_count" in worker
    assert "telemetry_.last_window_silence_samples" in worker
    assert "write<uint32_t>(input, kInTimeScale, 44100);" in worker
    broker = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(encoding="utf-8")
    assert '"last_audio_window_sample_count"' in broker
    assert '"last_audio_window_silence_samples"' in broker
    assert '"last_audio_output_channels"' in broker


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
    source = worker_source()
    assert "requested_rate" in source
    assert "source_position" in source
    assert "std::clamp(value, -1.0f, 1.0f)" in source
    assert "telemetry_.last_output_format" in source
    assert "telemetry_.rejected_format_requests" in source


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
    source = worker_source()
    assert "std::array<Handle, 16> handles_" in source
    assert "live_handle_count" in source
    assert "audio_handle_lifetimes_balanced()" in source
    assert "telemetry_.peak_live_handles" in source


def test_audio_data_includes_the_sdk_trailing_silent_frame():
    data = json.loads((ROOT / "analysis" / "PF_AUDIO_SENTINEL_FRAME_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
    cases = {case["fixture"]: case for case in data["verified_cases"] if "format" in case}
    assert data["host_contract"]["requested_window_frames_excludes_sentinel"] is True
    assert cases["pf_visual_audio_sidecar_probe"]["returned_frames"] == 7
    assert cases["pf_visual_audio_boundary_probe"]["returned_frames"] == 1
    format_cases = [case for case in data["verified_cases"] if case["fixture"] == "pf_visual_audio_format_probe"]
    assert {case["format"] for case in format_cases} == {"unsigned_pcm8", "signed_pcm16_stereo"}
    assert data["sdk_backwards_regression"]["status"] == "render_completed"
    source = worker_source()
    assert "returned_frames = window_count + 1" in source
    assert "sentinel_frame = frame == window_count" in source
    assert "telemetry_.last_returned_sample_frames" in source
