"""The committed ABI observation has to be exactly what the probe prints.

`tools/refresh-aex-abi-layout-evidence.ps1` writes
`analysis/AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json` from the SDK-compiled
probe's stdout, and `generate-aex-abi-contract.py` turns that document into
`minihost/src/generated/aex_abi_contract.hpp` without ever seeing an SDK. So a
document that drifted from the probe - hand-edited, or refreshed against a
different SDK - survives `generate-aex-abi-contract.py --check` (which only
rejects negative, out-of-range, and duplicate values), survives every
source-only test, and reaches the worker as a callback pointer written at the
wrong slot. That is the shape of issues #777 and #981, except that a
half-overwritten pointer crashes somewhere worse than a null one.

This is the refresh runner's `--check`: run the probe and require the committed
document to equal its output. The runner writes stdout verbatim plus a trailing
newline, so the comparison is on parsed JSON rather than bytes - a re-run on
another line-ending setting is not drift.
"""

import json
import os
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OBSERVATION = ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json"
PROBE_SOURCE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"

# Present-but-empty counts as an override, so a mis-set variable fails the
# existence check below instead of quietly falling back to auto-discovery.
_CONFIGURED = os.environ.get("AEXCOMPAT_ABI_LAYOUT_PROBE")
PROBE_IS_OVERRIDDEN = _CONFIGURED is not None
# `refresh-aex-abi-layout-evidence.ps1` builds into `target/abi-layout-probe-build`
# (multi-config puts it a directory down); a hand-run `cmake -S instruments`
# lands in `target/instruments-build`. The newest wins rather than the first, so
# a leftover from an older layout cannot shadow a fresh build and turn the
# staleness guard below into a verdict about the document.
_CANDIDATES = [
    ROOT / "target" / build / configuration / "abi_layout_probe.exe"
    for build in ("abi-layout-probe-build", "instruments-build")
    for configuration in (".", "Release", "Debug")
]
_BUILT = sorted(
    (path for path in _CANDIDATES if path.is_file()),
    key=lambda path: path.stat().st_mtime,
    reverse=True,
)
PROBE = (
    Path(_CONFIGURED) if PROBE_IS_OVERRIDDEN else next(iter(_BUILT), _CANDIDATES[0])
)


def probe_is_stale() -> bool:
    """True when the built probe predates its source.

    CLAUDE.md's canonical verification says to run the named build script
    rather than trust an untracked `target/` artifact from a previous checkout,
    and this test would otherwise turn a stale binary into a verdict about the
    document: a probe built before a field was added reports fewer of them, and
    the failure would read as the observation carrying entries the SDK does not
    have.

    This catches only that case. A probe built from the same source against a
    different SDK tree, or against an SDK whose headers moved under it, still
    reads as fresh; what the mtime cannot answer, the probe's own
    static_asserts do at build time.
    """
    return PROBE.stat().st_mtime < PROBE_SOURCE.stat().st_mtime


class AbiLayoutObservationMatchesProbeTests(unittest.TestCase):
    @unittest.skipUnless(
        PROBE.exists() or PROBE_IS_OVERRIDDEN, "abi_layout_probe is not built"
    )
    def test_the_committed_observation_is_what_the_probe_prints(self):
        # The `selectors` map alone is 24 keys, which is past unittest's default
        # diff cut-off; without this the key that drifted prints as "...".
        self.maxDiff = None
        self.assertTrue(
            PROBE.exists(),
            f"AEXCOMPAT_ABI_LAYOUT_PROBE names {PROBE}, which does not exist",
        )
        self.assertFalse(
            probe_is_stale(),
            f"{PROBE.name} is older than {PROBE_SOURCE.name}; rebuild it before "
            "reading its output as a verdict about the observation",
        )
        printed = subprocess.run(
            [str(PROBE)], capture_output=True, text=True, check=True, timeout=60
        ).stdout
        probe = json.loads(printed)
        document = json.loads(OBSERVATION.read_text(encoding="utf-8"))

        # Field by field before the whole-document compare: a mismatch has to
        # name the field, because the point is telling an operator which offset
        # is wrong rather than printing two 450-line documents.
        for name, value in probe["fields"].items():
            self.assertIn(name, document["fields"], f"{name} is missing from the observation")
            self.assertEqual(
                document["fields"][name],
                value,
                f"{name} does not match what the SDK-compiled probe reports",
            )
        self.assertEqual(document, probe)
