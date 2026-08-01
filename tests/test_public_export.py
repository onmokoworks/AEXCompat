import importlib.util
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("public_export", ROOT / "tools/public_export.py")
public_export = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = public_export
SPEC.loader.exec_module(public_export)


def git(repository: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repository, check=True, capture_output=True, text=True
    ).stdout.strip()


def test_export_keeps_only_main_and_explicit_tags_and_sanitizes_history(tmp_path):
    source = tmp_path / "source"
    source.mkdir()
    git(source, "init", "-b", "main")
    git(source, "config", "user.name", "Test")
    git(source, "config", "user.email", "test@host.tailbe216f.ts.net")
    (source / "README.md").write_text("public\n", encoding="utf-8")
    (source / "machine.txt").write_text(
        "C:\\Users\\alice\\private and /Users/bob/private\n", encoding="utf-8"
    )
    (source / "private.dll").write_bytes(b"not public")
    git(source, "add", ".")
    git(source, "commit", "-m", "initial")
    git(source, "tag", "v1")
    git(source, "tag", "internal-wip")
    (source / "private.dll").unlink()
    git(source, "add", "-u")
    git(source, "commit", "-m", "remove private payload")

    output = tmp_path / "export"
    public_export.create_export(source, output, ["v1"], "public@users.noreply.github.com")

    assert git(output, "for-each-ref", "--format=%(refname)", "refs/heads", "refs/tags").splitlines() == [
        "refs/heads/main", "refs/tags/v1"
    ]
    assert set(git(output, "log", "--all", "--format=%ae%n%ce").splitlines()) == {
        "public@users.noreply.github.com"
    }
    assert "private.dll" not in git(output, "log", "--all", "--name-only", "--format=")
    exported_text = (output / "machine.txt").read_text(encoding="utf-8")
    assert "alice" not in exported_text and "bob" not in exported_text
    assert exported_text.count("<redacted-home>") == 2
    assert public_export.scan_export(output) == []
    git(output, "fsck", "--full", "--no-reflogs", "--no-dangling")


def test_rejects_implicit_or_unsafe_tag_names():
    for tag in ("", "../wip", "bad tag", "refs/tags/v1"):
        try:
            public_export.validate_tag(tag)
        except ValueError:
            pass
        else:
            raise AssertionError(f"unsafe tag accepted: {tag!r}")


def test_script_has_no_push_implementation():
    source = (ROOT / "tools/public_export.py").read_text(encoding="utf-8")
    assert '["git", "push"' not in source
