from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]




def test_broker_validates_the_complete_lifecycle_contract() -> None:
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")

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
