#!/usr/bin/env python3
"""Retain reviewed X05 binaries/source; never overwrite an existing cache entry."""
import argparse
import fcntl
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
CACHE = Path.home() / '.cache/olm-emu'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-root', type=Path, default=CACHE / 'build-X05')
    parser.add_argument('--name', default='worker-X05-ui-canvas')
    args = parser.parse_args()
    assert args.name and '/' not in args.name and args.name not in ['.', '..']
    paths = ['guest', 'tools/mac-custom-ui', 'docs/olm/X05-mac-custom-ui-canvas.md', 'rust-toolchain.toml', 'AGENTS.md', 'CLAUDE.md']
    assert not subprocess.check_output(['git', 'status', '--porcelain', '--', *paths], cwd=ROOT), 'commit reviewed source before retention'
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    output = CACHE / args.name
    output.mkdir(exist_ok=False)
    worker = output / 'aex-guest-worker'
    shutil.copy2(args.build_root / 'target/release/aex-guest-worker', worker)
    shutil.copytree(args.build_root / 'MacCustomUI.app', output / 'MacCustomUI.app')
    canvas = output / 'MacCustomUI.app/Contents/MacOS/mac-custom-ui'
    archive = output / 'source.tar.gz'
    with archive.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as compressed:
        process = subprocess.Popen(['git', 'archive', revision, *paths], cwd=ROOT, stdout=subprocess.PIPE)
        shutil.copyfileobj(process.stdout, compressed)
        process.stdout.close()
        assert process.wait() == 0
    build = {
        'source_revision': revision,
        'source_branch': 'olm/X05-custom-ui-canvas',
        'source_archive_sha256': sha(archive),
        'worker_sha256': sha(worker),
        'canvas_sha256': sha(canvas),
        'rustc': subprocess.check_output(['rustc', '--version'], cwd=ROOT, text=True).strip(),
        'swiftc': subprocess.check_output(['swiftc', '--version'], cwd=ROOT, text=True).strip(),
        'worker_build': 'TMPDIR=$HOME/.cache/olm-emu/build-X05/tmp CARGO_TARGET_DIR=$HOME/.cache/olm-emu/build-X05/target CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_INCREMENTAL=0 cargo build --release --locked --offline --manifest-path guest/Cargo.toml -p aex-guest-worker -j 1',
        'canvas_build': './tools/mac-custom-ui/build.sh',
        'validation': 'See docs/olm/X05-mac-custom-ui-canvas.md and the OLM X05 report; retention alone is not a test verdict.',
    }
    (output / 'build.json').write_text(json.dumps(build, indent=2) + '\n')
    with (CACHE / '.index.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = CACHE / 'INDEX.json'
        index = json.loads(path.read_text())
        assert not any(entry['name'] == args.name for entry in index['links'])
        index['links'].append({'name': args.name, 'parent': 'worker-X01b-parity', 'worker_path': str(worker), 'worker_sha256': sha(worker), 'canvas_path': str(canvas), 'canvas_sha256': sha(canvas), 'build_json': str(output / 'build.json'), 'recipe': 'tools/mac-custom-ui/install.py', 'capability': 'X05 additive UI stream/key worker and AppKit custom-control laboratory; not AE raster equivalence'})
        staged = CACHE / 'INDEX.X05.json'
        staged.write_text(json.dumps(index, indent=2) + '\n')
        staged.replace(path)
    print(json.dumps(build, sort_keys=True))


if __name__ == '__main__':
    main()
