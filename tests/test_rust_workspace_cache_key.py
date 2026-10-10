"""Workspace cache freshness is checked with compiled output, not source grep."""

import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "rust-workspace-cache-key.py"
spec = importlib.util.spec_from_file_location("rust_workspace_cache_key", SCRIPT)
KEY = importlib.util.module_from_spec(spec)
spec.loader.exec_module(KEY)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


@pytest.fixture
def source(tmp_path):
    root = tmp_path / "source with spaces"
    for name in KEY.REQUIRED_FILES:
        write(root / name, "fixture input\n")
    shutil.copyfile(SCRIPT, root / "tools" / SCRIPT.name)
    return root


@pytest.fixture
def compiled_targets():
    # MSVC link.exe cannot create a >MAX_PATH build-script output. pytest's
    # long node-id directory plus a full SHA256 key exceeds that on hosted CI.
    # Keep the full key and use a private, short TEMP child (D: locally).
    with tempfile.TemporaryDirectory(prefix="aex-cache-") as directory:
        yield Path(directory)


def digest(root):
    return KEY.build_key(root)["digest"]


def test_key_uses_paths_and_bytes_not_mtimes(source):
    original = digest(source)
    item = source / "broker" / "same-content.rs"
    write(item, "pub fn answer() -> u32 { 42 }\n")
    with_file = digest(source)
    assert with_file != original
    os.utime(item, (946684800, 946684800))
    assert digest(source) == with_file
    item.rename(item.with_name("different-path.rs"))
    assert digest(source) != with_file


@pytest.mark.parametrize("name", [
    "profiles/scattermap/parameter_descriptors.json", "LICENSE",
    "tools/restore-mtime.py", "tools/rust-workspace-cache-key.py",
    ".github/actions/ci-setup/action.yml", ".github/workflows/windows-clean-clone.yml",
])
def test_external_and_producer_inputs_change_key(source, name):
    original = digest(source)
    write(source / name, "changed fixture input\n")
    assert digest(source) != original


def test_only_workspace_target_is_excluded(source):
    original = digest(source)
    write(source / "broker" / "target" / "debug" / "generated.rs", "output\n")
    assert digest(source) == original
    write(source / "broker" / "core" / "src" / "target" / "module.rs", "input\n")
    assert digest(source) != original


def test_missing_input_refuses_without_emitting_key(source, tmp_path):
    (source / "LICENSE").unlink()
    output = tmp_path / "github-output"
    result = subprocess.run(
        [os.sys.executable, str(SCRIPT), "--root", str(source), "--github-output", str(output)],
        capture_output=True, text=True, encoding="utf-8", timeout=30,
    )
    assert result.returncode != 0
    assert not output.exists()


def test_symlink_input_is_not_silently_omitted(source, tmp_path):
    outside = tmp_path / "outside"
    write(outside, "outside input")
    link = source / "broker" / "linked.rs"
    try:
        link.symlink_to(outside)
    except OSError:
        pytest.skip("OS does not permit test symlinks")
    with pytest.raises(KEY.InputError):
        digest(source)


def test_disappearing_input_tree_is_not_optional(source, monkeypatch):
    original = Path.iterdir

    def disappear(path):
        if path == source / "broker":
            path.rename(source / "removed-broker")
            raise FileNotFoundError("fixture disappeared during enumeration")
        return original(path)

    monkeypatch.setattr(Path, "iterdir", disappear)
    with pytest.raises(FileNotFoundError):
        digest(source)


@pytest.mark.skipif(os.name != "nt", reason="real Windows junctions")
@pytest.mark.parametrize("placement", ["parent", "source-child", "ignored-target", "individual-file-parent"])
def test_junctions_are_rejected_before_following(source, tmp_path, placement, monkeypatch):
    if placement == "parent":
        junction = tmp_path / "parent junction"
        target = tmp_path
        requested = junction / source.name
    elif placement == "individual-file-parent":
        junction = source / "tools"
        target = tmp_path / "outside tools target"
        junction.rename(target)
        requested = source
    else:
        junction = source / "broker" / ("target" if placement == "ignored-target" else "linked")
        target = tmp_path / "outside junction target"
        target.mkdir()
        requested = source
    quoted_path = str(junction).replace("'", "''")
    quoted_target = str(target).replace("'", "''")
    subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         f"New-Item -ItemType Junction -Path '{quoted_path}' -Target '{quoted_target}' "
         "-ErrorAction Stop | Out-Null"],
        check=True, capture_output=True, timeout=30,
    )
    try:
        if placement == "individual-file-parent":
            def refuse_content_read(path):
                pytest.fail("input bytes were read before parent junction rejection")
            monkeypatch.setattr(Path, "read_bytes", refuse_content_read)
        with pytest.raises(KEY.InputError):
            digest(requested)
    finally:
        # Remove only this owned junction, never its target tree.
        junction.rmdir()
    assert target.is_dir()


