import os
import re
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def source_text():
    # Suite structs, tables, and callbacks live in the worker-runtime owner
    # set; the "not in" checks below are scoped to regex slices of specific
    # functions, so the growable contract stays safe here.
    return source_owners.worker_text()


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)










def test_pf_pixel_data_native_depth_and_registry_matrix():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-pixel-data"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_pixel_data_suite":"passed"}'
