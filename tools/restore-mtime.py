"""Set each tracked file's mtime to the commit that last touched it.

A clean clone gives every file the checkout's own timestamp, so cargo sees
sources newer than any restored target/ and rebuilds everything. Git knows when
each file actually changed; restoring that keeps unchanged files older than the
cached artifacts while a file the branch touched still looks new.
"""

import os
import subprocess
import sys
from pathlib import Path


def main(root: Path) -> int:
    log = subprocess.run(
        ["git", "log", "--name-only", "--no-renames", "--format=%ct", "--reverse"],
        cwd=root, check=True, capture_output=True, text=True,
        encoding="utf-8", errors="replace",
    ).stdout

    latest: dict[str, int] = {}
    stamp = 0
    for line in log.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.isdigit():
            stamp = int(line)
        else:
            latest[line] = stamp

    tracked = set(subprocess.run(
        ["git", "ls-files"], cwd=root, check=True, capture_output=True, text=True,
        encoding="utf-8", errors="replace",
    ).stdout.split("\n"))

    applied = 0
    for name, when in latest.items():
        if name not in tracked:
            continue
        path = root / name
        try:
            os.utime(path, (when, when))
            applied += 1
        except OSError:
            continue
    print(f"restored mtime on {applied} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()))