def cargo(args, source, target, revision="one"):
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(target), AEXCOMPAT_BUILD_REVISION=revision,
               TEMP=str(source.parent), TMP=str(source.parent))
    result = subprocess.run(
        ["cargo", *args, "--offline", "--manifest-path", str(source / "broker" / "Cargo.toml")],
        cwd=ROOT, env=env, capture_output=True, text=True, encoding="utf-8", timeout=120,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result


def build(source, target, revision="one"):
    result = cargo(["build", "--locked", "--message-format=json"], source, target, revision)
    fresh = {}
    binary = None
    for line in result.stdout.splitlines():
        event = json.loads(line)
        if event["reason"] == "compiler-artifact":
            fresh[event["target"]["name"]] = event["fresh"]
            if event.get("executable"):
                binary = Path(event["executable"])
    assert binary and binary.is_file()
    output = subprocess.run([str(binary)], check=True, capture_output=True, text=True, timeout=30)
    return fresh, output.stdout.strip()


@pytest.mark.skipif(shutil.which("cargo") is None, reason="actual Cargo fixture")
def test_content_key_prevents_stale_cargo_result_and_preserves_revision(source, compiled_targets):
    write(source / "broker" / "Cargo.toml",
          '[workspace]\nresolver="2"\nmembers=["core", "app"]\n')
    write(source / "broker" / "core" / "Cargo.toml",
          '[package]\nname="cache_fixture_core"\nversion="0.1.0"\nedition="2021"\n')
    core = source / "broker" / "core" / "src" / "lib.rs"
    body = ('pub fn answer() -> u32 { 42 }\n'
            'pub fn external() -> u32 { include_str!("../../../profiles/scattermap/'
            'parameter_descriptors.json").trim().parse().unwrap() }\n')
    write(core, body)
    write(source / "profiles" / "scattermap" / "parameter_descriptors.json", "7\n")
    write(source / "broker" / "app" / "Cargo.toml",
          '[package]\nname="cache_fixture_app"\nversion="0.1.0"\nedition="2024"\n'
          '[dependencies]\ncache_fixture_core={path="../core"}\n')
    # Compile the actual broker revision carrier, rather than a lookalike
    # build script. The fixture's labels are explicitly test-only provenance.
    shutil.copyfile(ROOT / "broker" / "crates" / "broker" / "build.rs",
                    source / "broker" / "app" / "build.rs")
    write(source / "broker" / "app" / "src" / "main.rs",
          'fn main(){println!("{},{},{}",cache_fixture_core::answer(),'
          'cache_fixture_core::external(),env!("AEXCOMPAT_BUILD_REVISION"));}\n')
    targets = compiled_targets
    cargo(["generate-lockfile"], source, targets / "lock-only")
    first = digest(source)
    original_target = targets / first
    _, output = build(source, original_target)
    assert output == "42,7,one"
    fresh, output = build(source, original_target)
    assert fresh["cache_fixture_core"] and fresh["cache_fixture_app"]
    assert output == "42,7,one"

    # This explicitly-authored timestamp models a commit older than the cache.
    # An unkeyed restored target really returns stale output on affected Cargo.
    write(core, body.replace("{ 42 }", "{ 99 }"))
    os.utime(core, (946684800, 946684800))
    changed = digest(source)
    assert changed != first
    fresh, output = build(source, targets / changed)
    assert not fresh["cache_fixture_core"] and output == "99,7,one"

    external = source / "profiles" / "scattermap" / "parameter_descriptors.json"
    write(external, "11\n")
    os.utime(external, (946684800, 946684800))
    with_external = digest(source)
    assert with_external != changed
    fresh, output = build(source, targets / with_external)
    assert not fresh["cache_fixture_core"] and output == "99,11,one"

    # Revision is NOT frozen into the source key: Cargo must regenerate its
    # carrier when the actual build revision changes, while reusing the core.
    assert digest(source) == with_external
    fresh, output = build(source, targets / with_external, revision="two")
    assert fresh["cache_fixture_core"] and not fresh["cache_fixture_app"]
    assert output == "99,11,two"
