"""Generate a single-entry, schema-v1 allowlist for the L1 load probe.

The generated receipt is intentionally machine-bound: the plugin path is an
absolute path to the locally provisioned fixture.  Runtime loading remains
fail-closed in the broker; this tool only makes the existing L1 fixture
approval reproducible on a developer machine.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from datetime import datetime
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
ALLOWLIST_ROOT = (REPOSITORY_ROOT / "target" / "l1-allowlist").resolve()
SAFE_TOKEN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")


def _within(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def _safe_token(value: str, label: str) -> str:
    if not SAFE_TOKEN.fullmatch(value):
        raise ValueError(f"{label} must contain only letters, digits, '.', '_' or '-'")
    return value


def _resolve_plugin(raw_path: str) -> Path:
    path = Path(raw_path).expanduser().resolve(strict=True)
    if not path.is_file():
        raise ValueError("plugin path must name a regular file")
    if path.suffix.lower() != ".aex":
        raise ValueError("plugin path must have an .aex extension")
    return path


def _resolve_output(raw_path: str) -> Path:
    ALLOWLIST_ROOT.mkdir(parents=True, exist_ok=True)
    candidate = Path(raw_path).expanduser()
    if not candidate.is_absolute():
        candidate = REPOSITORY_ROOT / candidate
    candidate = candidate.resolve(strict=False)
    if candidate == ALLOWLIST_ROOT or not _within(candidate, ALLOWLIST_ROOT):
        raise ValueError("output must stay under repository target/l1-allowlist")
    if candidate.suffix.lower() != ".json":
        raise ValueError("output must have a .json extension")
    candidate.parent.mkdir(parents=True, exist_ok=True)
    return candidate


def _parse_expiry(value: str) -> str:
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError as error:
        raise ValueError("expires must be an ISO-8601 timestamp") from error
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ValueError("expires must include a timezone offset")
    return value


def _sha256_and_size(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest().upper(), size


def build_allowlist(
    *, plugin_id: str, plugin_path: str, receipt_id: str, expires: str, timeout_ms: int
) -> dict[str, object]:
    plugin_id = _safe_token(plugin_id, "plugin-id")
    receipt_id = _safe_token(receipt_id, "receipt-id")
    if timeout_ms <= 0 or timeout_ms > 30_000:
        raise ValueError("timeout-ms must be between 1 and 30000")
    expires = _parse_expiry(expires)
    path = _resolve_plugin(plugin_path)
    sha256, byte_size = _sha256_and_size(path)
    return {
        "schema_version": 1,
        "entries": [
            {
                "id": plugin_id,
                "plugin_path": str(path),
                "sha256": sha256,
                "byte_size": byte_size,
                "approved_stage": "L1",
                "receipt_id": receipt_id,
                "expires": expires,
                "timeout_ms": timeout_ms,
            }
        ],
    }


def write_allowlist(document: dict[str, object], output: str, *, force: bool = False) -> Path:
    path = _resolve_output(output)
    mode = "w" if force else "x"
    with path.open(mode, encoding="utf-8", newline="\n") as stream:
        json.dump(document, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    return path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plugin-id", required=True)
    parser.add_argument("--plugin-path", required=True)
    parser.add_argument("--receipt-id", required=True)
    parser.add_argument("--expires", required=True)
    parser.add_argument("--timeout-ms", required=True, type=int)
    parser.add_argument("--output", required=True)
    parser.add_argument("--force", action="store_true", help="replace an existing output")
    args = parser.parse_args()
    try:
        document = build_allowlist(
            plugin_id=args.plugin_id,
            plugin_path=args.plugin_path,
            receipt_id=args.receipt_id,
            expires=args.expires,
            timeout_ms=args.timeout_ms,
        )
        output = write_allowlist(document, args.output, force=args.force)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
