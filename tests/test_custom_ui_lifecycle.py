from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_minihost_dispatches_a_bounded_custom_ui_lifecycle() -> None:
    source = "\n".join(
        (ROOT / "minihost" / "src" / name).read_text(encoding="utf-8")
        for name in (
            "l2_main.cpp",
            "worker_invocation_orchestration.cpp",
            "worker_ui_event_execution.cpp",
        )
    )

    assert 'L"--l2-ui-lifecycle"' in source
    assert "std::array<int32_t, 5>{0, 1, 7, 5, 6}" in source
    assert '\\"lifecycle_context_stable\\"' in source
    assert '\\"plugin_state_before_close\\"' in source
    assert '\\"lifecycle_host_state_cleared\\"' in source
    assert "event_assignments_applied" in source
    assert "requested_parameters_json(ui_event_assignments)" in source


def test_broker_validates_the_complete_lifecycle_contract() -> None:
    source = (
        ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
    ).read_text(encoding="utf-8")

    assert "probe_experimental_custom_ui_lifecycle" in source
    assert 'json!([0, 0, 0, 0])' in source
    assert 'report.get("lifecycle_context_stable")' in source
    assert 'report.get("lifecycle_host_state_cleared")' in source
    assert 'worker_report.get("custom_ui_lifecycle_errors")' in source
    assert 'worker_report.get("custom_ui_context_closed")' in source
    assert "probe_experimental_custom_ui_idle" in source
    assert "probe_experimental_custom_ui_keydown" in source
    assert "probe_experimental_custom_ui_mouse_exited" in source
    assert 'json!([0, 0, 0, 0, 0])' in source
