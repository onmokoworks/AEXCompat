from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
TEST_CLASSES = (
    (
        ROOT / "tests" / "local_artifact_tests.txt",
        "local_artifact",
        "--run-local-artifact-tests",
        "requires untracked native builds or machine-bound evidence",
    ),
    (
        ROOT / "tests" / "sdk_required_tests.txt",
        "sdk_required",
        "--run-sdk-tests",
        "requires AFTER_EFFECTS_SDK_ROOT and a native build toolchain",
    ),
    (
        ROOT / "tests" / "prebuilt_required_tests.txt",
        "prebuilt_required",
        "--run-prebuilt-tests",
        "requires a named native build artifact produced before pytest",
    ),
)


def _manifest_entries(path):
    entries = [
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.startswith("#")
    ]
    duplicates = sorted({entry for entry in entries if entries.count(entry) > 1})
    if duplicates:
        raise pytest.UsageError(
            f"duplicate node ids in {path.name}: " + ", ".join(duplicates)
        )
    return set(entries)


def pytest_addoption(parser):
    parser.addoption(
        "--run-local-artifact-tests",
        action="store_true",
        default=False,
        help="run tests requiring untracked native builds or machine-bound evidence",
    )
    parser.addoption(
        "--run-sdk-tests",
        action="store_true",
        default=False,
        help="run tests requiring the installed After Effects SDK",
    )
    parser.addoption(
        "--run-prebuilt-tests",
        action="store_true",
        default=False,
        help="run tests requiring prebuilt native artifacts",
    )
    parser.addoption(
        "--validate-local-artifact-manifest",
        action="store_true",
        default=False,
        help="fail when a local-artifact manifest node id is not collected",
    )


def pytest_collection_modifyitems(config, items):
    manifests = [
        (path, marker, option, reason, _manifest_entries(path))
        for path, marker, option, reason in TEST_CLASSES
    ]
    all_entries = [nodeid for *_, entries in manifests for nodeid in entries]
    duplicates = sorted({entry for entry in all_entries if all_entries.count(entry) > 1})
    if duplicates:
        raise pytest.UsageError(
            "node ids occur in multiple dependency manifests: " + ", ".join(duplicates)
        )

    for _, marker, option, reason, required in manifests:
        skip = pytest.mark.skip(reason=f"{reason}; rerun with {option}")
        for item in items:
            if item.nodeid.replace("\\", "/") not in required:
                continue
            item.add_marker(marker)
            if not config.getoption(option):
                item.add_marker(skip)

    if config.getoption("--validate-local-artifact-manifest"):
        collected = {item.nodeid.replace("\\", "/") for item in items}
        required = set(all_entries)
        missing = sorted(required - collected)
        if missing:
            raise pytest.UsageError(
                "local-artifact manifest contains uncollected node ids: "
                + ", ".join(missing)
            )
