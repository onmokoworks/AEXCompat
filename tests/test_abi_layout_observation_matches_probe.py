"""The committed ABI observation has to be what the SDK-compiled probe prints.

`analysis/AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json` is the only source of the
offsets the host writes its callback tables at, and the generator turns it into
`minihost/src/generated/aex_abi_contract.hpp` without ever seeing an SDK. So a
mistyped offset in that document survives `generate-aex-abi-contract.py --check`
(which only rejects negative, out-of-range, and duplicate values), survives every
source-only test, and reaches the worker as a callback pointer written at the
wrong slot - the shape of issues #777 and #981, except that a half-overwritten
pointer crashes somewhere worse than a null one.

This runs the compiled probe and holds the document to it. The probe emits
strictly less than the document: `utils.app` is a legacy slot at
PF_UtilCallbacks+0xC8 that no SDK header declares (issue #362), and
`supplemental_contract_source` is prose. Everything the probe does emit must
match exactly.
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
_BUILD = ROOT / "target" / "instruments-build"
# Single-config Ninja is what CI and the documented configure produce; a
# multi-config generator puts it a directory down. The newest wins rather than
# the first, so a leftover from an older layout cannot shadow a fresh build and
# turn the staleness guard below into a verdict about the document.
_CANDIDATES = [
    _BUILD / "abi_layout_probe.exe",
    _BUILD / "Release" / "abi_layout_probe.exe",
    _BUILD / "Debug" / "abi_layout_probe.exe",
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
    document: a probe built before the ANSI entries were added reports fewer
    fields, and the failure would read as the observation carrying entries the
    SDK does not have.

    This catches only the case the diff creates. A probe built from the same
    source against a different SDK tree, or against an SDK whose headers moved
    under it, still reads as fresh; what the mtime cannot answer, the probe's
    own static_asserts do at build time.
    """
    return PROBE.stat().st_mtime < PROBE_SOURCE.stat().st_mtime

# What the document carries and the probe does not. Anything else missing from
# the probe's output is drift, not a known addition.
DOCUMENT_ONLY_FIELDS = {"utils.app"}
DOCUMENT_ONLY_KEYS = {"supplemental_contract_source"}


class AbiLayoutObservationMatchesProbeTests(unittest.TestCase):
    # The wording matters: the workflow's "Fail if SDK or built-artifact tests
    # were skipped" step greps skip reasons for "is not built", so on a run that
    # provisioned the SDK and built the probe, a silent skip here fails CI
    # instead of reading as a pass.
    @unittest.skipUnless(
        PROBE.exists() or PROBE_IS_OVERRIDDEN, "abi_layout_probe is not built"
    )
    def test_every_value_the_probe_emits_matches_the_committed_observation(self):
        # The scalar comparisons below include the 24-key `selectors` map, whose
        # diff is longer than unittest's default cut-off; without this the
        # failure names the key that drifted only as "...".
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

        probe_fields = probe["fields"]
        document_fields = document["fields"]
        # Field-by-field rather than a dict compare: a mismatch has to name the
        # field, because the whole point is telling an operator which offset is
        # wrong.
        for name, value in probe_fields.items():
            self.assertIn(name, document_fields, f"{name} is missing from the observation")
            self.assertEqual(
                document_fields[name],
                value,
                f"{name} does not match what the SDK-compiled probe reports",
            )
        self.assertEqual(
            set(document_fields) - set(probe_fields),
            DOCUMENT_ONLY_FIELDS,
            "the observation carries fields the probe does not, beyond the known ones",
        )

        for key, value in probe.items():
            if key == "fields":
                continue
            self.assertIn(key, document, f"{key} is missing from the observation")
            self.assertEqual(
                document[key], value, f"{key} does not match the SDK-compiled probe"
            )
        self.assertEqual(
            set(document) - set(probe),
            DOCUMENT_ONLY_KEYS,
            "the observation carries keys the probe does not, beyond the known ones",
        )
