from _native_selftest import run


def test_sweetpea_bootstrap_and_pica_load_locking():
    """Issue #1279: Sweet Pea is bootstrapped through U.dll's own U_SP_Birth
    whenever U.dll is mapped, because only that path registers the host
    plug-in whose Startup message makes U_SP_GetSPBasicSuite stop answering
    11; the latch keys on the U.dll mapping so a session member without U.dll
    decides nothing for the members after it. Issue #1287 also requires the
    actual loader callback to run outside the PICA component mutex."""
    report = run(
        "worker_sweetpea_bootstrap_selftest.exe",
        "worker_sweetpea_bootstrap_selftest",
    )
    assert report["failures"] == []
    assert report["checks"] > 0
