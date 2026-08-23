from _msvc_compile import compile_driver


def test_compile_driver_uses_plain_msvc_by_default(monkeypatch):
    monkeypatch.delenv("AEXCOMPAT_COMPILE_CACHE", raising=False)
    assert compile_driver() == "cl.exe"


def test_compile_driver_wraps_only_the_explicit_sccache_mode(monkeypatch):
    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "sccache")
    assert compile_driver() == "sccache cl.exe"

    monkeypatch.setenv("AEXCOMPAT_COMPILE_CACHE", "ccache")
    assert compile_driver() == "cl.exe"
