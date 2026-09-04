import _msvc_compile
import pytest
from _msvc_compile import compile_driver


def test_compile_driver_uses_plain_msvc_by_default(monkeypatch):
    monkeypatch.delenv("AEXCOMPAT_COMPILE_CACHE", raising=False)
    assert compile_driver() == "cl.exe"


def test_compile_driver_wraps_only_the_explicit_sccache_mode(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: f"/fake/{name}")
    assert compile_driver() == "sccache cl.exe"

    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "ccache")
    assert compile_driver() == "cl.exe"


def test_compile_driver_refuses_an_absent_sccache(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    monkeypatch.setattr(_msvc_compile.shutil, "which", lambda name: None)
    with pytest.raises(RuntimeError, match="sccache is not on PATH"):
        compile_driver()
