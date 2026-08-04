import os
import re
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
PF_SUITES = ROOT / "minihost" / "src" / "worker_pf_suites.cpp"
PF_SUITES_ABI = ROOT / "minihost" / "src" / "worker_pf_suites_internal.hpp"
SOURCE = source_owners.L2_MAIN
def source_text():
    return "\n".join(path.read_text(encoding="utf-8") for path in (SOURCE, source_owners.SRC / "worker_host_suite_wiring.cpp", PF_SUITES_ABI, PF_SUITES))


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)












def test_iterate_native_lut_non_clip_generic_and_error_paths():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-iterate"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_iterate_suite":"passed"}'
