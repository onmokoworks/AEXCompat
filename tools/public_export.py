#!/usr/bin/env python3
"""Create and verify a disposable, sanitized public-export repository.

This tool never pushes. It clones only ``main`` plus tags named explicitly by
the caller, rewrites private host-derived author/committer email addresses in
the disposable clone, removes prohibited binary payloads from reachable
history, and scans the resulting history before reporting success.
"""

from __future__ import annotations

import argparse
import hashlib
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
PRIVATE_EMAIL_IN_PAYLOAD = re.compile(
    rb"\b[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
    rb"[A-Za-z0-9.-]+(?:\.tail[0-9a-z]+\.ts\.net|\.local)\b",
    re.I,
)
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
    "container workspace path": re.compile(
        b"/" + rb"(?:workspace|workspaces|github/workspace|__w)(?:/[^\s`\"']*)?",
        re.I,
    ),
    "Tailscale hostname": re.compile(rb"\b[A-Za-z0-9._-]+\.tail[0-9a-z]+\.ts\.net\b", re.I),
}
INTENTIONAL_SCANNER_FIXTURES = {
    "tests/test_public_export.py",
    "tools/public_export.py",
}
INTENTIONAL_PERSONAL_PATH_DIGESTS = {
    "instruments/common/trace_writer_selftest/main.cpp": frozenset({
        "a4fc1f4893afb162e8829785530ef8dc92e418b047b01e5b67ab67c4a08ce17b",
    }),
    "tools/public_export.py": frozenset({
        "1bffdae4417988548991dba5de1ab86e3063bedcc746ca1f48caeee39de59115",
        "2307dfb012141522a63239579fb4648b52373d42ba7bcd66f9ccff99e59c11ad",
        "5ccd63c1e0fdf546596eb6d6783346d49e96122bf8bd50a7703904b1056e9a3a",
        "ee9b9d34eec0eda397f0c47d1ab42ab9637df3409e8e6a89ce9198c0f885d2b6",
        "5cb8a96e2bfaae670922fc77d3fbd574176382b68e5ce07d694f776ef9f34586",
        "748067ba9c4bac007f896566966e8b7bfee338039fe93fcf9756f8dabb0dee2c",
        "94a6b447580330f9f2b609422537b04239ff3a39df9137e32efd559f1a2935cb",
        "9c7fe4a6f0da32a1a464d6156eacb0ce18eb04f2ce135b65f97905ee14f3b4b9",
        "9da1eb87453bacd92d2d9927874a40f7432c59d11604fdba341dbda5249f0832",
        "a541acfac2e649581bca0cd03e4c3dcf306344915017d9cd6be4ad57a8fd7379",
        "b5e939a75e230250eaa68340f0e0625f431d7ca981b4bb1772f36d06a583390c",
        "b9f44d4003eb7edaa205eeed6549e5b1e7acda967aebcef8c689d6762a785524",
        "c428097596980bfa629b48ad9cfbc6afa3327c8dcdd0946de95e837b176721eb",
        "c52ddf65534b7b46035084358ab7902be4bfef220bdb503ac7039cc861905b05",
        "cd4db80677bd42c006117a8326ea55fcb03c39acc98087d16a2b2b8c5d103fc3",
        "d2f86015d0c19ab337eaa9ad0986f63b10000593d7b5fc812653036a0350c5de",
    }),
    "tests/test_public_export.py": frozenset({
        "018e3b3f571bf13a444cf7ba3688b3e48732883cbe63cbe790ebe13adc55345b",
        "03b873626405418d0cd6180769893aa897a33193672c9ff9a526ef148fe6884e",
        "05a54a7be8f16be6296824e1545aa940cf15facbc8d7647621212574f1badcfe",
        "23e126580f9354884ade6b7f0956e5460b67dc4879f7497d0a5e0ae46907a819",
        "5792ac3ff48febf1d72854e288c93d8495719e87a901882e2c0cb63afddef210",
        "5c5ca155691b67b3645774d9cdf5fe82c2baae9f7739f3c4ad8c74f96bf0c5cd",
        "612b6fc44e3094a36043870b929ac5fb00daf8513229001e92a639fb827cac26",
        "631b8542bef006819290e438e507f7c7a1033c144fe2de0ec246c01ae980efac",
        "685fa6b48149400b29410c53a375700d13d0c045f5de94ae847a48b97f0ff9b8",
        "6fc53fb8381ffd738d7728aeb9cfcabdb7cd11c48e09628d3f0fdbf5c151b0b6",
        "767367c57e2a600826e0237872a8c29ecc22504170d60860bf3ff06fc902d86c",
        "9b2de6e64bf69ea17928b77c3ea82fbde120e771abd498cd7263ff35f2c33711",
        "9f99ec4dad6503d1114aaec6dc8cef3f8f35d6b15ecf66e21c84b544b5ca11f6",
        "a28b1fb160fdfe1f57631bf998ff974912ac8d9b8c4cc666d288ecb3f9ee3195",
        "b9345075b3f6a0fd093a3fa878e5a9d71ab6f1ed636b51dfdecc5e89337dcbe2",
        "d31a0d00d7f0c994da66352dcec58e110f1a3217ecf9ffe033853c5ad940ddb7",
        "e79c60e7d95fd88c1508f05bd1d846411c682d2ee393b8a79cd3218509ad37bb",
        "e922f64b0c068649adcefe4516ee40e86a4f41773fca9813e727bf5dbbfa36c6",
        "edfd8aa05aef948c7a2c18312d7d27a5f8405117d84a01420dd5703f00e4fa12",
    }),
}
INTENTIONAL_PRIVATE_EMAIL_DIGESTS = {
    "tests/test_public_export.py": frozenset({
        "7f7133531186b4111a5064669ffb421e2d454cd91126d30d872f6f04584520a3",
        "8127d9e7a091c9e9f50175214c5aaaec1ed598737170686ef3697455bd5404f1",
        "c01af6ee6d8fa95f05bd62d5e9ce9f5e64009b22132b239e3be6a6de099a7902",
    }),
}
DIAGNOSTIC_SUFFIXES = {".json", ".jsonl", ".log"}
DIAGNOSTIC_PARTS = {"analysis", "corpus", "diagnostics", "results"}
PATH_BEARING_METADATA = {".gitmodules", ".mailmap", ".gitconfig"}
PATH_BEARING_SOURCE_SUFFIXES = {".cpp", ".jsx"}


