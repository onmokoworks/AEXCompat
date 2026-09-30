from _native_selftest import run


def test_only_the_read_from_bee_build_is_admitted_for_the_unexported_call():
    """Issues #1264 / #1302: the host installs BEE's BIB proc-address resolver
    by calling a function BEE.dll does not export, at an image-relative
    address. What makes that defensible is refusing every module that is not
    the build the behaviour was read from - a smaller image, a different
    function at the offset, a `cmp` that is not a null test, or one that names
    a different word than the resolver read back afterwards. A machine has one
    AE per version installed, so a corpus run exercises the accept path and
    none of the refusals; these run against synthetic images."""
    report = run(
        "worker_bee_bib_installer_selftest.exe",
        "worker_bee_bib_installer_selftest",
    )
    assert report["failures"] == []
