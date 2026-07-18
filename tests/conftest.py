from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
LOCAL_ARTIFACT_MANIFEST = ROOT / "tests" / "local_artifact_tests.txt"


def _required_local_artifacts():
    entries = [
        line.strip()
        for line in LOCAL_ARTIFACT_MANIFEST.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.startswith("#")
    ]
    duplicates = sorted({entry for entry in entries if entries.count(entry) > 1})
    if duplicates:
        raise pytest.UsageError(
            "duplicate local-artifact manifest node ids: " + ", ".join(duplicates)
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
        "--validate-local-artifact-manifest",
        action="store_true",
        default=False,
        help="fail when a local-artifact manifest node id is not collected",
    )


def pytest_collection_modifyitems(config, items):
    required = _required_local_artifacts()
    run_local = config.getoption("--run-local-artifact-tests")
    skip = pytest.mark.skip(
        reason="requires local artifacts; rerun with --run-local-artifact-tests after the named build/gate"
    )
    for item in items:
        nodeid = item.nodeid.replace("\\", "/")
        if nodeid not in required:
            continue
        item.add_marker("local_artifact")
        if not run_local:
            item.add_marker(skip)

    if config.getoption("--validate-local-artifact-manifest"):
        collected = {item.nodeid.replace("\\", "/") for item in items}
        missing = sorted(required - collected)
        if missing:
            raise pytest.UsageError(
                "local-artifact manifest contains uncollected node ids: "
                + ", ".join(missing)
            )
