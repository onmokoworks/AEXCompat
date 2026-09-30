from pathlib import Path

import _compile_cache
import _msvc_compile
import pytest
from _compile_cache import compile_and_link, compiler_argv


@pytest.fixture
def sccache_on_path(monkeypatch):
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: f"/fake/{name}")


def test_compiler_argv_refuses_an_absent_sccache(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: None)
    with pytest.raises(RuntimeError, match="sccache is not on PATH"):
        compiler_argv("clang++")


def test_compiler_argv_uses_the_compiler_directly_by_default(monkeypatch):
    monkeypatch.delenv("AEXCOMPAT_COMPILE_CACHE", raising=False)
    assert compiler_argv("clang++") == ["clang++"]


def test_compiler_argv_wraps_only_the_explicit_sccache_mode(monkeypatch, sccache_on_path):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    assert compiler_argv("clang++") == ["sccache", "clang++"]

    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "ccache")
    assert compiler_argv("clang++") == ["clang++"]


def test_compile_and_link_caches_only_compile_steps(monkeypatch, tmp_path, sccache_on_path):
    calls = []

    def record_run(argv, **kwargs):
        calls.append((argv, kwargs))

    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_compile_cache.subprocess, "run", record_run)
    output = tmp_path / "selftest"
    sources = [Path("first.cpp"), Path("second.cpp")]

    compile_and_link(
        "clang++",
        sources,
        output,
        compile_args=("-std=c++17",),
        cwd=tmp_path,
    )

    objects = [output.with_name(f"{output.name}.{index}.o") for index in range(2)]
    assert calls == [
        (
            [
                "sccache",
                "clang++",
                "-std=c++17",
                "-c",
                str(sources[0]),
                "-o",
                str(objects[0]),
            ],
            {"cwd": tmp_path, "check": True},
        ),
        (
            [
                "sccache",
                "clang++",
                "-std=c++17",
                "-c",
                str(sources[1]),
                "-o",
                str(objects[1]),
            ],
            {"cwd": tmp_path, "check": True},
        ),
        (
            ["clang++", *(str(path) for path in objects), "-o", str(output)],
            {"cwd": tmp_path, "check": True},
        ),
    ]
