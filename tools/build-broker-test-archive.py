#!/usr/bin/env python3
"""Build the broker test archive with the existing locked workspace command."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'broker/Cargo.toml'


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f'duplicate metadata key: {key}')
        result[key] = value
    return result


def _metadata_ids(items: object, label: str) -> set[str]:
    if not isinstance(items, list) or not items:
        raise RuntimeError(f'full offline Cargo metadata has no {label}')
    ids = [item.get('id') if isinstance(item, dict) else None for item in items]
    if not all(isinstance(value, str) and value for value in ids):
        raise RuntimeError(f'full offline Cargo metadata has invalid {label} IDs')
    if len(set(ids)) != len(ids):
        raise RuntimeError(f'full offline Cargo metadata has duplicate {label} IDs')
    return set(ids)


def _cache_miss_kind(stderr: str) -> str:
    # Report only stable categories: Cargo errors can contain paths, URLs, or
    # credentials. Do not copy the raw diagnostic into hosted CI logs.
    message = stderr.lower()
    if 'no matching package named' in message:
        return 'missing_index_entry'
    if 'failed to download' in message and '--offline' in message:
        return 'missing_crate_archive'
    if ('unable to update' in message or "can't checkout" in message) and 'offline' in message:
        return 'missing_git_checkout'
    if 'lock file' in message and ('needs to be updated' in message or '--locked' in message):
        return 'locked_input_change'
    if 'failed to parse manifest' in message or 'failed to load manifest' in message:
        return 'manifest_error'
    return 'other'


def cached_dependencies_ready(manifest: Path) -> bool:
    # --no-deps would only prove the manifest can be read, not that the locked
    # dependency sources are available. A cache miss keeps the old online path.
    result = subprocess.run(
        ['cargo', 'metadata', '--manifest-path', str(manifest), '--locked', '--offline', '--format-version', '1'],
        cwd=manifest.parent, check=False, capture_output=True, text=True,
        encoding='utf-8', errors='replace',
    )
    if result.returncode:
        print(f'offline_dependency_cache=unavailable metadata_exit={result.returncode}', flush=True)
        print(f'offline_dependency_cache_kind={_cache_miss_kind(result.stderr)}', flush=True)
        return False
    try:
        metadata = json.loads(result.stdout, object_pairs_hook=_unique_object)
    except (ValueError, TypeError) as error:
        raise RuntimeError('offline Cargo metadata is not valid JSON') from error
    if not isinstance(metadata, dict) or type(metadata.get('version')) is not int or metadata['version'] != 1:
        raise RuntimeError('offline Cargo metadata has an unsupported schema')
    workspace = metadata.get('workspace_root')
    if not isinstance(workspace, str) or not workspace or not Path(workspace).is_absolute():
        raise RuntimeError('offline Cargo metadata has no absolute workspace root')
    if Path(workspace).resolve() != manifest.parent.resolve():
        raise RuntimeError('offline Cargo metadata belongs to a different workspace')
    packages = _metadata_ids(metadata.get('packages'), 'packages')
    resolve = metadata.get('resolve')
    nodes = _metadata_ids(resolve.get('nodes') if isinstance(resolve, dict) else None, 'resolved nodes')
    members = metadata.get('workspace_members')
    if (not isinstance(members, list) or not members
            or not all(isinstance(member, str) and member for member in members)
            or len(set(members)) != len(members)
            or not set(members) <= packages & nodes or not nodes <= packages):
        raise RuntimeError('offline Cargo metadata has an incomplete workspace resolution')
    print(f'offline_dependency_cache=ready packages={len(packages)} workspace_members={len(members)}', flush=True)
    return True


def build_archive(manifest: Path = MANIFEST, archive: str = 'target/nextest-archive.tar.zst') -> None:
    command = ['cargo', 'nextest', 'archive', '--workspace', '--locked', '--archive-file', archive]
    if cached_dependencies_ready(manifest):
        command.append('--offline')
    print('+', subprocess.list2cmdline(command), flush=True)
    # A successful preflight is only a candidate. Never retry or suppress a
    # failed actual archive, including an input missing after the preflight.
    subprocess.run(command, cwd=manifest.parent, check=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--archive-file', default='target/nextest-archive.tar.zst')
    args = parser.parse_args()
    try:
        build_archive(archive=args.archive_file)
    except subprocess.CalledProcessError as error:
        return error.returncode
    except RuntimeError as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
