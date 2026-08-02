#!/usr/bin/env python3
"""Create and verify a disposable, sanitized public-export repository.

This tool never pushes. It clones only ``main`` plus tags named explicitly by
the caller, rewrites private host-derived author/committer email addresses in
the disposable clone, removes prohibited binary payloads from reachable
history, and scans the resulting history before reporting success.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath


PROHIBITED_SUFFIXES = {
    ".aex", ".dll", ".dmp", ".dump", ".pdb", ".lib", ".obj",
    ".exe", ".zip", ".7z", ".rar", ".aep", ".psd", ".pyc", ".pyo",
}
PROHIBITED_PARTS = {"private", "proprietary", "adobe-sdk", "after-effects-sdk"}
SECRET_PATTERNS = {
    # Split the marker so the scanner's own source is not a finding.
    "private key": re.compile(
        b"-----BEGIN " + rb"(?:(?:RSA|EC|DSA|OPENSSH|ENCRYPTED) )?PRIVATE KEY-----"
    ),
    "GitHub token": re.compile(
        rb"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b"
    ),
    "AWS access key": re.compile(rb"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
    "Slack token": re.compile(rb"\bxox[baprs]-[A-Za-z0-9-]{10,}\b"),
}
PRIVATE_EMAIL = re.compile(r"(?i)(?:\.tail[0-9a-z]+\.ts\.net|\.local)$")
PERSONAL_PATH_PATTERNS = {
    "Windows user path": re.compile(
        rb"\b[A-Za-z]:[\\/]+Users[\\/]+[^\\/\r\n]+", re.I
    ),
    "Windows absolute path": re.compile(
        rb"\b[A-Za-z]:[\\/]+[^\s`\"']+", re.I
    ),
    "Windows UNC path": re.compile(
        rb"\\{2,}[^\\/\s`\"']+\\+[^\s`\"']+", re.I
    ),
    "macOS user path": re.compile(b"/" + rb"Users/[^/\r\n]+"),
    "Linux user path": re.compile(b"/" + rb"home/[^/\r\n]+"),
    "Linux root path": re.compile(b"/" + rb"root(?:/[^\s`\"']*)?"),
    "Tailscale hostname": re.compile(rb"\b[A-Za-z0-9._-]+\.tail[0-9a-z]+\.ts\.net\b", re.I),
}
INTENTIONAL_SCANNER_FIXTURES = {
    "tests/test_public_export.py",
    "tools/public_export.py",
}


def run(argv: list[str], *, cwd: Path | None = None, text: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, check=True, capture_output=True, text=text)


def validate_tag(tag: str) -> str:
    if (
        not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", tag)
        or ".." in tag
        or tag.startswith("refs/")
    ):
        raise ValueError(f"invalid tag name: {tag!r}")
    return tag


def prohibited_path(path: str) -> bool:
    candidate = PurePosixPath(path)
    lowered = {part.lower() for part in candidate.parts}
    return candidate.suffix.lower() in PROHIBITED_SUFFIXES or bool(lowered & PROHIBITED_PARTS)


def reachable_objects(repository: Path) -> list[tuple[str, str, frozenset[str]]]:
    paths_by_oid: dict[str, set[str]] = {}
    for line in run(["git", "rev-list", "--objects", "--all"], cwd=repository).stdout.splitlines():
        oid, *path = line.split(" ", 1)
        paths_by_oid.setdefault(oid, set()).update(path)
    object_ids = sorted(paths_by_oid)
    if not object_ids:
        return []
    request = "".join(f"{oid}\n" for oid in object_ids)
    proc = subprocess.run(
        ["git", "cat-file", "--batch-check=%(objectname) %(objecttype)"],
        cwd=repository,
        input=request,
        check=True,
        capture_output=True,
        text=True,
    )
    return [
        (oid, kind, frozenset(paths_by_oid[oid]))
        for oid, kind in (line.split(" ", 1) for line in proc.stdout.splitlines())
    ]


def validate_selected_tags(repository: Path, tags: list[str]) -> None:
    for tag in tags:
        try:
            run(
                ["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"],
                cwd=repository,
            )
        except subprocess.CalledProcessError as error:
            raise ValueError(f"selected tag does not resolve to a commit: {tag}") from error


def read_exact(stream, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining:
        chunk = stream.read(min(64 * 1024, remaining))
        if not chunk:
            raise RuntimeError("truncated git cat-file payload")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def discard_exact(stream, size: int) -> None:
    remaining = size
    while remaining:
        chunk = stream.read(min(64 * 1024, remaining))
        if not chunk:
            raise RuntimeError("truncated git cat-file payload")
        remaining -= len(chunk)


def scan_export(repository: Path) -> list[str]:
    findings: list[str] = []
    paths = run(
        ["git", "log", "--all", "-m", "--pretty=format:", "--name-only"], cwd=repository
    ).stdout.splitlines()
    for path in sorted({path for path in paths if path and prohibited_path(path)}):
        findings.append(f"prohibited historical path: {path}")

    scanned_objects = [
        (oid, kind, paths)
        for oid, kind, paths in reachable_objects(repository)
        if kind in {"blob", "commit", "tag", "tree"}
    ]
    if scanned_objects:
        # One batch process is material on Windows: a large history can contain
        # tens of thousands of blobs, and spawning twice per blob made the
        # post-export gate take longer than ten minutes.
        batch = subprocess.Popen(
            ["git", "cat-file", "--batch"],
            cwd=repository,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        assert batch.stdin is not None and batch.stdout is not None
        for expected_oid, expected_kind, object_paths in scanned_objects:
            batch.stdin.write(f"{expected_oid}\n".encode("ascii"))
            batch.stdin.flush()
            header = batch.stdout.readline().decode("ascii").strip().split()
            if (
                len(header) != 3
                or header[0] != expected_oid
                or header[1] != expected_kind
            ):
                raise RuntimeError(f"unexpected git cat-file header: {header}")
            size = int(header[2])
            if size > 8 * 1024 * 1024:
                discard_exact(batch.stdout, size)
                if batch.stdout.read(1) != b"\n":
                    raise RuntimeError("git cat-file batch framing error")
                findings.append(
                    f"oversized reachable {expected_kind}: {expected_oid} ({size} bytes)"
                )
                continue
            payload = read_exact(batch.stdout, size)
            if batch.stdout.read(1) != b"\n":
                raise RuntimeError("git cat-file batch framing error")
            for label, pattern in SECRET_PATTERNS.items():
                if pattern.search(payload) and not (
                    expected_kind == "blob"
                    and object_paths
                    and object_paths <= INTENTIONAL_SCANNER_FIXTURES
                ):
                    findings.append(
                        f"{label} candidate in reachable {expected_kind} {expected_oid}"
                    )
            for label, pattern in PERSONAL_PATH_PATTERNS.items():
                if pattern.search(payload) and not (
                    expected_kind == "blob"
                    and object_paths
                    and object_paths <= INTENTIONAL_SCANNER_FIXTURES
                ):
                    findings.append(
                        f"{label} in reachable {expected_kind} {expected_oid}"
                    )
        batch.stdin.close()
        stderr = batch.stderr.read() if batch.stderr is not None else b""
        if batch.wait() != 0:
            raise RuntimeError(
                "git cat-file failed: " + stderr.decode("utf-8", errors="replace")
            )
    return findings


def rewrite_export(repository: Path, public_email: str, tags: list[str]) -> None:
    refs = ["main", *(f"refs/tags/{tag}" for tag in tags)]
    identities = run(
        ["git", "log", "--all", "--format=%an%x00%ae%n%cn%x00%ce"], cwd=repository
    ).stdout.splitlines()
    identities.extend(run(
        ["git", "for-each-ref", "--format=%(taggername)%00%(taggeremail)", "refs/tags"],
        cwd=repository,
    ).stdout.splitlines())
    private_identities = sorted({
        (line.split("\0", 1)[0], line.split("\0", 1)[1].strip("<>"))
        for line in identities
        if "\0" in line
        and PRIVATE_EMAIL.search(line.split("\0", 1)[1].strip("<>"))
    })
    with tempfile.TemporaryDirectory(prefix="aexcompat-public-export-") as temporary:
        temporary = Path(temporary)
        mailmap = temporary / "mailmap"
        mailmap.write_text(
            "".join(f"{name} <{public_email}> <{email}>\n" for name, email in private_identities),
            encoding="utf-8",
        )
        replacements = temporary / "replacements.txt"
        replacements.write_text(
            "regex:(?i)[A-Za-z]:[\\\\/]+Users[\\\\/]+[^\\\\/\\r\\n]+==><redacted-home>\n"
            "regex:(?i)\\b[A-Za-z]:[\\\\/]+[^\\s`\"']+==><redacted-windows-path>\n"
            "regex:(?i)\\\\{2,}[^\\\\/\\s`\"']+\\\\+[^\\s`\"']+==><redacted-unc-path>\n"
            "regex:/Users/[^/\\r\\n]+==><redacted-home>\n"
            "regex:/home/[^/\\r\\n]+==><redacted-home>\n"
            "regex:/root(?:/[^\\s`\"']*)?==><redacted-home>\n"
            "regex:[A-Za-z0-9._-]+\\.tail[0-9a-z]+\\.ts\\.net==><redacted-tailscale-host>\n",
            encoding="utf-8",
        )
        command = [
            sys.executable, "-m", "git_filter_repo", "--force",
            "--mailmap", str(mailmap), "--replace-message", str(replacements),
        ]
        for suffix in sorted(PROHIBITED_SUFFIXES):
            command.extend(["--path-glob", f"*{suffix}"])
        for part in sorted(PROHIBITED_PARTS):
            command.extend(["--path-glob", f"{part}/*"])
            command.extend(["--path-glob", f"*/{part}/*"])
        command.extend(["--invert-paths", "--refs", *refs])
        subprocess.run(command, cwd=repository, check=True)


def create_export(source: Path, output: Path, tags: list[str], public_email: str) -> None:
    source = source.resolve()
    output = output.resolve()
    if output.exists():
        raise FileExistsError(f"output must not exist: {output}")
    if not (source / ".git").exists():
        raise ValueError(f"source is not a working-tree Git repository: {source}")
    tags = [validate_tag(tag) for tag in tags]

    run([
        "git", "clone", "--no-local", "--no-tags", "--single-branch",
        "--branch", "main", str(source), str(output),
    ])
    try:
        for tag in tags:
            run([
                "git", "fetch", "origin", f"refs/tags/{tag}:refs/tags/{tag}"
            ], cwd=output)
        validate_selected_tags(output, tags)
        run(["git", "remote", "remove", "origin"], cwd=output)
        # Git for Windows can leave this symbolic ref pointing at a deleted
        # remote ref. for-each-ref silently omits the broken ref, but fsck does
        # not; delete it explicitly before rewriting.
        subprocess.run(
            ["git", "symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
            cwd=output,
            capture_output=True,
        )
        rewrite_export(output, public_email, tags)

        actual_refs = set(run(
            ["git", "for-each-ref", "--format=%(refname)", "refs/heads", "refs/tags", "refs/remotes"],
            cwd=output,
        ).stdout.splitlines())
        expected_refs = {"refs/heads/main", *(f"refs/tags/{tag}" for tag in tags)}
        if actual_refs != expected_refs:
            raise RuntimeError(f"unexpected export refs: {sorted(actual_refs ^ expected_refs)}")

        private_emails = [
            email for email in run(["git", "log", "--all", "--format=%ae%n%ce"], cwd=output).stdout.splitlines()
            if PRIVATE_EMAIL.search(email)
        ]
        private_emails.extend(
            email.strip("<>")
            for email in run(
                ["git", "for-each-ref", "--format=%(taggeremail)", "refs/tags"],
                cwd=output,
            ).stdout.splitlines()
            if PRIVATE_EMAIL.search(email.strip("<>"))
        )
        if private_emails:
            raise RuntimeError("private host-derived author email remains after rewrite")
        findings = scan_export(output)
        if findings:
            raise RuntimeError("post-export scan failed:\n- " + "\n- ".join(findings))
        run(["git", "fsck", "--full", "--no-reflogs", "--no-dangling"], cwd=output)
    except Exception:
        print(f"export retained for inspection: {output}", file=sys.stderr)
        raise


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True, help="fresh private working-tree clone")
    parser.add_argument("--out", type=Path, required=True, help="create-new disposable export directory")
    parser.add_argument("--tag", action="append", default=[], help="tag to include; repeat explicitly")
    parser.add_argument(
        "--public-email", default="onmokoworks@users.noreply.github.com",
        help="replacement for .local and Tailscale host-derived emails",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    create_export(args.source, args.out, args.tag, args.public_email)
    print(f"verified export: {args.out.resolve()}")
    print("No push was performed. Review this disposable repository manually.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
