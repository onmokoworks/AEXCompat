import os


def compile_driver() -> str:
    """Return the configured MSVC compile-only driver for batch commands."""
    if os.environ.get("AEXCOMPAT_COMPILE_CACHE") == "sccache":
        return "sccache cl.exe"
    return "cl.exe"
