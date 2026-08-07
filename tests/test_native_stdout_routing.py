"""Where a plug-in's own stdout goes.

Issue #905: the worker redirects the plug-in's stdout away from its own, because
that stream carries the final report the broker parses and a plug-in writing
there would corrupt it. The sink was ``NUL``, so a plug-in's diagnostics were
discarded rather than moved - DeepGlow2 has parameters that dump its PreRender
rects and buffer dimensions to ``std::cout``, and none of it was readable while
diagnosing why it refused to render.

Under ``AEXCOMPAT_EXTENDED_DIAG`` the sink is stderr instead: no protocol rides
there, and the worker's own stage lines already do. The default is unchanged.
"""

import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
WORKERS = ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")
MARKER = "native-stdout-marker"


def _run(worker: Path, diagnostics: bool) -> subprocess.CompletedProcess:
    environment = dict(os.environ)
    if diagnostics:
        environment["AEXCOMPAT_EXTENDED_DIAG"] = "1"
    else:
        environment.pop("AEXCOMPAT_EXTENDED_DIAG", None)
    return subprocess.run(
        [str(worker), "--self-test-native-stdout-routing"],
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        timeout=30,
    )


def test_plugin_stdout_is_discarded_by_default() -> None:
    for name in WORKERS:
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = _run(worker, diagnostics=False)
        assert completed.returncode == 0, completed.stderr or completed.stdout
        # The worker's own report still owns stdout.
        assert json.loads(completed.stdout) == {"native_stdout_routing": "passed"}
        assert MARKER not in completed.stdout
        assert MARKER not in completed.stderr


def test_plugin_stdout_reaches_stderr_under_extended_diagnostics() -> None:
    for name in WORKERS:
        worker = BUILD / name
        assert worker.exists(), f"build {name} before running the native test"
        completed = _run(worker, diagnostics=True)
        assert completed.returncode == 0, completed.stderr or completed.stdout
        # The report is unchanged: the plug-in's writes never join it.
        assert json.loads(completed.stdout) == {"native_stdout_routing": "passed"}
        assert MARKER not in completed.stdout
        assert MARKER in completed.stderr
