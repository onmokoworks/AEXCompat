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
PRIVATE_EMAIL = re.compile(
    r"(?i)^[^@\r\n]+@(?:[^.@\s]+|[^@\s]+(?:\.tail[0-9a-z]+\.ts\.net|\.local))$"
)
PRIVATE_EMAIL_IN_PAYLOAD = re.compile(
    rb"\b[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
    rb"[A-Za-z0-9.-]+(?:\.tail[0-9a-z]+\.ts\.net|\.local)\b",
    re.I,
)
HIGH_CONFIDENCE_PATH_REPLACEMENTS_TEXT = (
    "regex:(?i)[A-Za-z]:[\\\\/]+Users[\\\\/]+[^\\\\/\\r\\n]+==><redacted-home>\n"
    "regex:/Users/[^/\\r\\n]+==><redacted-home>\n"
    "regex:/home/[^/\\r\\n]+==><redacted-home>\n"
    "regex:/root(?:/[^\\s`\"']*)?==><redacted-home>\n"
    "regex:/(?i:workspace|workspaces|github/workspace|__w)"
    "(?:/[^\\s`\"']*)?==><redacted-workspace>\n"
    "regex:[A-Za-z0-9._-]+\\.tail[0-9a-z]+\\.ts\\.net==><redacted-tailscale-host>\n"
)
PATH_REPLACEMENTS_TEXT = (
    HIGH_CONFIDENCE_PATH_REPLACEMENTS_TEXT
    + "regex:(?i)\\b[A-Za-z]:[\\\\/]+[^\\s`\"']+==><redacted-windows-path>\n"
    "regex:(?i)\\\\{2,}[^\\\\/\\s`\"']+\\\\+[^\\s`\"']+==><redacted-unc-path>\n"
)
PRIVATE_EMAIL_REPLACEMENTS_TEXT = (
    "regex:[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
    "[A-Za-z0-9.-]+(?:\\.tail[0-9a-z]+\\.ts\\.net|\\.local)"
    "==><redacted-private-email>\n"
    "regex:[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@"
    "[A-Za-z0-9_-]+\\b(?!\\.)==><redacted-private-email>\n"
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
INTENTIONAL_SECRET_BLOB_OIDS = {
    "tests/test_public_export.py": frozenset({
        # Historical blobs containing only synthetic scanner fixtures.
        "01febeefd79326a34b8e1d0724bd2f9489010e47",
        "26c779edb7ceec0558280af3dd6a0c30d9cb7230",
        "30b7ecad253a3a6f2a3f60dce889543a1e071c99",
        "7cebcd8eefbd914814c3d128fe49f5a09f929b83",
        "97319ce947875e4b28f5cd413acce163a793619f",
        "cd3e69d575db44223e9d4b98afa0526b25644af6",
        "cec025b719117a1d8f4ff9dc4449d95aac794fd3",
        "dba9f31a1df3884528a5480d94d6daa3e96e8513",
    }),
}
INTENTIONAL_PERSONAL_PATH_DIGESTS = {
    "broker/crates/broker/src/opencl_runtime_probe.rs": frozenset({
        "05e24481d49af5ff4b88980216126fdaa1506fb0e7dcdbc60d64e5bcc5a61312",
        "143b2a35c870c71948c9adb607c02dd5e1a5caba9c7a5f2a0ea3007b3c129856",
        "24dce2d613d2d2e15e2c1f9a4d6a6fb9976deb8856b010785d07c0405df6b899",
        "5f5c467ab82151c736e7d0d9df4d38cfc1f3ee6b29166953003cb187eae4ecd7",
        "79ed539cf15cd04214a808b3af03ae5e3b0799d95f61c8aa5fa2b18cc712abed",
        "b997a2dd8b1cf20e9e641dd92747b02e4823228326229dcb20446e8940b0a9a4",
    }),
    "imports/aviutlas-rust-contracts/aviutl-rs/examples/aex_image_probe.rs": frozenset({
        "895be9833f4d131d0b1b3b817f8990b7c3f386cfe6f05790d5521b855b4d44e2",
        "b89aceec2314b6ec967b354f7561e6a378dc5be81b98f0fa2465bf5e8e6fa5dd",
        "bcf05e6abd327667a16ec643d61b74d68fe2b146b02692ebf774462a7c31c041",
    }),
    "imports/aviutlas-rust-contracts/aviutl-rs/tests/aex_fixture_gate_refresh_audit_contract.rs": frozenset({
        "94f27c71474b819d2cbe28ffd78d6abd44e9b43fc1f0c0431e18d6ae7703b945",
        "a99b652e18df81c2dec2461ad438afca96afbd8f3faec4b129e7f7a1455e4035",
    }),
    "imports/aviutlas-rust-contracts/aviutl-rs/tests/aex_image_probe_contract.rs": frozenset({
        "0852eb49ff4033694fbbb9e0e37ab7ae72d5683bf012f7537d879544e7dc53a4",
        "0b93ed2101f53d543f18ef805b42fac956f87f07e6d8f41b10a3f289e72a376e",
        "119335d2096813a57e28c9996f2ece37350a53efbcddf1c49a4607f3af8d567d",
        "3fe1f8c88bc840738504c51c56929877a9b523bdfe8bc75fbca45baef60b378e",
        "41cc85d81cc165d2fafe4f4c9f9b70ee19a095f940cb59caeb86bb54147ea3de",
        "483a2e30b315176993be09260e1458d0704c4f1cd0de3839ffe0cc65b68c69df",
        "6eb8a75638edd992e3f77d5109d49e0dba54838e3b2b7a04c1659c7b1dadfc5f",
        "8d24447d0091c27473ea8dfba0bb3235a18dacd0708169d744d4adfb3109d5f6",
        "e90a9771e41d705f0f24e7571a5d9480ba99b27d9d5b7ea5dc57c94a814eaeb1",
    }),
    "instruments/common/trace_writer_selftest/main.cpp": frozenset({
        "04c2e02b6905edb3ec210148f64f1b00bfbf06659dbc24ba6a867550e14cb939",
        "23e959658f5082a89c6db72c842271a117887b8658ab703f60eaba650b3d5f20",
        "a4fc1f4893afb162e8829785530ef8dc92e418b047b01e5b67ab67c4a08ce17b",
        "a624dd1252b630f65e7d72b448c5cc93f4beb3afd7865767068f1d569470e484",
        "c4338c5984465bb5c832e43f99618473074c7e1b3163522b75c00ab98b3710ff",
    }),
    "tools/public_export.py": frozenset({
        "09ecaeaa0830c5d76af26bc84dbe37be15fa5e7af890148ef14d666945664b3c",
        "32f58f7dce118a10cf9b323bd23671bd44dfd7d97da484ff0260b7b7d27751e6",
        "dd948f5bacf2a70a419e66d8e879b20e2feb028377f5ea4d7ad81dd9151a240c",
        "ded43b390a96207196c332c53742324550433c9154f9df02dd0d66f7f8f5dc40",
        "5de556e108534f4ea7bdabf9f25adb37f01a114380ad399662c64b31492d0d01",
        "81b7fb1a49849b37cc4f999d28ed103b8cd526a47637fcab339b78a614c617cb",
        "1bffdae4417988548991dba5de1ab86e3063bedcc746ca1f48caeee39de59115",
        "2307dfb012141522a63239579fb4648b52373d42ba7bcd66f9ccff99e59c11ad",
        "5ccd63c1e0fdf546596eb6d6783346d49e96122bf8bd50a7703904b1056e9a3a",
        "ee9b9d34eec0eda397f0c47d1ab42ab9637df3409e8e6a89ce9198c0f885d2b6",
        "78d570b85717bcadb7dbb21a27c673100ab3445e4c1cae5daeb89e9598ba7415",
        "f428e77a5477b1ed434a43767e9e03dad99e399124c20ad4fbbed5d2937804bb",
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
        "548c24abf8eafcac5b60be737744e5e31d7373d11f1629392e0273f6f8ef5ef0",
        "7a52c97c4277dbfa38c6ddf260e56b15b33e869c2892de05f895a462232c2979",
        "88e53e1e1f77b2a7880982e5172fe79269619726b35f1a1fc1960e16e738eb84",
    }),
    "tools/aex_dependency_availability_preflight.py": frozenset({
        "4c754b6dc9cd24a7e1a0801560911fbf9a832bf0f5b3bcba0f3844a71356489c",
    }),
    "tools/aex_native_loader_path_policy_selftest.py": frozenset({
        "0ebc17c6b46a7acf5f8c721666191d070e117c112c21dc5768e8cd4ee290b47e",
    }),
    "tools/conformance_bundle_validator.py": frozenset({
        "2da0adb572c983c5e4451038ea6267e88305bc4875f73684e8372673ab9e8f59",
        "4078491cd748c7038a076acbd60a86e980f15bafb1b8e9b15278db28abbe4996",
    }),
    "tools/issue26_sdk_provenance.py": frozenset({
        "822628af0a69ff78603fc92f95f1532781e09dd2386dc490d5a92c5319de3f62",
    }),
    "tools/run-real-aex-corpus.py": frozenset({
        "2d434b28530bb818d0131735105c0666b8c970221ebd31fcc7563c269098d7cd",
        "2da0adb572c983c5e4451038ea6267e88305bc4875f73684e8372673ab9e8f59",
        "cc2f806732c6860842738f9c74f47c42b9df1c91362d9fce48fae3972ab6a056",
        "ce54bde369789c0ad1944bd8be8b44987d831633dd6593a85877609a4e2ccb9d",
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
        "8d10eace3eede3521e71ac191db8a493c168e213dcadbfa834de34096a072034",
        "ed70ff84cd0007fff92162c8ee27b1bfea04aded2546fe1b5dff5ce6250fd1f7",
    }),
}
INTENTIONAL_PRIVATE_EMAIL_DIGESTS = {
    "tests/test_public_export.py": frozenset({
        "7f7133531186b4111a5064669ffb421e2d454cd91126d30d872f6f04584520a3",
        "8127d9e7a091c9e9f50175214c5aaaec1ed598737170686ef3697455bd5404f1",
        "c01af6ee6d8fa95f05bd62d5e9ce9f5e64009b22132b239e3be6a6de099a7902",
        "60a84811c364b26b46fea3b624091dee6b0bdf86f50a191c371a45de70bc45d2",
        "6e1f34cab96ab431774dc6e2ff80d29063ea80d1f4702121180d3972adf4a4ad",
        "81811e6237d4dbdf4c49b5050a466423c07c52c8bfe0d64ec3ef9f81e9e78958",
    }),
}


def audited_tip_restore_paths() -> tuple[str, ...]:
    """Files whose current synthetic fixtures must survive broad history cleanup."""
    return tuple(sorted(
        INTENTIONAL_SCANNER_FIXTURES
        | INTENTIONAL_SECRET_BLOB_OIDS.keys()
        | INTENTIONAL_PERSONAL_PATH_DIGESTS.keys()
        | INTENTIONAL_PRIVATE_EMAIL_DIGESTS.keys()
    ))
DIAGNOSTIC_SUFFIXES = {".json", ".jsonl", ".log"}
DIAGNOSTIC_PARTS = {"analysis", "corpus", "diagnostics", "results"}
PUBLIC_NOTE_SUFFIXES = {".md", ".rst"}
PATH_BEARING_METADATA = {".gitmodules", ".mailmap", ".gitconfig"}
PATH_BEARING_SOURCE_SUFFIXES = {".cpp", ".csproj", ".jsx", ".rs"}
HIGH_CONFIDENCE_SOURCE_PATH_LABELS = {
    "Windows user path",
    "macOS user path",
    "Linux user path",
    "Linux root path",
    "container workspace path",
    "Tailscale hostname",
}


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
        if candidate.suffix.lower() == ".py" and "tools" in {
            part.lower() for part in candidate.parts
        }:
            return True
        if candidate.suffix.lower() in DIAGNOSTIC_SUFFIXES:
            return True
        if (
            {part.lower() for part in candidate.parts} & DIAGNOSTIC_PARTS
            and candidate.suffix.lower() not in PUBLIC_NOTE_SUFFIXES
        ):
            return True
    return False


def scans_all_personal_paths(kind: str, paths: frozenset[str]) -> bool:
    if kind != "blob":
        return True
    for path in paths:
        candidate = PurePosixPath(path)
        lowered_parts = {part.lower() for part in candidate.parts}
        if candidate.name.lower() in PATH_BEARING_METADATA:
            return True
        if candidate.suffix.lower() == ".csproj":
            return True
        if candidate.suffix.lower() in DIAGNOSTIC_SUFFIXES:
            return True
        if (
            lowered_parts & DIAGNOSTIC_PARTS
            and candidate.suffix.lower() not in PUBLIC_NOTE_SUFFIXES
        ):
            return True
    return False


PERSONAL_PATH_REDACTIONS = {
    "Windows user path": b"<redacted-home>",
    "Windows absolute path": b"<redacted-windows-path>",
    "Windows UNC path": b"<redacted-unc-path>",
    "macOS user path": b"<redacted-home>",
    "Linux user path": b"<redacted-home>",
    "Linux root path": b"<redacted-home>",
    "container workspace path": b"<redacted-workspace>",
    "Tailscale hostname": b"<redacted-tailscale-host>",
}


def redact_personal_paths(payload: bytes) -> bytes:
    for label, pattern in PERSONAL_PATH_PATTERNS.items():
        payload = pattern.sub(PERSONAL_PATH_REDACTIONS[label], payload)
    return payload


def historical_path_cleanup_paths(repository: Path) -> set[str]:
    cleanup: set[str] = set()
    for oid, kind, paths in reachable_objects(repository):
        eligible = {
            path for path in paths
            if scans_all_personal_paths(kind, frozenset({path}))
        }
        if not eligible:
            continue
        payload = run(["git", "cat-file", "blob", oid], cwd=repository, text=False).stdout
        if any(pattern.search(payload) for pattern in PERSONAL_PATH_PATTERNS.values()):
            cleanup.update(eligible)
    return cleanup


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


def names_from_tree(payload: bytes, oid_size: int) -> bytes:
    names: list[bytes] = []
    cursor = 0
    while cursor < len(payload):
        mode_end = payload.find(b" ", cursor)
        name_end = payload.find(b"\0", mode_end + 1)
        if mode_end < 0 or name_end < 0 or name_end + 1 + oid_size > len(payload):
            raise RuntimeError("malformed reachable tree payload")
        names.append(payload[mode_end + 1 : name_end])
        cursor = name_end + 1 + oid_size
    return b"\n".join(names)


def tag_identity(payload: bytes) -> tuple[str, str] | None:
    match = re.search(rb"(?m)^tagger (.*?) <([^<>]+)> [0-9]+ [+-][0-9]{4}$", payload)
    if not match:
        return None
    return (
        match.group(1).decode("utf-8", errors="replace"),
        match.group(2).decode("utf-8", errors="replace"),
    )


def commit_identities(payload: bytes) -> list[tuple[str, str]]:
    identities = []
    for match in re.finditer(
        rb"(?m)^(?:author|committer) (.*?) <([^<>]+)> [0-9]+ [+-][0-9]{4}$",
        payload,
    ):
        identities.append((
            match.group(1).decode("utf-8", errors="replace"),
            match.group(2).decode("utf-8", errors="replace"),
        ))
    return identities


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
            scan_payload = (
                names_from_tree(payload, oid_size)
                if expected_kind == "tree"
                else payload
            )
            metadata_identities = (
                commit_identities(payload)
                if expected_kind == "commit"
                else [identity] if expected_kind == "tag" and (identity := tag_identity(payload))
                else []
            )
            for _name, email in metadata_identities:
                if PRIVATE_EMAIL.search(email):
                    findings.append(
                        f"private identity email in reachable {expected_kind} {expected_oid}"
                    )
            scans_private_emails = (
                expected_oid in symlink_oids
                or scans_personal_paths(expected_kind, object_paths)
            )
            for match in PRIVATE_EMAIL_IN_PAYLOAD.finditer(scan_payload):
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
                for _match in pattern.finditer(scan_payload):
                    if (
                        expected_kind == "blob"
                        and object_paths
                        and all(
                            expected_oid in INTENTIONAL_SECRET_BLOB_OIDS.get(path, ())
                            for path in object_paths
                        )
                    ):
                        continue
                    findings.append(
                        f"{label} candidate in reachable {expected_kind} {expected_oid}"
                    )
            for label, pattern in PERSONAL_PATH_PATTERNS.items():
                if not (
                    expected_oid in symlink_oids
                    or scans_all_personal_paths(expected_kind, object_paths)
                    or (
                        label in HIGH_CONFIDENCE_SOURCE_PATH_LABELS
                        and scans_personal_paths(expected_kind, object_paths)
                    )
                ):
                    continue
                for match in pattern.finditer(scan_payload):
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
    cleanup_paths = historical_path_cleanup_paths(repository)
    audited_tip_files: dict[str, bytes] = {}
    for path in audited_tip_restore_paths():
        result = subprocess.run(
            ["git", "show", f"main:{path}"], cwd=repository, capture_output=True
        )
        if result.returncode == 0:
            audited_tip_files[path] = result.stdout
    sanitized_tip_files: dict[str, bytes] = {}
    for path in sorted(cleanup_paths):
        result = subprocess.run(
            ["git", "show", f"main:{path}"], cwd=repository, capture_output=True
        )
        if result.returncode == 0:
            sanitized_tip_files[path] = redact_personal_paths(result.stdout)
    with tempfile.TemporaryDirectory(prefix="aexcompat-public-export-") as temporary:
        temporary = Path(temporary)
        mailmap = temporary / "mailmap"
        mailmap.write_text(
            "".join(f"{name} <{public_email}> <{email}>\n" for name, email in private_identities),
            encoding="utf-8",
        )
        replacements = temporary / "replacements.txt"
        replacements.write_text(
            PATH_REPLACEMENTS_TEXT + PRIVATE_EMAIL_REPLACEMENTS_TEXT,
            encoding="utf-8",
        )
        source_replacements = temporary / "source-replacements.txt"
        source_replacements.write_text(
            HIGH_CONFIDENCE_PATH_REPLACEMENTS_TEXT
            + "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling.result.json==>target/ae-oracles/"
            "pf-batch-sampling.result.json\n"
            "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling-run.result.json==>target/ae-oracles/"
            "pf-batch-sampling-run.result.json\n"
            "D:/Projects/01_Project/04_Tools/AEXCompat/target/ae-oracles/"
            "pf-batch-sampling.png==>target/ae-oracles/pf-batch-sampling.png\n"
            "D:\\Projects\\01_Project\\04_Tools\\Ae_Plugins==>target/local-aex\n"
            "D:/Projects/01_Project/04_Tools/WizTree MCP/exports"
            "==>external WizTree export directory\n"
            "D:\\Projects\\01_Project\\04_Tools==>external project directory\n"
            r"D:\\Projects\\01_Project\\04_Tools\\AEXCompat"
            "==><redacted-windows-path>\n"
            "H:\\04_software\\YukkuriMovieMaker_v4_Lite\\"
            "==>$(AEXCOMPAT_YMM4_DIR)\n",
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
        for path in sorted(INTENTIONAL_SCANNER_FIXTURES | cleanup_paths):
            command.extend(["--path", path])
        command.extend(["--invert-paths", "--refs", *refs])
        subprocess.run(command, cwd=repository, check=True)
    for path, contents in audited_tip_files.items():
        destination = repository / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)
    for path, contents in sanitized_tip_files.items():
        destination = repository / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)
    restored_paths = sorted(audited_tip_files.keys() | sanitized_tip_files.keys())
    if restored_paths:
        run(["git", "add", "--", *restored_paths], cwd=repository)
        changed = subprocess.run(
            ["git", "diff", "--cached", "--quiet"], cwd=repository
        ).returncode
        if changed:
            run(["git", "config", "user.name", "AEXCompat public export"], cwd=repository)
            run(["git", "config", "user.email", public_email], cwd=repository)
            run(
                ["git", "commit", "-m", "Restore audited public test fixtures"],
                cwd=repository,
            )


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
