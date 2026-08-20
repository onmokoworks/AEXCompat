import dis
import json
import os
import shutil
import subprocess
from pathlib import Path

import _native_selftest
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
        ROOT / "tests" / "built_artifact_tests.txt",
        "built_artifact",
        "--run-built-artifact-tests",
        "requires workers, probes, and input fixtures built from this checkout",
    ),
)


@pytest.fixture(scope="session")
def canonical_release_worker(tmp_path_factory, request):
    if os.name != "nt":
        pytest.skip("native worker self-tests require Windows")

    # ae-sdk-tests.yml は前段の Build minihost workers ステップで同一 checkout
    # から aex_render_worker.exe をビルド済みなので、ここで再ビルドせず
    # そのバイナリを指せる (#681 の二重ビルド解消)。指定が壊れている場合は
    # fail-closed (黙ってビルドに fallback すると workflow 側の期待とずれる)。
    override = os.environ.get("AEXCOMPAT_CANONICAL_WORKER")
    if override:
        worker = Path(override)
        if not worker.is_file():
            pytest.fail(
                f"AEXCOMPAT_CANONICAL_WORKER points to a missing file: {worker}")
        return worker

    # workerinput は xdist の worker プロセスにだけ存在する。worker_id fixture
    # と違い、xdist plugin を無効にした実行 (-p no:xdist) でも壊れない。
    if getattr(request.config, "workerinput", None) is None:
        return _build_canonical_release_worker(
            tmp_path_factory.mktemp("canonical-release-worker"))

    # xdist では session fixture が worker プロセスごとに実行される。minihost の
    # MSVC ビルド (~2分) を worker の数だけ走らせないため、全 worker 共有の
    # basetemp 親ディレクトリでロックを取り、最初の worker だけがビルドして
    # 結果のパスをマーカーに書く。後続はマーカーを読んで同じバイナリを使う。
    from filelock import FileLock

    shared_root = tmp_path_factory.getbasetemp().parent
    marker = shared_root / "canonical-release-worker.json"
    with FileLock(str(marker) + ".lock"):
        if marker.is_file():
            worker = Path(json.loads(marker.read_text(encoding="utf-8"))["worker"])
            assert worker.is_file(), f"canonical worker marker is stale: {worker}"
            return worker
        build = shared_root / "canonical-release-worker"
        build.mkdir(parents=True, exist_ok=True)
        worker = _build_canonical_release_worker(build)
        # マーカーはビルド成功後にのみ書く。途中で死んだ worker が残した
        # 半端なビルドを後続が拾わないようにするため。
        marker.write_text(json.dumps({"worker": str(worker)}), encoding="utf-8")
        return worker


