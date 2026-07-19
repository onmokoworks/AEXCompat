import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PORTABLE_HOST_ITERATE_RESULT_2026-07-15.json"
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
PF_ANSI_RUNTIME = ROOT / "minihost" / "src" / "worker_pf_ansi_runtime.cpp"
RENDER_REPORT = ROOT / "minihost" / "src" / "worker_render_report.cpp"


def test_portable_observes_host_lifecycle_and_classic_iterate():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    lifecycle = result["lifecycle"]
    render = result["render"]

    assert result["result"] == "lifecycle_and_classic_render_completed"
    assert lifecycle["about_selector_dispatched"] is True
    assert lifecycle["host_detection_message"].endswith("17.1) or later.")
    for selector_error in (
        "sequence_setup_error",
        "sequence_resetup_error",
        "frame_setup_error",
        "frame_setdown_error",
        "sequence_setdown_error",
    ):
        assert lifecycle[selector_error] == 0
    assert lifecycle["lifecycle_data_null"] is True
    assert render["default_mix_rgba_oracle_exact"] is True
    assert render["zero_mix_identity_exact"] is True
    for ownership in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "param_checkouts_balanced",
    ):
        assert render[ownership] is True


def test_portable_bounded_ansi_callback_and_render_message_are_exposed():
    source = "\n".join(path.read_text(encoding="utf-8") for path in (
        SOURCE, PF_ANSI_RUNTIME,
    ))

    assert "kUtilsAnsiSprintf = 328" in source
    assert "required >= 0 && required <= 4096" in source
    assert "vsprintf_s(destination, static_cast<std::size_t>(required) + 1" in source
    assert "strnlen_s(format, 256) == 256" in source
    report = RENDER_REPORT.read_text(encoding="utf-8")
    assert r'\"return_message\"' in report
    assert "value.escaped_return_message" in report
