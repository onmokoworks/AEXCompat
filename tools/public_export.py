#!/usr/bin/env python3
"""Create and verify a disposable, sanitized public-export repository.

This tool never pushes. It clones only ``main`` plus tags named explicitly by
the caller, rewrites private host-derived author/committer email addresses in
the disposable clone, removes prohibited binary payloads from reachable
history, and scans the resulting history before reporting success.
"""

from __future__ import annotations

import argparse
import io
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
    "private key": re.compile(b"-----BEGIN " + rb"(?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    "GitHub token": re.compile(rb"\b(?:ghp|github_pat)_[A-Za-z0-9_]{20,}\b"),
    "AWS access key": re.compile(rb"\bAKIA[0-9A-Z]{16}\b"),
    "Slack token": re.compile(rb"\bxox[baprs]-[A-Za-z0-9-]{10,}\b"),
}
PRIVATE_EMAIL = re.compile(r"(?i)(?:\.tail[0-9a-z]+\.ts\.net|\.local)$")
PERSONAL_PATH_PATTERNS = {
    "Windows user path": re.compile(rb"\b[A-Za-z]:\\+Users\\+[^\\/\r\n]+"),
    "macOS user path": re.compile(b"/" + rb"Users/[^/\r\n]+"),
    "Tailscale hostname": re.compile(rb"\b[A-Za-z0-9._-]+\.tail[0-9a-z]+\.ts\.net\b", re.I),
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


def reachable_objects(repository: Path) -> list[tuple[str, str]]:
    output = run(
        ["git", "rev-list", "--objects", "--no-object-names", "--all"], cwd=repository
    ).stdout.splitlines()
    object_ids = sorted(set(output))
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
    return [tuple(line.split(" ", 1)) for line in proc.stdout.splitlines()]


def scan_export(repository: Path) -> list[str]:
    findings: list[str] = []
    paths = run(
        ["git", "log", "--all", "--pretty=format:", "--name-only"], cwd=repository
    ).stdout.splitlines()
    for path in sorted({path for path in paths if path and prohibited_path(path)}):
        findings.append(f"prohibited historical path: {path}")

    scanned_objects = [
        (oid, kind)
        for oid, kind in reachable_objects(repository)
        if kind in {"blob", "commit", "tag"}
    ]
    if scanned_objects:
        # One batch process is material on Windows: a large history can contain
        # tens of thousands of blobs, and spawning twice per blob made the
        # post-export gate take longer than ten minutes.
        batch = subprocess.run(
            ["git", "cat-file", "--batch"],
            cwd=repository,
            input="".join(f"{oid}\n" for oid, _ in scanned_objects).encode("ascii"),
            check=True,
            capture_output=True,
        )
        stream = io.BytesIO(batch.stdout)
        for expected_oid, expected_kind in scanned_objects:
            header = stream.readline().decode("ascii").strip().split()
            if (
                len(header) != 3
                or header[0] != expected_oid
                or header[1] != expected_kind
            ):
                raise RuntimeError(f"unexpected git cat-file header: {header}")
            size = int(header[2])
            payload = stream.read(size)
            if stream.read(1) != b"\n":
                raise RuntimeError("git cat-file batch framing error")
            if expected_kind == "blob" and size > 8 * 1024 * 1024:
                findings.append(f"oversized reachable blob: {expected_oid} ({size} bytes)")
                continue
            for label, pattern in SECRET_PATTERNS.items():
                if pattern.search(payload):
                    findings.append(
                        f"{label} candidate in reachable {expected_kind} {expected_oid}"
                    )
            for label, pattern in PERSONAL_PATH_PATTERNS.items():
                if pattern.search(payload):
                    findings.append(
                        f"{label} in reachable {expected_kind} {expected_oid}"
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
            "regex:[A-Za-z]:\\\\+Users\\\\+[^\\\\/\\r\\n]+==><redacted-home>\n"
            "regex:/Users/[^/\\r\\n]+==><redacted-home>\n"
            "regex:[A-Za-z0-9._-]+\\.tail[0-9a-z]+\\.ts\\.net==><redacted-tailscale-host>\n",
            encoding="utf-8",
        )
        command = [
            sys.executable, "-m", "git_filter_repo", "--force",
            "--mailmap", str(mailmap), "--replace-text", str(replacements),
        ]
        for suffix in sorted(PROHIBITED_SUFFIXES):
            command.extend(["--path-glob", f"*{suffix}"])
        for part in sorted(PROHIBITED_PARTS):
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
