import os
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
