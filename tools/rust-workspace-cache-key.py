"""Content key for CI workspace artifacts; never an AEX admission check.

Cargo's path-source mtime check cannot distinguish older, changed PR sources
from a newer main target cache. Put this digest in the cache *restore prefix*.
Keep the provider's compiler/environment/lock keys as additional boundaries.
"""

import argparse
import hashlib
import os
import stat
import struct
import sys
from pathlib import Path


INPUT_DIRECTORIES = ("broker", "profiles", ".cargo", ".github", ".config")
INPUT_FILES = (
    "LICENSE", "THIRD_PARTY_LICENSES.txt", "rust-toolchain.toml", "rust-toolchain",
    "tools/restore-mtime.py", "tools/rust-workspace-cache-key.py",
    "tools/run-broker-rust-tests.py",
)
REQUIRED_FILES = (
    "broker/Cargo.toml", "broker/Cargo.lock", "LICENSE", "THIRD_PARTY_LICENSES.txt",
    "rust-toolchain.toml", "tools/restore-mtime.py", "tools/rust-workspace-cache-key.py",
    "tools/run-broker-rust-tests.py", ".github/actions/ci-setup/action.yml",
    ".github/workflows/windows-clean-clone.yml",
    "profiles/scattermap/parameter_descriptors.json",
    "profiles/maskoffset/parameter_descriptors.json",
)


class InputError(ValueError):
    pass


def checked_stat(path: Path):
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or (
        getattr(info, "st_file_attributes", 0)
        & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400)
    ):
        raise InputError("reparse/symlink input refused")
    return info


def build_key(root: Path) -> dict:
    root = Path(os.path.abspath(root))
    # A plain-looking root can itself be beneath a junction. Check ancestors
    # before walking, and every child before following it.
    for ancestor in (root, *root.parents):
        checked_stat(ancestor)
    files = set()

    def checked_input(path: Path):
        # Individual files are not reached through visit's directory walk.
        # Check every component so a tools/ junction cannot alias their parent.
        current = root
        parts = path.relative_to(root).parts
        for index, part in enumerate(parts):
            current = current / part
            info = checked_stat(current)
            if index < len(parts) - 1 and not stat.S_ISDIR(info.st_mode):
                raise InputError("source input parent is not a directory")
        return info

    def visit(path: Path):
        info = checked_stat(path)
        relative = path.relative_to(root).as_posix()
        # Only the actual workspace build output is excluded, not arbitrary
        # source modules named 'target' somewhere beneath a crate.
        if relative == "broker/target":
            if not stat.S_ISDIR(info.st_mode):
                raise InputError("broker/target is not a directory")
            return
        if stat.S_ISDIR(info.st_mode):
            for child in sorted(path.iterdir(), key=lambda p: p.name):
                visit(child)
        elif stat.S_ISREG(info.st_mode):
            files.add(path)
        else:
            raise InputError("non-regular source input refused")

    for name in REQUIRED_FILES:
        try:
            info = checked_input(root / name)
        except OSError as error:
            raise InputError(f"required input unavailable: {name}") from error
        if not stat.S_ISREG(info.st_mode):
            raise InputError(f"required input is not a file: {name}")
    for name in (*INPUT_DIRECTORIES, *INPUT_FILES):
        path = root / name
        try:
            checked_input(path)
        except FileNotFoundError:
            if name in REQUIRED_FILES:
                raise
            continue
        # Only an initially absent optional root can be skipped. Any later
        # disappearance (including the root itself) is an enumeration failure.
        visit(path)

    digest = hashlib.sha256(b"aexcompat-rust-workspace-source-v1\0")
    byte_count = 0
    for path in sorted(files, key=lambda p: p.relative_to(root).as_posix()):
        before = checked_input(path)
        data = path.read_bytes()
        after = checked_input(path)
        if len(data) != before.st_size or (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_size, after.st_mtime_ns, after.st_ctime_ns
        ):
            raise InputError("source input changed while hashing")
        name = path.relative_to(root).as_posix().encode("utf-8")
        digest.update(struct.pack("<Q", len(name)))
        digest.update(name)
        digest.update(struct.pack("<Q", len(data)))
        digest.update(data)
        byte_count += len(data)
    return {"digest": digest.hexdigest(), "files": len(files), "bytes": byte_count}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).absolute().parents[1])
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    try:
        key = build_key(args.root)
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8", newline="\n") as output:
                output.write(f"digest={key['digest']}\n")
    except (OSError, InputError) as error:
        print(f"workspace cache key unavailable: {error}", file=sys.stderr)
        return 2
    print(f"workspace_source_digest={key['digest']}")
    print(f"workspace_source_files={key['files']} workspace_source_bytes={key['bytes']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
