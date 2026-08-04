"""Header-dependency tracking must stay enforced for the minihost build (#657).

Ninja + MSVC recovers header dependencies by matching cl's ``/showIncludes``
output against the localized ``msvc_deps_prefix`` captured at configure time.
When configure and build disagree on the compiler's language, nothing matches,
ninja records zero dependencies per object, and the directory answers "no work
to do" however many headers change - linking objects compiled against older
headers into a worker that looks current. That produced a worker whose
GLOBAL_SETUP dispatch access-violated for every AEX (#651).

Two things keep that from recurring, and both are pinned here: the build pins
the diagnostic language on both sides, and a verification script fails closed
on a directory that already lost the tracking.
"""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CMAKELISTS = ROOT / "minihost" / "CMakeLists.txt"
VERIFIER = ROOT / "tools" / "verify-minihost-build-deps.ps1"
BUILD_REQUIREMENTS = ROOT / "docs" / "BUILD_REQUIREMENTS.md"


class MinihostBuildDependencyTracking(unittest.TestCase):
    def setUp(self) -> None:
        self.cmake = CMAKELISTS.read_text(encoding="utf-8")

    def test_diagnostic_language_is_pinned_before_the_compiler_is_probed(self) -> None:
        """CMake captures the prefix while probing, so the pin must precede it."""
        pin = self.cmake.index("set(ENV{VSLANG}")
        project = self.cmake.index("project(AEXCompatMinihost")
        self.assertLess(
            pin,
            project,
            "VSLANG must be pinned before project(): the /showIncludes prefix is "
            "captured while the compiler is probed",
        )

    def test_a_non_numeric_language_is_not_forwarded(self) -> None:
        """The value reaches every compile, so only a bare LCID is honored."""
        self.assertRegex(
            self.cmake,
            r'if\(NOT AEXCOMPAT_VSLANG MATCHES "\^\[0-9\]\+\$"\)\s*\n\s*set\(AEXCOMPAT_VSLANG 1033\)',
        )

    def test_the_build_side_pin_forwards_the_same_language(self) -> None:
        """Configure-time alone is not enough: the build is a separate process.

        Pinning only the probe leaves the prefix agreeing purely by luck - the
        exact failure of #651, where the recorded prefix was Japanese and later
        builds emitted English.
        """
        launcher = re.search(
            r"set\(CMAKE_CXX_COMPILER_LAUNCHER\s*\n?\s*"
            r'"\$\{CMAKE_COMMAND\}" -E env "VSLANG=\$\{AEXCOMPAT_VSLANG\}"\)',
            self.cmake,
        )
        self.assertIsNotNone(
            launcher, "every compile must run with the pinned VSLANG"
        )
        self.assertIn(
            "AND NOT CMAKE_CXX_COMPILER_LAUNCHER",
            self.cmake,
            "an operator's own launcher (ccache and friends) must not be replaced",
        )




if __name__ == "__main__":
    unittest.main()
