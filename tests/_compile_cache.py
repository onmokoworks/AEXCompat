import os
import sys
import subprocess
from collections.abc import Iterable, Sequence
from pathlib import Path

from _msvc_compile import require_sccache


def compiler_argv(compiler: str) -> list[str]:
    """Return a compiler argv prefix with the explicitly configured cache."""
    if os.environ.get("AEXCOMPAT_COMPILE_CACHE") == "sccache":
        require_sccache()
        return ["sccache", compiler]
    return [compiler]


def portable_cxx_flags() -> list[str]:
    """Flags every portable native self-test build needs on this platform.

    On Windows these self-tests are built with the runner's preinstalled
    clang++ against the newest MSVC STL, which rejects any clang older than the
    one it was released with (STL1000, issue #1852). The self-tests use plain
    C++17 and none of the STL features behind that check, so allow the pair.
    """
    if _platform() == "win32":
        return ["-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH"]
    return []


def _platform() -> str:
    """The platform the flags are chosen for; tests replace this, not sys."""
    return sys.platform


def compile_and_link(
    compiler: str,
    sources: Iterable[Path],
    output: Path,
    *,
    compile_args: Sequence[str] = (),
    cwd: Path | None = None,
) -> None:
    """Compile cacheable translation units, then link without the cache wrapper."""
    objects = []
    for index, source in enumerate(sources):
        object_path = output.with_name(f"{output.name}.{index}.o")
        subprocess.run(
            [
                *compiler_argv(compiler),
                *portable_cxx_flags(),
                *compile_args,
                "-c",
                str(source),
                "-o",
                str(object_path),
            ],
            cwd=cwd,
            check=True,
        )
        objects.append(object_path)
    subprocess.run(
        [compiler, *(str(object_path) for object_path in objects), "-o", str(output)],
        cwd=cwd,
        check=True,
    )
