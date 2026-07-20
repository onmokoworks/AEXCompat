import json
import os
import shutil
import subprocess
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
    (
        ROOT / "tests" / "built_artifact_tests.txt",
        "built_artifact",
        "--run-built-artifact-tests",
        "requires workers, probes, and input fixtures built from this checkout",
    ),
)


@pytest.fixture(scope="session")
def canonical_release_worker(tmp_path_factory):
    if os.name != "nt":
        pytest.skip("native worker self-tests require Windows")

    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        pytest.fail("vswhere.exe is unavailable; install Visual Studio C++ tools")
    # -utf8 forces UTF-8 output; without it vswhere emits its description strings
    # in the console code page (CP932 on a Japanese locale), which breaks the
    # utf-8-sig decode below with a UnicodeDecodeError (#58).
    result = subprocess.run(
        [str(vswhere), "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-format", "json", "-utf8"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8-sig",
    )
    installations = json.loads(result.stdout)
    if not installations:
        pytest.fail("no Visual Studio installation with the C++ x64 toolset")
    installation = installations[0]
    vs_root = Path(installation["installationPath"])
    vcvars = vs_root / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    if not vcvars.is_file():
        pytest.fail("vcvars64.bat is unavailable; install Visual Studio C++ tools")
    cmake = shutil.which("cmake")
    if cmake is None:
        pytest.fail("cmake is unavailable on PATH")

    major = int(installation["installationVersion"].split(".", 1)[0])
    known_generator_years = {17: "2022", 18: "2026"}
    generator_year = known_generator_years.get(major)
    if generator_year is None:
        pytest.fail(f"unsupported Visual Studio CMake generator version: {major}")
    generator = f"Visual Studio {major} {generator_year}"
    build = tmp_path_factory.mktemp("canonical-release-worker")
    source = ROOT / "minihost"
    command = (
        f'@call "{vcvars}" >nul && '
        f'"{cmake}" -S "{source}" -B "{build}" -G "{generator}" -A x64 && '
        f'"{cmake}" --build "{build}" --config Release --target aex_render_worker'
    )
    batch = build / "build-worker.bat"
    batch.write_text(command + "\n", encoding="ascii")
    subprocess.run(["cmd", "/d", "/c", str(batch)], check=True, timeout=420)
    worker = build / "Release" / "aex_render_worker.exe"
    assert worker.is_file(), f"canonical worker was not produced: {worker}"
    return worker


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
        "--run-built-artifact-tests",
        action="store_true",
        default=False,
        help="run tests requiring workers, probes, and fixtures built from this checkout",
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