def scans_personal_paths(kind: str, paths: frozenset[str]) -> bool:
    if kind != "blob":
        return True
    for path in paths:
        if path in INTENTIONAL_SCANNER_FIXTURES:
            return True
        candidate = PurePosixPath(path)
        if candidate.name.lower() in PATH_BEARING_METADATA:
            return True
        if candidate.suffix.lower() in PATH_BEARING_SOURCE_SUFFIXES:
            return True
        if candidate.suffix.lower() in DIAGNOSTIC_SUFFIXES:
            return True
        if {part.lower() for part in candidate.parts} & DIAGNOSTIC_PARTS:
            return True
    return False


def symlink_oids_from_tree(payload: bytes, oid_size: int) -> set[str]:
    symlinks: set[str] = set()
    cursor = 0
    while cursor < len(payload):
        mode_end = payload.find(b" ", cursor)
        name_end = payload.find(b"\0", mode_end + 1)
        if mode_end < 0 or name_end < 0 or name_end + 1 + oid_size > len(payload):
            raise RuntimeError("malformed reachable tree payload")
        mode = payload[cursor:mode_end]
        oid = payload[name_end + 1 : name_end + 1 + oid_size]
        if mode == b"120000":
            symlinks.add(oid.hex())
        cursor = name_end + 1 + oid_size
    return symlinks


