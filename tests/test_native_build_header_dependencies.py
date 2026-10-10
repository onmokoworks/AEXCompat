"""Real MSVC builds must track both consumers of a shared ABI header."""
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-native.ps1"
FIXTURE = ROOT / "tests" / "native" / "native_build_deps_fixture"


def invoke(argv, env):
    result = subprocess.run(
        argv, cwd=ROOT, env=env, capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=300,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout


def build(source, output, env, repair=False):
    args = [
        "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT),
        "-Source", os.path.relpath(source, ROOT), "-BuildDir", str(output),
    ]
    if repair:
        args.append("-RepairHeaderDependencies")
    return invoke(args, env)


def cache_options(output):
    entries = {}
    for line in (output / "CMakeCache.txt").read_text(encoding="utf-8").splitlines():
        if line.startswith(("#", "//")) or "=" not in line:
            continue
        name_and_type, value = line.split("=", 1)
        entries[name_and_type.split(":", 1)[0]] = value
    return entries


def assert_dependencies(output, env):
    report = invoke(["ninja", "-C", str(output), "-t", "deps"], env)
    counts = {}
    for name, count in re.findall(r"([^\r\n]+\.cpp\.obj): #deps (\d+)", report):
        for stem in ("main", "peer"):
            if name.replace("\\", "/").endswith(f"/{stem}.cpp.obj"):
                counts[stem] = int(count)
    assert set(counts) == {"main", "peer"}, report
    assert all(count >= 1 for count in counts.values()), report


def set_header(source, version, wide):
    # Generated compiler input: actual output from BOTH TUs is the oracle,
    # not substrings in the product source.
    member = "long long" if wide else "int"
    (source / "shared.hpp").write_text(
        f"#pragma once\nstruct Packet {{ {member} version; }};\n"
        f"inline constexpr int kVersion = {version};\n"
        "int peer_version();\nint peer_size();\n", encoding="utf-8",
    )


@pytest.mark.skipif(sys.platform != "win32", reason="Windows MSVC build entry point")
@pytest.mark.parametrize("repair", [False, True], ids=["fresh", "retained-cache"])
def test_native_build_tracks_shared_abi_header(tmp_path, repair):
    source = tmp_path / "source with spaces"
    output = tmp_path / "build with spaces"
    shutil.copytree(FIXTURE, source)
    env = os.environ.copy()
    env.update(TEMP=str(tmp_path), TMP=str(tmp_path), VSLANG="1041")
    build(source, output, env)
    binary = output / "native_build_deps_fixture.exe"
    assert invoke([str(binary)], env).strip() == "42 42 4 4"
    assert_dependencies(output, env)

    if repair:
        invoke([
            "cmake", "-S", str(source), "-B", str(output),
            "-DAEXCOMPAT_CACHE_SENTINEL=keep-me",
            "-DCMAKE_CXX_FLAGS=/DHEADER_DEPS_SENTINEL=1",
        ], env)
        saved = cache_options(output)
        info_files = list((output / "CMakeFiles").glob("*/CMakeCXXCompiler.cmake"))
        assert len(info_files) == 1
        info = info_files[0]
        text, count = re.subn(
            r'set\(CMAKE_CXX_CL_SHOWINCLUDES_PREFIX "[^"\n]*"\)',
            'set(CMAKE_CXX_CL_SHOWINCLUDES_PREFIX "retained wrong prefix: ")',
            info.read_text(encoding="utf-8"),
        )
        assert count == 1
        info.write_text(text, encoding="utf-8")
        invoke(["cmake", "-S", str(source), "-B", str(output)], env)
        before = info.read_bytes(), (output / "CMakeCache.txt").read_bytes()
        partial = subprocess.run([
            "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT),
            "-Source", os.path.relpath(source, ROOT), "-BuildDir", str(output),
            "-RepairHeaderDependencies", "-Target", "native_build_deps_fixture",
        ], cwd=ROOT, env=env, capture_output=True, timeout=30)
        assert partial.returncode != 0
        assert before == (info.read_bytes(), (output / "CMakeCache.txt").read_bytes())
        wrong_source = subprocess.run([
            "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT),
            "-Source", "minihost", "-BuildDir", str(output), "-RepairHeaderDependencies",
        ], cwd=ROOT, env=env, capture_output=True, timeout=30)
        assert wrong_source.returncode != 0
        assert before == (info.read_bytes(), (output / "CMakeCache.txt").read_bytes())
        outside = tmp_path / "outside compiler metadata"
        shutil.copytree(info.parent, outside)
        saved_directory = info.parent.with_name(info.parent.name + "-saved")
        info.parent.rename(saved_directory)
        # A valid earlier candidate must also survive: preflight is atomic,
        # not a loop which deletes good candidates before noticing the junction.
        earlier = output / "CMakeFiles" / "0.0.1"
        earlier.mkdir()
        earlier_info = earlier / "CMakeCXXCompiler.cmake"
        earlier_info.write_bytes(b"generated earlier candidate sentinel")
        outside_before = (outside / info.name).read_bytes()
        deps_before = (output / ".ninja_deps").read_bytes()
        junction_path = str(info.parent).replace("'", "''")
        junction_target = str(outside).replace("'", "''")
        invoke([
            "powershell", "-NoProfile", "-Command",
            f"New-Item -ItemType Junction -Path '{junction_path}' "
            f"-Target '{junction_target}' -ErrorAction Stop | Out-Null",
        ], env)
        try:
            redirected = subprocess.run([
                "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT),
                "-Source", os.path.relpath(source, ROOT), "-BuildDir", str(output),
                "-RepairHeaderDependencies",
            ], cwd=ROOT, env=env, capture_output=True, timeout=30)
            assert redirected.returncode != 0
            assert (outside / info.name).read_bytes() == outside_before
            assert earlier_info.read_bytes() == b"generated earlier candidate sentinel"
            assert (output / ".ninja_deps").read_bytes() == deps_before
            assert (output / "CMakeCache.txt").read_bytes() == before[1]
        finally:
            # rmdir removes just this generated junction, not its target tree.
            info.parent.rmdir()
            saved_directory.rename(info.parent)
            earlier_info.unlink()
            earlier.rmdir()
        assert (outside / info.name).read_bytes() == outside_before
        parent_alias = tmp_path / "parent junction"
        parent_path = str(parent_alias).replace("'", "''")
        parent_target = str(tmp_path).replace("'", "''")
        invoke([
            "powershell", "-NoProfile", "-Command",
            f"New-Item -ItemType Junction -Path '{parent_path}' "
            f"-Target '{parent_target}' -ErrorAction Stop | Out-Null",
        ], env)
        try:
            before_parent = info.read_bytes(), (output / ".ninja_deps").read_bytes(), before[1]
            redirected_parent = subprocess.run([
                "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT),
                "-Source", os.path.relpath(source, ROOT),
                "-BuildDir", str(parent_alias / output.name), "-RepairHeaderDependencies",
            ], cwd=ROOT, env=env, capture_output=True, timeout=30)
            assert redirected_parent.returncode != 0
            assert before_parent == (
                info.read_bytes(), (output / ".ninja_deps").read_bytes(),
                (output / "CMakeCache.txt").read_bytes(),
            )
        finally:
            parent_alias.rmdir()
        build(source, output, env, repair=True)
        restored = cache_options(output)
        for key in ("AEXCOMPAT_CACHE_SENTINEL", "CMAKE_CXX_FLAGS",
                    "CMAKE_CXX_COMPILER", "CMAKE_LINKER"):
            assert restored[key] == saved[key], (key, saved, restored)
        assert_dependencies(output, env)

    set_header(source, 99, True)
    build(source, output, env)
    assert invoke([str(binary)], env).strip() == "99 99 8 8"
    assert_dependencies(output, env)
    # The second edit catches a rebuild which subsequently lost its deps again.
    set_header(source, 101, False)
    build(source, output, env)
    assert invoke([str(binary)], env).strip() == "101 101 4 4"
    assert_dependencies(output, env)
