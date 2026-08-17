from _native_selftest import run


def test_u_dll_birth_latch_keys_on_the_mapping_not_on_the_first_attempt():
    """Issue #1063: a discovery session whose first member has no U.dll must
    still birth the allocator when a later member maps it."""
    report = run(
        "worker_legacy_support_init_selftest.exe",
        "worker_legacy_support_init_selftest",
    )
    assert report["failures"] == []
