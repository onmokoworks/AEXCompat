"""Runner for the native self-test executables built out of `minihost/`.

Each of these self-tests is a standalone binary that exercises host functions
with fakes - no plug-in, no AEX, no worker process - and prints one JSON object
saying whether it passed. The Python side only has to find the binary, run it,
and read that object back, and every one of these wrappers had its own copy of
the same twenty lines. The build-layout knowledge in particular (Ninja puts the
binary flat, a multi-config generator puts it under the config directory) is
one fact, so it lives in one place: a change to it that reaches some of the
copies and not the rest fails the stragglers as "missing self-test binary"
rather than as the stale paths they are. The copies also disagreed about which
configuration directory to search first before they were folded together.
"""

import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
# Ninja puts the binary flat; a multi-config generator puts it under the config
# directory. CI uses Ninja, so the flat path comes first - which is also the
# order every wrapper but one already used. A tree carrying both a stale flat
# binary and a fresh multi-config one runs the flat one; that is a stale-build
# hazard in either order, and one order everywhere beats two.
CONFIGURATIONS = (None, "Release", "RelWithDebInfo")


def candidates(name: str, override_variable: str | None = None) -> list[Path]:
    """Where the named self-test binary may be, most likely first.

    `override_variable` names an environment variable that, when set, points at
    the binary directly - for a caller that builds it somewhere else.
    """
    configured = os.environ.get(override_variable) if override_variable else None
    found = [Path(configured)] if configured else []
    return found + [
        BUILD / name if configuration is None else BUILD / configuration / name
        for configuration in CONFIGURATIONS
    ]


def locate_optional(name: str, override_variable: str | None = None) -> Path | None:
    """`locate` for callers that have a fallback when the build is absent."""
    for candidate in candidates(name, override_variable):
        if candidate.is_file():
            return candidate
    return None


def locate(name: str, override_variable: str | None = None) -> Path:
    searched = candidates(name, override_variable)
    for candidate in searched:
        if candidate.is_file():
            return candidate
    raise AssertionError(
        "missing self-test binary, looked in: "
        + ", ".join(str(candidate) for candidate in searched)
    )


def run(name: str, report_key: str, override_variable: str | None = None) -> dict:
    """Run the named self-test and return its report, asserting it passed."""
    completed = subprocess.run(
        [str(locate(name, override_variable))],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report[report_key] == "passed", report
    return report
