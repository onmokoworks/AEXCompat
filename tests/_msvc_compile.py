import os
import shutil


def require_sccache() -> None:
    """Fail with a readable message when the requested cache wrapper is absent.

    Without this, cmd reports ``'sccache' is not recognized`` (exit 9009) from
    inside a vcvars batch and the test looks like a compiler failure.
    """
    if shutil.which("sccache") is None:
        raise RuntimeError(
            "AEXCOMPAT_COMPILE_CACHE=sccache but sccache is not on PATH; "
            "install it or unset the variable to compile without a cache"
        )


def compile_driver() -> str:
    """Return the configured MSVC compile-only driver for batch commands."""
    if os.environ.get("AEXCOMPAT_COMPILE_CACHE") == "sccache":
        require_sccache()
        return "sccache cl.exe"
    return "cl.exe"
