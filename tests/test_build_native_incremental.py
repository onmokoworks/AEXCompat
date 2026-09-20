"""Behavioral incremental-build coverage for the native build entrypoint."""

import hashlib
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
CREATE_NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)

pytestmark = pytest.mark.skipif(
    os.name != "nt", reason="native MSVC incremental behavior is Windows-only"
)


def _display(payload):
    for encoding in ("utf-8", "cp932"):
        try:
            return payload.decode(encoding)
        except UnicodeDecodeError:
            pass
    return payload.decode("utf-8", errors="replace")


def _run(command, *, cwd, timeout=60):
    try:
        completed = subprocess.run(
            [str(item) for item in command],
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
            check=False,
            creationflags=CREATE_NO_WINDOW,
        )
    except subprocess.TimeoutExpired as error:
        output = error.stdout or b""
        pytest.fail(
            f"command timed out after {timeout}s: {command!r}\n{_display(output)}"
        )
    assert completed.returncode == 0, (
        f"command failed ({completed.returncode}): {command!r}\n"
        f"{_display(completed.stdout)}"
    )
    return completed.stdout


def _require_windows_toolchain():
    pwsh = shutil.which("pwsh")
    cmake = shutil.which("cmake")
    ninja = shutil.which("ninja")
    missing = [
        name
        for name, value in (("pwsh", pwsh), ("cmake", cmake), ("ninja", ninja))
        if value is None
    ]
    if missing:
        pytest.skip(f"Windows native toolchain command missing: {', '.join(missing)}")

    program_files_x86 = os.environ.get("ProgramFiles(x86)")
    if not program_files_x86:
        pytest.skip("ProgramFiles(x86) is unavailable; cannot locate vswhere")
    vswhere = (
        Path(program_files_x86)
        / "Microsoft Visual Studio"
        / "Installer"
        / "vswhere.exe"
    )
    if not vswhere.is_file():
        pytest.skip(f"Visual Studio locator is missing: {vswhere}")
    query = subprocess.run(
        [
            str(vswhere),
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
        check=False,
        creationflags=CREATE_NO_WINDOW,
    )
    installation = query.stdout.decode("utf-8", errors="replace").strip()
    if query.returncode != 0 or not installation:
        pytest.skip("Visual Studio x64 C++ toolset is not installed")
    vcvars = Path(installation) / "VC" / "Auxiliary" / "Build" / "vcvars64.bat"
    if not vcvars.is_file():
        pytest.skip(f"Visual Studio x64 developer environment is missing: {vcvars}")
    return Path(pwsh), Path(ninja)


def _write_fixture(
    repository, pwsh, incoming_code_page, powershell_output_code_page
):
    tools = repository / "tools"
    source = repository / "toy"
    tools.mkdir(parents=True)
    source.mkdir()
    shutil.copy2(ROOT / "tools" / "build-native.ps1", tools / "build-native.ps1")
    (source / "CMakeLists.txt").write_text(
        """cmake_minimum_required(VERSION 3.20)
set(ENV{VSLANG} 1033)
project(AEXCompatHeaderDependencyProbe LANGUAGES CXX)
if(MSVC AND CMAKE_GENERATOR MATCHES "Ninja")
  set(CMAKE_CXX_COMPILER_LAUNCHER
      "${CMAKE_COMMAND}" -E env "VSLANG=1033")
endif()
add_executable(header_dependency_probe main.cpp)
""",
        encoding="utf-8",
    )
    (source / "main.cpp").write_text(
        """#include "value.hpp"
#include <iostream>

int main() {
  std::cout << AEXCOMPAT_PROBE_VALUE;
  return 0;
}
""",
        encoding="utf-8",
    )
    header = source / "value.hpp"
    header.write_text("#define AEXCOMPAT_PROBE_VALUE 41\n", encoding="utf-8")

    escaped_pwsh = str(pwsh).replace("%", "%%")
    wrapper = repository / "invoke-build.cmd"
    if powershell_output_code_page is None:
        invocation = (
            f'"{escaped_pwsh}" -NoLogo -NoProfile -NonInteractive '
            '-ExecutionPolicy Bypass -File "%~dp0tools\\build-native.ps1" '
            '-Source toy -BuildDir "target\\toy-build"'
        )
        output_encoding_marker = ""
    else:
        invocation = (
            f'"{escaped_pwsh}" -NoLogo -NoProfile -NonInteractive '
            '-ExecutionPolicy Bypass -Command '
            '"[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); '
            "& '%~dp0tools\\build-native.ps1' -Source toy "
            "-BuildDir 'target\\toy-build'\""
        )
        output_encoding_marker = (
            "echo AEXCOMPAT_TEST_PWSH_OUTPUT_CODE_PAGE="
            f"{powershell_output_code_page}"
        )
    wrapper.write_text(
        "\r\n".join(
            (
                "@echo off",
                f"chcp {incoming_code_page} >nul",
                "if errorlevel 1 exit /b 90",
                'set "VSLANG=1041"',
                f"echo AEXCOMPAT_TEST_INCOMING_CODE_PAGE={incoming_code_page}",
                "echo AEXCOMPAT_TEST_INCOMING_VSLANG=%VSLANG%",
                output_encoding_marker,
                invocation,
                "exit /b %errorlevel%",
                "",
            )
        ),
        encoding="ascii",
        newline="",
    )
    return header, wrapper


def _build(
    repository, wrapper, incoming_code_page, powershell_output_code_page
):
    comspec = Path(os.environ.get("ComSpec", r"C:\Windows\System32\cmd.exe"))
    assert comspec.is_file(), f"cmd.exe is missing: {comspec}"
    output = _run([comspec, "/d", "/c", wrapper], cwd=repository)
    text = _display(output)
    assert f"AEXCOMPAT_TEST_INCOMING_CODE_PAGE={incoming_code_page}" in text
    assert "AEXCOMPAT_TEST_INCOMING_VSLANG=1041" in text
    if powershell_output_code_page is not None:
        assert (
            "AEXCOMPAT_TEST_PWSH_OUTPUT_CODE_PAGE="
            f"{powershell_output_code_page}"
        ) in text
    return text


def _fingerprint(path):
    stat = path.stat()
    return {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "size": stat.st_size,
        "mtime_ns": stat.st_mtime_ns,
    }


def _run_probe(executable):
    output = _run([executable], cwd=executable.parent, timeout=10)
    return _display(output).strip()


def _dependency_record(ninja, build_dir):
    text = _display(_run([ninja, "-C", build_dir, "-t", "deps"], cwd=build_dir))
    matches = re.findall(r"(?m)^.*main\.cpp\.obj: #deps (\d+),", text)
    assert len(matches) == 1, f"expected one main.cpp dependency record:\n{text}"
    return int(matches[0]), text


@pytest.mark.parametrize(
    ("incoming_code_page", "powershell_output_code_page"),
    (
        pytest.param(932, None, id="cp932"),
        pytest.param(65001, None, id="cp65001"),
        pytest.param(932, 65001, id="cp932-pwsh-output-cp65001"),
    ),
)
def test_build_native_rebuilds_after_header_only_change(
    tmp_path, incoming_code_page, powershell_output_code_page
):
    pwsh, ninja = _require_windows_toolchain()
    repository = tmp_path / "minimal-repository"
    repository.mkdir()
    header, wrapper = _write_fixture(
        repository, pwsh, incoming_code_page, powershell_output_code_page
    )
    build_dir = repository / "target" / "toy-build"
    executable = build_dir / "header_dependency_probe.exe"

    _build(
        repository, wrapper, incoming_code_page, powershell_output_code_page
    )
    assert executable.is_file()
    objects = list(build_dir.rglob("main.cpp.obj"))
    assert len(objects) == 1, objects
    object_path = objects[0]
    first_object = _fingerprint(object_path)
    first_executable = _fingerprint(executable)
    first_output = _run_probe(executable)
    first_dep_count, first_deps = _dependency_record(ninja, build_dir)

    time.sleep(1.1)
    header.write_text("#define AEXCOMPAT_PROBE_VALUE 42\n", encoding="utf-8")
    _build(
        repository, wrapper, incoming_code_page, powershell_output_code_page
    )
    second_object = _fingerprint(object_path)
    second_executable = _fingerprint(executable)
    second_output = _run_probe(executable)
    second_dep_count, second_deps = _dependency_record(ninja, build_dir)
    observations = {
        "first_output": first_output,
        "second_output": second_output,
        "first_dependency_count": first_dep_count,
        "second_dependency_count": second_dep_count,
        "object_changed": first_object != second_object,
        "executable_changed": first_executable != second_executable,
    }
    assert first_output == "41", observations
    assert second_output == "42", observations
    assert second_object["sha256"] != first_object["sha256"], observations
    assert second_object["mtime_ns"] > first_object["mtime_ns"], observations
    assert second_executable["sha256"] != first_executable["sha256"], observations
    assert second_executable["mtime_ns"] > first_executable["mtime_ns"], observations
    assert first_dep_count > 0, first_deps
    assert second_dep_count > 0, second_deps
    normalized_deps = second_deps.replace("\\", "/").lower()
    recorded_header = os.path.relpath(header, build_dir).replace("\\", "/").lower()
    assert recorded_header in normalized_deps

    stable_object = _fingerprint(object_path)
    stable_executable = _fingerprint(executable)
    _build(
        repository, wrapper, incoming_code_page, powershell_output_code_page
    )
    assert _fingerprint(object_path) == stable_object
    assert _fingerprint(executable) == stable_executable
    assert _run_probe(executable) == "42"