def tag_identity(payload: bytes) -> tuple[str, str] | None:
    match = re.search(rb"(?m)^tagger (.*?) <([^<>]+)> [0-9]+ [+-][0-9]{4}$", payload)
    if not match:
        return None
    return (
        match.group(1).decode("utf-8", errors="replace"),
        match.group(2).decode("utf-8", errors="replace"),
    )


def reachable_tag_identities(repository: Path) -> list[tuple[str, str]]:
    identities: list[tuple[str, str]] = []
    tag_oids = [oid for oid, kind, _paths in reachable_objects(repository) if kind == "tag"]
    if not tag_oids:
        return identities
    batch = subprocess.Popen(
        ["git", "cat-file", "--batch"],
        cwd=repository,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert batch.stdin is not None and batch.stdout is not None
    for expected_oid in tag_oids:
        batch.stdin.write(f"{expected_oid}\n".encode("ascii"))
        batch.stdin.flush()
        header = batch.stdout.readline().decode("ascii").strip().split()
        if len(header) != 3 or header[:2] != [expected_oid, "tag"]:
            raise RuntimeError(f"unexpected git cat-file header: {header}")
        size = int(header[2])
        retained_size = min(size, 64 * 1024)
        header_payload = read_exact(batch.stdout, retained_size)
        discard_exact(batch.stdout, size - retained_size)
        if batch.stdout.read(1) != b"\n":
            raise RuntimeError("git cat-file batch framing error")
        identity = tag_identity(header_payload)
        if identity is not None:
            identities.append(identity)
    batch.stdin.close()
    stderr = batch.stderr.read() if batch.stderr is not None else b""
    if batch.wait() != 0:
        raise RuntimeError(
            "git cat-file failed: " + stderr.decode("utf-8", errors="replace")
        )
    return identities


def run(argv: list[str], *, cwd: Path | None = None, text: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(argv, cwd=cwd, check=True, capture_output=True, text=text)


def validate_tag(tag: str) -> str:
    if (
        not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", tag)
        or ".." in tag
        or tag.startswith("refs/")
    ):
        raise ValueError(f"invalid tag name: {tag!r}")
    encoded = tag.encode("utf-8")
    for label, pattern in {**SECRET_PATTERNS, **PERSONAL_PATH_PATTERNS}.items():
        if pattern.search(encoded):
            raise ValueError(f"unsafe tag name ({label}): {tag!r}")
    return tag


def prohibited_path(path: str) -> bool:
    candidate = PurePosixPath(path)
    lowered = {part.lower() for part in candidate.parts}
    return candidate.suffix.lower() in PROHIBITED_SUFFIXES or bool(lowered & PROHIBITED_PARTS)


def reachable_objects(repository: Path) -> list[tuple[str, str, frozenset[str]]]:
    paths_by_oid: dict[str, set[str]] = {}
    object_ids = sorted(set(run(
        ["git", "rev-list", "--objects", "--all", "--no-object-names"],
        cwd=repository,
    ).stdout.splitlines()))
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
    kinds = dict(line.split(" ", 1) for line in proc.stdout.splitlines())
    changes = run(
        [
            "git", "log", "--all", "-m", "--raw", "-z", "--format=",
            "--no-renames", "--no-abbrev", "--root",
        ],
        cwd=repository,
        text=False,
    ).stdout.split(b"\0")
    for index in range(0, len(changes) - 1, 2):
        metadata = changes[index].lstrip(b"\n")
        path = changes[index + 1]
        if not metadata or not path:
            continue
        _old_mode, _new_mode, old_oid, new_oid, _status = metadata[1:].split(b" ", 4)
        decoded_path = path.decode("utf-8", errors="surrogateescape")
        for oid in (old_oid.decode("ascii"), new_oid.decode("ascii")):
            if set(oid) != {"0"}:
                paths_by_oid.setdefault(oid, set()).add(decoded_path)
    return [(oid, kinds[oid], frozenset(paths_by_oid.get(oid, set()))) for oid in object_ids]


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
    for ref in run(
        ["git", "for-each-ref", "--format=%(refname)"], cwd=repository
    ).stdout.splitlines():
        encoded_ref = ref.encode("utf-8")
        for label, pattern in {**SECRET_PATTERNS, **PERSONAL_PATH_PATTERNS}.items():
            if pattern.search(encoded_ref):
                findings.append(f"{label} in reachable ref {ref}")
    reachable = reachable_objects(repository)
    paths = {path for _oid, _kind, object_paths in reachable for path in object_paths}
    for path in sorted({path for path in paths if path and prohibited_path(path)}):
        findings.append(f"prohibited historical path: {path}")

    scanned_objects = [
        (oid, kind, paths)
        for oid, kind, paths in reachable
        if kind in {"blob", "commit", "tag", "tree"}
    ]
    scanned_objects.sort(key=lambda item: item[1] != "tree")
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
        object_format = run(
            ["git", "rev-parse", "--show-object-format"], cwd=repository
        ).stdout.strip()
        oid_size = 20 if object_format == "sha1" else 32
        symlink_oids: set[str] = set()
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
            if expected_kind == "tree":
                symlink_oids.update(symlink_oids_from_tree(payload, oid_size))
            if expected_kind == "tag":
                identity = tag_identity(payload)
                if identity is not None and PRIVATE_EMAIL.search(identity[1]):
                    findings.append(
                        f"private tagger email in reachable tag {expected_oid}"
                    )
            scans_private_emails = (
                expected_oid in symlink_oids
                or scans_personal_paths(expected_kind, object_paths)
            )
            for match in PRIVATE_EMAIL_IN_PAYLOAD.finditer(payload):
                if not scans_private_emails:
                    break
                if (
                    expected_kind == "blob"
                    and object_paths
                    and all(
                        hashlib.sha256(match.group(0)).hexdigest()
                        in INTENTIONAL_PRIVATE_EMAIL_DIGESTS.get(path, ())
                        for path in object_paths
                    )
                ):
                    continue
                findings.append(
                    f"private email in reachable {expected_kind} {expected_oid}"
                )
            for label, pattern in SECRET_PATTERNS.items():
                if pattern.search(payload):
                    findings.append(
                        f"{label} candidate in reachable {expected_kind} {expected_oid}"
                    )
            for label, pattern in PERSONAL_PATH_PATTERNS.items():
                if not (
                    expected_oid in symlink_oids
                    or scans_personal_paths(expected_kind, object_paths)
                ):
                    continue
                for match in pattern.finditer(payload):
                    if (
                        expected_kind == "blob"
                        and object_paths
                        and all(
                            hashlib.sha256(match.group(0)).hexdigest()
                            in INTENTIONAL_PERSONAL_PATH_DIGESTS.get(path, ())
                            for path in object_paths
                        )
                    ):
                        continue
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
    identities.extend(f"{name}\0{email}" for name, email in reachable_tag_identities(repository))
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
            "regex:/(?i:workspace|workspaces|github/workspace|__w)"
            "(?:/[^\\s`\"']*)?==><redacted-workspace>\n"
            "regex:[A-Za-z0-9._-]+\\.tail[0-9a-z]+\\.ts\\.net==><redacted-tailscale-host>\n"
            "regex:[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
            "[A-Za-z0-9.-]+(?:\\.tail[0-9a-z]+\\.ts\\.net|\\.local)"
            "==><redacted-private-email>\n",
            encoding="utf-8",
        )
        source_replacements = temporary / "source-replacements.txt"
        source_replacements.write_text(
            "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling.result.json==>target/ae-oracles/"
            "pf-batch-sampling.result.json\n"
            "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling-run.result.json==>target/ae-oracles/"
            "pf-batch-sampling-run.result.json\n"
            "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling.png==>target/ae-oracles/pf-batch-sampling.png\n",
            encoding="utf-8",
        )
        command = [
            sys.executable, "-m", "git_filter_repo", "--force",
            "--mailmap", str(mailmap), "--replace-message", str(replacements),
            "--replace-text", str(source_replacements),
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
