from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
CONFORMANCE = ROOT / "broker" / "crates" / "broker" / "src" / "conformance.rs"
IMAGE_RENDER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"

def test_runtime_report_promotes_native_suite_timeline():
    text = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    # The second assertion pinned the one-shot's gpu_attempt projection, which
    # copied suite_timeline out of a failed GPU launch's report before the CPU
    # retry. #365 deleted that retry (a session cannot retry mid-flight), so the
    # timeline reaches the public report only through the projection above.
    assert '"suite_timeline": initial_report.as_ref()' not in text
