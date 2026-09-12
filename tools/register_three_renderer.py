"""One-time installer registration for an already-built Three renderer.

Does not start executables, install dependencies, or modify an existing record.
The installer supplies its runtime location; subsequent GUI/CLI renders resolve
this shared per-user registration without a runtime-folder picker.
"""
import argparse
import hashlib
import json
import os
import tempfile
from pathlib import Path


def path_key(path):
    text = str(path)
    if text.startswith('\\\\?\\UNC\\'):
        text = '//' + text[8:]
    elif text.startswith('\\\\?\\'):
        text = text[4:]
    text = text.replace('\\', '/').translate(str.maketrans(
        'ABCDEFGHIJKLMNOPQRSTUVWXYZ', 'abcdefghijklmnopqrstuvwxyz'))
    return hashlib.sha256(text.encode('utf-8')).hexdigest()


def registration_path(directory, plugin):
    return Path(directory)/(path_key(Path(plugin).resolve(strict=True))+'.json')


def register(runtime_root, plugins, destination):
    root = Path(runtime_root).resolve(strict=True)
    if not root.is_dir():
        raise ValueError('renderer root is not a directory')
    for relative in ('node_modules/electron/dist/electron.exe',
                     'out/main/index.js', 'out/renderer/index.html'):
        asset = (root / relative).resolve(strict=True)
        if not asset.is_relative_to(root) or not asset.is_file():
            raise ValueError(f'invalid renderer asset: {relative}')
    paths = [Path(p).resolve(strict=True) for p in plugins]
    if not 1 <= len(paths) <= 16 or any(not p.is_file() for p in paths):
        raise ValueError('expected 1..16 installed image AEX files')
    if any(p.suffix.lower() != '.aex' for p in paths):
        raise ValueError('plugin path must name an AEX')
    if len({os.path.normcase(str(p)) for p in paths}) != len(paths):
        raise ValueError('duplicate plugin association')
    record = dict(schema_version=1, backend='three-v1',
                  plugin_paths=[str(p) for p in paths], runtime_root=str(root))
    data = (json.dumps(record, indent=2, ensure_ascii=False)+'\n').encode('utf-8')
    if len(data) > 65536:
        raise ValueError('registration exceeds 64 KiB')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Publish only a complete file, without replacing an existing registration.
    descriptor, name = tempfile.mkstemp(prefix='.three-register-', dir=destination.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, destination)
    finally:
        temporary.unlink()
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime-root', type=Path, required=True)
    parser.add_argument('--plugin', type=Path, required=True)
    args = parser.parse_args()
    local = os.environ.get('LOCALAPPDATA')
    if not local:
        parser.error('LOCALAPPDATA is unavailable')
    try:
        directory = Path(local)/'AEXCompat/render-services/three-v1'
        destination = registration_path(directory, args.plugin)
        register(args.runtime_root, [args.plugin], destination)
    except (OSError, ValueError) as error:
        parser.exit(1, f'Registration not changed: {error}\n')
    print(f'Registered Three renderer: {destination}')


if __name__ == '__main__':
    main()
