from types import SimpleNamespace

from conftest import _native_selftest_run_node_ids


def test_native_selftest_run_detection_tracks_targets_and_helpers():
    namespace = {}
    exec(
        """
import _native_selftest as native_module
from _native_selftest import run as imported_run

def direct_import():
    imported_run("direct.exe", "direct")

def module_alias():
    native_module.run("module.exe", "module")

def helper():
    imported_run("helper.exe", "helper")

def indirect_helper():
    helper()

class Other:
    def run(self):
        return None

    def helper(self):
        return None

def unrelated_run():
    native_module.locate_optional("optional.exe")
    Other().run()

def unrelated_helper_name():
    Other().helper()
""",
        namespace,
    )
    items = [
        SimpleNamespace(nodeid="tests/direct.py::test_direct", obj=namespace["direct_import"]),
        SimpleNamespace(nodeid="tests/module.py::test_module", obj=namespace["module_alias"]),
        SimpleNamespace(nodeid="tests/helper.py::test_helper", obj=namespace["indirect_helper"]),
        SimpleNamespace(
            nodeid="tests/parameterized.py::test_case[first]",
            obj=namespace["direct_import"],
        ),
        SimpleNamespace(
            nodeid="tests/parameterized.py::test_case[second]",
            obj=namespace["direct_import"],
        ),
        SimpleNamespace(
            nodeid="tests/unrelated.py::test_unrelated", obj=namespace["unrelated_run"]
        ),
        SimpleNamespace(
            nodeid="tests/unrelated.py::test_helper_name",
            obj=namespace["unrelated_helper_name"],
        ),
    ]

    assert _native_selftest_run_node_ids(items) == {
        "tests/direct.py::test_direct",
        "tests/module.py::test_module",
        "tests/helper.py::test_helper",
        "tests/parameterized.py::test_case[first]",
        "tests/parameterized.py::test_case[second]",
    }