def _build_canonical_release_worker(build: Path) -> Path:
    program_files_x86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    vswhere = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if not vswhere.is_file():
        pytest.fail("vswhere.exe is unavailable; install Visual Studio C++ tools")
    # -utf8 forces UTF-8 output; without it vswhere emits its description strings
    # in the console code page (CP932 on a Japanese locale), which breaks the
    # utf-8-sig decode below with a UnicodeDecodeError (#58).
    # Enumerate every C++ x64 install, not just -latest: the resolved cmake may
    # predate the newest VS. cmake 3.24 knows "Visual Studio 17 2022" but not
    # "Visual Studio 18 2026", so taking the latest install and configuring its
    # generator fails outright on a machine with both VS 2022 and VS 2026 (#89).
    result = subprocess.run(
        [str(vswhere), "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-format", "json", "-utf8"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8-sig",
        errors="replace",
    )
    installations = json.loads(result.stdout)
    if not installations:
        pytest.fail("no Visual Studio installation with the C++ x64 toolset")
    cmake = shutil.which("cmake")
    if cmake is None:
        pytest.fail("cmake is unavailable on PATH")

    def installation_version(entry):
        return tuple(int(part) for part in entry["installationVersion"].split("."))

    # Ninja first (#671): builds translation units in parallel, so the worker
    # build is minutes faster than the MSBuild solution path on a hosted
    # runner, and cmake does not need to know the newest VS's solution
    # generator (the #89 failure class). vcvars64 supplies cl and, on a dev
    # box without a PATH ninja, the VS-bundled ninja. A failed attempt falls
    # through to the VS-generator path below.
    newest = sorted(installations, key=installation_version, reverse=True)[0]
    vcvars = (Path(newest["installationPath"]) / "VC" / "Auxiliary" / "Build"
              / "vcvars64.bat")
    source = ROOT / "minihost"
    if vcvars.is_file():
        ninja_build = build / "ninja"
        ninja_build.mkdir(parents=True, exist_ok=True)
        command = (
            f'@call "{vcvars}" >nul && '
            f'"{cmake}" -S "{source}" -B "{ninja_build}" -G Ninja '
            f"-DCMAKE_BUILD_TYPE=Release && "
            f'"{cmake}" --build "{ninja_build}" --target aex_render_worker'
        )
        batch = ninja_build / "build-worker.bat"
        batch.write_text(command + "\n", encoding="ascii")
        completed = subprocess.run(
            ["cmd", "/d", "/c", str(batch)], check=False, timeout=420)
        worker = ninja_build / "aex_render_worker.exe"
        if completed.returncode == 0 and worker.is_file():
            return worker
        print("canonical worker: Ninja build failed "
              f"(exit {completed.returncode}); falling back to a VS generator")

    # Generator names are ASCII, but pin the decode so a Japanese console code
    # page cannot break the substring match (cf. #58).
    cmake_help = subprocess.run(
        [cmake, "--help"],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    ).stdout
    known_generator_years = {17: "2022", 18: "2026"}

    # Prefer the newest install whose generator the resolved cmake actually
    # advertises, falling back to older toolsets (e.g. VS 2022 under cmake 3.24).
    chosen = None
    for installation in sorted(installations, key=installation_version, reverse=True):
        major = int(installation["installationVersion"].split(".", 1)[0])
        generator_year = known_generator_years.get(major)
        if generator_year is None:
            continue
        candidate = f"Visual Studio {major} {generator_year}"
        if candidate in cmake_help:
            chosen = (installation, candidate)
            break
    if chosen is None:
        pytest.fail(
            "no Visual Studio install whose CMake generator is supported by the "
            f"resolved cmake ({cmake}); install a newer cmake or an older VS toolset"
        )
    installation, generator = chosen
    vs_root = Path(installation["installationPath"])
    vcvars = vs_root / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    if not vcvars.is_file():
        pytest.fail("vcvars64.bat is unavailable; install Visual Studio C++ tools")
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


def _calls_native_selftest_run(function, seen=None):
    if seen is None:
        seen = set()
    if function in seen:
        return False
    seen.add(function)
    code = getattr(function, "__code__", None)
    globals_ = getattr(function, "__globals__", {})
    if code is None:
        return False

    instructions = tuple(dis.get_instructions(code))
    for index, instruction in enumerate(instructions):
        if instruction.opname not in {"LOAD_GLOBAL", "LOAD_NAME"}:
            continue
        value = globals_.get(instruction.argval)
        if value is _native_selftest.run:
            return True
        if value is _native_selftest and index + 1 < len(instructions):
            following = instructions[index + 1]
            if following.opname in {"LOAD_ATTR", "LOAD_METHOD"} and following.argval == "run":
                return True

    for instruction in instructions:
        if instruction.opname not in {"LOAD_GLOBAL", "LOAD_NAME"}:
            continue
        helper = globals_.get(instruction.argval)
        if (
            callable(helper)
            and getattr(helper, "__globals__", None) is globals_
            and _calls_native_selftest_run(helper, seen)
        ):
            return True
    return False


def _native_selftest_run_node_ids(items):
    node_ids = set()
    for item in items:
        function = getattr(item, "obj", None)
        if _calls_native_selftest_run(function):
            node_ids.add(item.nodeid.replace("\\", "/"))
    return node_ids


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
        built_artifact_entries = next(
            entries
            for _, marker, _, _, entries in manifests
            if marker == "built_artifact"
        )
        missing = sorted(required - collected)
        if missing:
            raise pytest.UsageError(
                "local-artifact manifest contains uncollected node ids: "
                + ", ".join(missing)
            )
        unregistered_native_selftests = sorted(
            _native_selftest_run_node_ids(items) - built_artifact_entries
        )
        if unregistered_native_selftests:
            raise pytest.UsageError(
                "tests that call _native_selftest.run are absent from "
                "built_artifact_tests.txt: " + ", ".join(unregistered_native_selftests)
            )

    # canonical_release_worker (minihost の MSVC ビルド、数分) を含む
    # モジュールを collection の先頭へ寄せる。xdist は collection 順に配る
    # ので、後半に残るとビルドがそのまま実行時間の tail になる。モジュール
    # 単位の安定ソートなので loadscope のスコープ連続性は崩れない。
    canonical_first = ("tests/test_pf_adv_time_suite1.py",
                      "tests/test_suite_entry_utility13.py")
    items.sort(key=lambda item: 0 if item.nodeid.replace(
        "\\", "/").startswith(canonical_first) else 1)
