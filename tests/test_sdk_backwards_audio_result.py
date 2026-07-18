import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_BACKWARDS_AUDIO_RESULT_2026-07-15.json"
WORKER_SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp",
    ROOT / "minihost" / "src" / "host_audio_runtime.hpp",
    ROOT / "minihost" / "src" / "host_audio_runtime.cpp",
)
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
PROBE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"


def test_sdk_backwards_audio_matches_the_bitwise_reverse_oracle():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    render = result["render"]

    assert result["result"] == "isolated_float32_audio_render_completed"
    assert render["sample_rate"] == 44100
    assert render["channels"] == 1
    assert render["input_samples"] == render["output_samples"] == 16
    assert render["input_sha256"] != render["output_sha256"]
    assert render["tone_level"] == 0
    assert render["bitwise_exact_reverse"] is True
    assert render["first_output_sample"] == 0.9375
    assert render["last_output_sample"] == 0
    assert render["guard_bytes_intact"] is True
    assert render["samples_finite"] is True


def test_audio_selector_and_checkout_lifetimes_are_balanced():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    lifecycle = result["lifecycle"]
    abi = result["sdk_abi"]

    for key, value in lifecycle.items():
        if key.endswith("_error"):
            assert value == 0
    assert lifecycle["checkout_calls"] == lifecycle["checkin_calls"] == 1
    assert lifecycle["audio_usage_advertised"] is False
    assert lifecycle["audio_checkout_allowed"] is True
    assert lifecycle["get_data_calls"] == 1
    assert lifecycle["invalid_audio_operations"] == 0
    assert lifecycle["audio_lifetimes_balanced"] is True
    assert [abi["audio_render_selector"], abi["audio_setup_selector"], abi["audio_setdown_selector"]] == [19, 20, 21]
    assert [abi["checkout_audio_callback_offset"], abi["checkin_audio_callback_offset"], abi["get_audio_data_callback_offset"]] == [48, 56, 64]


def test_audio_abi_is_instrumented_and_runtime_boundaries_are_explicit():
    probe = PROBE.read_text(encoding="utf-8")
    worker = "\n".join(path.read_text(encoding="utf-8") for path in WORKER_SOURCES)
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")

    for marker in (
        "PF_SoundFormatInfo",
        "PF_SoundWorld",
        "PF_Cmd_AUDIO_RENDER",
        "inter.checkout_layer_audio",
        "utils.ansi_sin",
    ):
        assert marker in probe
    for marker in (
        'mode.audio_mode = effective_argc == 9 && equals(command, L"--render-audio")',
        "kAudioGuardSamples",
        "checkout_layer_audio",
        "audio_lifetimes_balanced",
        "checkout_allowed",
        "kUtilsAnsiSin",
    ):
        assert marker in worker
    for marker in (
        "MAX_SAMPLES: usize = 10_000_000",
        'repository.join("target/audio-transport")',
        "worker_input.clone(), worker_output.clone()",
        ".create_new(true)",
        "audio worker contract failed",
    ):
        assert marker in broker
    assert 'args[1] == "--render-experimental-audio-request"' in harness


def test_audio_only_effect_is_never_dispatched_through_an_image_selector():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    gate = result["image_media_negotiation"]
    worker = "\n".join(path.read_text(encoding="utf-8") for path in WORKER_SOURCES)
    harness = HARNESS.read_text(encoding="utf-8")

    assert gate["advertisement_flag"] == "PF_OutFlag_AUDIO_EFFECT_ONLY"
    assert gate["advertisement_bit"] == 31
    assert gate["checked_before_classic_or_smart_selector"] is True
    assert gate["matrix_case_count"] == gate["matrix_unsupported_count"] == 6
    assert gate["matrix_applicable_count"] == gate["matrix_failed_count"] == 0
    assert gate["classification"] == "unsupported_media_type"
    assert gate["failure_stage"] == "media_type_negotiation"
    assert gate["image_outputs_created"] == 0
    assert "constexpr uint32_t kOutFlagAudioEffectOnly = 1u << 31;" in worker
    assert "params_error == 0 && image_render_supported && depth_supported" in worker
    assert '"unsupported_media_type"' in harness
    assert 'Some("media_type_negotiation".to_owned())' in harness
