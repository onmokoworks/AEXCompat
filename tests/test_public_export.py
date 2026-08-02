import importlib.util
import subprocess
import sys
from pathlib import Path

import pytest


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
    pytest.importorskip("git_filter_repo")
    source = tmp_path / "source"
    source.mkdir()
    git(source, "init", "-b", "main")
    git(source, "config", "user.name", "Test")
    git(source, "config", "user.email", "test@host.tailbe216f.ts.net")
    (source / "README.md").write_text("public\n", encoding="utf-8")
    (source / "machine.txt").write_text("public payload\n", encoding="utf-8")
    workflow = source / ".github" / "workflows" / "ci.yml"
    workflow.parent.mkdir(parents=True)
    workflow.write_text("- uses: actions/checkout@v4\n", encoding="utf-8")
    schema = source / "schemas" / "probe.schema.json"
    schema.parent.mkdir(parents=True)
    schema_contents = '{"pattern":"^[^/\\\\:]+\\.inf$"}\n'
    schema.write_text(schema_contents, encoding="utf-8")
    worker_source = source / "worker.cpp"
    pipe_literal = 'L"\\\\.\\pipe\\ae-timeline-sync"\n'
    worker_source.write_text(pipe_literal, encoding="utf-8")
    (source / "private.dll").write_bytes(b"not public")
    git(source, "add", ".")
    git(
        source,
        "commit",
        "-m",
        "built under C:\\Users\\alice\\checkout\n\n"
        "Co-authored-by: Test <test@host.tailbe216f.ts.net>\n"
        "Reviewed-by: Alice <alice@workstation.local>\n"
        "Public: Alice Local <alice.local@example.com>\n"
        "Public: Named User <naari.named@gmail.com>\n"
        "Public: Build User <alice@build-host.example.com>\n"
        "Public: Unicode User <alice@bücher.example>\n"
        "Private: Unicode Host <alice@bücher>\n"
        "Public: Unicode Local <álîce@example.com>\n"
        "Private: Unicode Local <álîce@workstation>\n"
        "Private: Digit Host <alice@3dworkstation>\n"
        "Private: Tagged Local <alice!tag@workstation>\n"
        "Public: Tagged Local <alice!tag@example.com>\n"
        "Protocol: Suite@2",
    )
    git(source, "config", "user.email", "tagger@workstation.local")
    git(source, "tag", "-a", "inner", "-m", "inner release")
    inner_oid = git(source, "rev-parse", "refs/tags/inner")
    git(source, "tag", "-d", "inner")
    git(source, "config", "user.email", "public-tagger@example.invalid")
    git(source, "tag", "-a", "v1", "-m", "public release", inner_oid)
    git(source, "config", "user.email", "alice@workstation")
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
    assert git(output, "for-each-ref", "--format=%(taggeremail)", "refs/tags/v1") == (
        "<public-tagger@example.invalid>"
    )
    exported_inner_oid = git(output, "rev-parse", "refs/tags/v1^{tag}")
    exported_inner_oid = git(output, "cat-file", "-p", exported_inner_oid).splitlines()[0].split()[1]
    assert "<public@users.noreply.github.com>" in git(output, "cat-file", "-p", exported_inner_oid)
    assert "private.dll" not in git(output, "log", "--all", "--name-only", "--format=")
    assert (output / "machine.txt").read_text(encoding="utf-8") == "public payload\n"
    assert (output / ".github" / "workflows" / "ci.yml").read_text(
        encoding="utf-8"
    ) == "- uses: actions/checkout@v4\n"
    assert (output / "worker.cpp").read_text(encoding="utf-8") == pipe_literal
    assert (output / "schemas" / "probe.schema.json").read_text(
        encoding="utf-8"
    ) == schema_contents
    exported_messages = git(output, "log", "--all", "--format=%B")
    assert "tailbe216f.ts.net" not in exported_messages
    assert "workstation.local" not in exported_messages
    assert "<redacted-home>" in exported_messages
    assert "<redacted-private-email>" in exported_messages
    assert "alice.local@example.com" in exported_messages
    assert "naari.named@gmail.com" in exported_messages
    assert "alice@build-host.example.com" in exported_messages
    assert "alice@bücher.example" in exported_messages
    assert "alice@bücher>" not in exported_messages
    assert "álîce@example.com" in exported_messages
    assert "álîce@workstation" not in exported_messages
    assert "alice@3dworkstation" not in exported_messages
    assert "alice!tag@workstation" not in exported_messages
    assert "alice!tag@example.com" in exported_messages
    assert "Suite@2" in exported_messages
    assert public_export.scan_export(output) == []
    git(output, "fsck", "--full", "--no-reflogs", "--no-dangling")


def test_export_drops_scanner_fixture_history_and_restores_tip_bytes(tmp_path):
    pytest.importorskip("git_filter_repo")
    source = tmp_path / "source"
    source.mkdir()
    git(source, "init", "-b", "main")
    git(source, "config", "user.name", "Test")
    git(source, "config", "user.email", "test@example.invalid")
    fixture = source / "tests" / "test_public_export.py"
    fixture.parent.mkdir()
    diagnostic = source / "analysis" / "result.json"
    diagnostic.parent.mkdir()
    diagnostic_toml = source / "analysis" / "result.toml"
    diagnostic_env = source / "analysis" / "result.env"
    root_dotenv = source / ".env"
    local_dotenv = source / "analysis" / ".env.local"
    old_contents = "token = 'ghp_" + "abcdefghijklmnopqrstuvwxyz123456'\n"
    fixture.write_text(old_contents, encoding="utf-8")
    diagnostic.write_text('{"path":"C:/' + 'Users/alice/private"}\n', encoding="utf-8")
    diagnostic_toml.write_text(
        "owner='alice!tag@workstation'\n"
        "equals='alice=tag@workstation'\n"
        "query='alice?tag@workstation'\n",
        encoding="utf-8",
    )
    diagnostic_env.write_text(
        "OWNER=alice@workstation\n"
        "TAGGED=alice=tag@workstation\n",
        encoding="utf-8",
    )
    root_dotenv.write_text("OWNER=alice@workstation\n", encoding="utf-8")
    local_dotenv.write_text(
        "# Contact alice@workstation\nOWNER=alice@workstation\n",
        encoding="utf-8",
    )
    git(
        source,
        "add",
        "tests/test_public_export.py",
        "analysis/result.json",
        "analysis/result.toml",
        "analysis/result.env",
        ".env",
        "analysis/.env.local",
    )
    git(source, "commit", "-m", "old synthetic scanner fixture")
    old_oid = git(source, "hash-object", "tests/test_public_export.py")
    old_diagnostic_oid = git(source, "hash-object", "analysis/result.json")
    old_toml_oid = git(source, "hash-object", "analysis/result.toml")
    old_env_oid = git(source, "hash-object", "analysis/result.env")
    old_root_dotenv_oid = git(source, "hash-object", ".env")
    old_local_dotenv_oid = git(source, "hash-object", "analysis/.env.local")
    tip_contents = b"safe current scanner fixture\n"
    fixture.write_bytes(tip_contents)
    diagnostic.write_text(
        '{"path":"D:/'
        + 'Projects/current/result","owner":"alice@workstation",'
        '"digit_owner":"alice@3dworkstation",'
        '"tagged_owner":"alice!tag@workstation","suite":"Suite@2",'
        '"protocol":"v2|brightness@1"}\n',
        encoding="utf-8",
    )
    git(
        source,
        "add",
        "tests/test_public_export.py",
        "analysis/result.json",
        "analysis/result.toml",
        "analysis/result.env",
        ".env",
        "analysis/.env.local",
    )
    git(source, "commit", "-m", "split synthetic scanner fixture")

    output = tmp_path / "export"
    public_export.create_export(
        source, output, [], "public@users.noreply.github.com"
    )

    assert (output / "tests" / "test_public_export.py").read_bytes() == tip_contents
    assert (output / "analysis" / "result.json").read_text(encoding="utf-8") == (
        '{"path":"<redacted-windows-path>","owner":"<redacted-private-email>",'
        '"digit_owner":"<redacted-private-email>",'
        '"tagged_owner":"<redacted-private-email>","suite":"Suite@2",'
        '"protocol":"v2|brightness@1"}\n'
    )
    assert (output / "analysis" / "result.toml").read_text(encoding="utf-8") == (
        "owner='<redacted-private-email>'\n"
        "equals='<redacted-private-email>'\n"
        "query='<redacted-private-email>'\n"
    )
    assert (output / "analysis" / "result.env").read_text(encoding="utf-8") == (
        "OWNER=<redacted-private-email>\n"
        "TAGGED=<redacted-private-email>\n"
    )
    assert (output / ".env").read_text(encoding="utf-8") == (
        "OWNER=<redacted-private-email>\n"
    )
    assert (output / "analysis" / ".env.local").read_text(encoding="utf-8") == (
        "# Contact <redacted-private-email>\n"
        "OWNER=<redacted-private-email>\n"
    )
    assert old_oid not in git(output, "rev-list", "--objects", "--all")
    assert old_diagnostic_oid not in git(output, "rev-list", "--objects", "--all")
    assert old_toml_oid not in git(output, "rev-list", "--objects", "--all")
    assert old_env_oid not in git(output, "rev-list", "--objects", "--all")
    assert old_root_dotenv_oid not in git(output, "rev-list", "--objects", "--all")
    assert old_local_dotenv_oid not in git(output, "rev-list", "--objects", "--all")
    assert public_export.scan_export(output) == []


def test_rejects_implicit_or_unsafe_tag_names():
    for tag in (
        "",
        "../wip",
        "bad tag",
        "refs/tags/v1",
        "ghp_" + "abcdefghijklmnopqrstuvwxyz123456",
    ):
        try:
            public_export.validate_tag(tag)
        except ValueError:
            pass
        else:
            raise AssertionError(f"unsafe tag accepted: {tag!r}")


def test_tree_name_scan_excludes_binary_object_ids():
    binary_oid = b"j1@k" + bytes(16)
    payload = b"100644 safe.txt\0" + binary_oid

    names = public_export.names_from_tree(payload, 20)

    assert names == b"safe.txt"
    assert public_export.PRIVATE_EMAIL_IN_PAYLOAD.search(names) is None


def test_rejects_selected_tag_that_does_not_resolve_to_commit(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    payload = repository / "payload.bin"
    payload.write_bytes(b"binary payload")
    oid = git(repository, "hash-object", "-w", "payload.bin")
    git(repository, "tag", "blob-tag", oid)

    with pytest.raises(ValueError, match="does not resolve to a commit"):
        public_export.validate_selected_tags(repository, ["blob-tag"])


def test_single_label_host_email_is_private(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Alice")
    git(repository, "config", "user.email", "alice @workstation.local")
    (repository / "README.md").write_text("public\n", encoding="utf-8")
    git(repository, "add", "README.md")
    git(repository, "commit", "-m", "local identity")

    assert public_export.PRIVATE_EMAIL.search("alice @workstation.local")
    assert any(
        "private identity email in reachable commit" in item
        for item in public_export.scan_export(repository)
    )


@pytest.mark.parametrize(
    "email", [
        "alice.local@example.com",
        "naari.named@gmail.com",
        "alice@build-host.example.com",
        "alice@bücher.example",
        "álîce@example.com",
        "alice!tag@example.com",
    ]
)
def test_public_dotted_domain_email_is_not_private(email):
    assert public_export.PRIVATE_EMAIL.search(email) is None
    assert public_export.PRIVATE_EMAIL_IN_PAYLOAD.search(email.encode()) is None


def test_scan_finds_prohibited_path_created_by_merge_resolution(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    (repository / "README.md").write_text("base\n", encoding="utf-8")
    git(repository, "add", "README.md")
    git(repository, "commit", "-m", "base")
    git(repository, "checkout", "-b", "side")
    (repository / "side.txt").write_text("side\n", encoding="utf-8")
    git(repository, "add", "side.txt")
    git(repository, "commit", "-m", "side")
    git(repository, "checkout", "main")
    (repository / "main.txt").write_text("main\n", encoding="utf-8")
    git(repository, "add", "main.txt")
    git(repository, "commit", "-m", "main")
    git(repository, "merge", "--no-commit", "side")
    private = repository / "private" / "payload.txt"
    private.parent.mkdir()
    private.write_text("merge resolution\n", encoding="utf-8")
    git(repository, "add", "private/payload.txt")
    git(repository, "commit", "-m", "merge with private resolution")

    findings = public_export.scan_export(repository)

    assert any("prohibited historical path: private/payload.txt" in item for item in findings)


def test_scan_finds_non_ascii_uppercase_prohibited_suffix(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    prohibited = repository / "秘密.DLL"
    prohibited.write_bytes(b"not public")
    git(repository, "add", "秘密.DLL")
    git(repository, "commit", "-m", "add quoted prohibited path")

    assert any(
        "prohibited historical path: 秘密.DLL" in item
        for item in public_export.scan_export(repository)
    )


def test_scan_includes_commit_and_annotated_tag_messages(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    (repository / "README.md").write_text("public\n", encoding="utf-8")
    (repository / "diagnostic.json").write_text(
        r'{"home":"C:\\users\\alice\\checkout",'
        r'"workspace":"D:\\Projects\\private-checkout",'
        r'"forward":"E:/Projects/private-checkout",'
        r'"container":"/workspace/alice/private/AEXCompat",'
        r'"share":"\\\\alice-pc\\private-share\\AEXCompat"}',
        encoding="utf-8",
    )
    secret_name = "gho_" + "abcdefghijklmnopqrstuvwxyz123456"
    (repository / secret_name).write_text("empty payload\n", encoding="utf-8")
    git(repository, "add", "README.md", "diagnostic.json", secret_name)
    git(repository, "commit", "-m", r"built under C:\Users\alice\checkout")
    git(
        repository,
        "tag",
        "-a",
        "v1",
        "-m",
        "token ghp_" + "abcdefghijklmnopqrstuvwxyz123456",
    )

    findings = public_export.scan_export(repository)

    assert any("Windows user path in reachable commit" in item for item in findings)
    assert any("Windows user path in reachable blob" in item for item in findings)
    assert any("Windows absolute path in reachable blob" in item for item in findings)
    assert any("Windows UNC path in reachable blob" in item for item in findings)
    assert any("container workspace path in reachable blob" in item for item in findings)
    assert any("GitHub token candidate in reachable tag" in item for item in findings)
    assert any("GitHub token candidate in reachable tree" in item for item in findings)


def test_scanner_fixture_path_does_not_exempt_real_secret(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    fixture = repository / "tests" / "test_public_export.py"
    fixture.parent.mkdir()
    fixture.write_text(
        "credential = 'ghp_" + "abcdefghijklmnopqrstuvwxyz123456'\n",
        encoding="utf-8",
    )
    git(repository, "add", "tests/test_public_export.py")
    git(repository, "commit", "-m", "accidental credential")

    assert any(
        "GitHub token candidate in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_scanner_fixture_path_does_not_exempt_real_personal_path(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    fixture = repository / "tests" / "test_public_export.py"
    fixture.parent.mkdir()
    real_path = "/" + "workspace/alice/customer/AEXCompat"
    fixture.write_text(f"checkout = '{real_path}'\n", encoding="utf-8")
    git(repository, "add", "tests/test_public_export.py")
    git(repository, "commit", "-m", "accidental personal path")

    assert any(
        "container workspace path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_scanner_fixture_path_does_not_exempt_real_private_email(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    fixture = repository / "tests" / "test_public_export.py"
    fixture.parent.mkdir()
    private_email = "bob@customer" + ".local"
    fixture.write_text(f"owner = '{private_email}'\n", encoding="utf-8")
    git(repository, "add", "tests/test_public_export.py")
    git(repository, "commit", "-m", "accidental private email")

    assert any(
        "private email in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


@pytest.mark.parametrize(
    ("payload", "expected"),
    [
        (b"temporary key ASIA" + b"ABCDEFGHIJKLMNOP", "AWS access key candidate"),
        (b"-----BEGIN DSA " + b"PRIVATE KEY-----", "private key candidate"),
        (b"-----BEGIN ENCRYPTED " + b"PRIVATE KEY-----", "private key candidate"),
        (b"checkout /home/alice/private/AEXCompat", "Linux user path"),
        (b"checkout /root/private/AEXCompat", "Linux root path"),
        (b"checkout D:/Projects/alice/private/AEXCompat", "Windows absolute path"),
    ],
)
def test_scan_recognizes_publication_sensitive_variants(tmp_path, payload, expected):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    payload_path = repository / "analysis" / "payload.txt"
    payload_path.parent.mkdir()
    payload_path.write_bytes(payload)
    git(repository, "add", "analysis/payload.txt")
    git(repository, "commit", "-m", "fixture")

    assert any(expected in item for item in public_export.scan_export(repository))


def test_public_documentation_path_literal_is_not_private_diagnostic(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    docs = repository / "docs" / "BUILD_REQUIREMENTS.md"
    docs.parent.mkdir()
    docs.write_text("Install under C:\\Program Files\\Tool\n", encoding="utf-8")
    git(repository, "add", "docs/BUILD_REQUIREMENTS.md")
    git(repository, "commit", "-m", "document public install path")

    assert not any(
        "Windows absolute path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_public_analysis_note_is_not_private_diagnostic(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    note = repository / "analysis" / "license-note.md"
    note.parent.mkdir()
    public_example = "D:\\" + "Projects\\Example\\Plugin.aex"
    note.write_text(public_example, encoding="utf-8")
    git(repository, "add", "analysis/license-note.md")
    git(repository, "commit", "-m", "add public analysis note")

    assert not any(
        "Windows absolute path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_symlink_target_is_always_scanned_for_personal_paths(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    target = "/home/alice/private/AEXCompat"
    oid = subprocess.run(
        ["git", "hash-object", "-w", "--stdin"],
        cwd=repository,
        input=target,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()
    subprocess.run(
        ["git", "update-index", "--add", "--cacheinfo", "120000", oid, "checkout-link"],
        cwd=repository,
        check=True,
    )
    git(repository, "commit", "-m", "add checkout symlink")

    assert any(
        "Linux user path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_newline_diagnostic_path_and_private_email_are_scanned(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    payload = (
        b'{"path":"/home/alice/private/AEXCompat",'
        b'"email":"alice@workstation.local"}'
    )
    oid = subprocess.run(
        ["git", "hash-object", "-w", "--stdin"],
        cwd=repository,
        input=payload,
        capture_output=True,
        check=True,
    ).stdout.strip().decode("ascii")
    tree_oid = subprocess.run(
        ["git", "mktree", "-z"],
        cwd=repository,
        input=f"100644 blob {oid}\tdiagnostic\n.json\0".encode(),
        capture_output=True,
        check=True,
    ).stdout.strip().decode("ascii")
    commit_oid = subprocess.run(
        ["git", "commit-tree", tree_oid],
        cwd=repository,
        input=b"add unusual diagnostic\n",
        capture_output=True,
        check=True,
    ).stdout.strip().decode("ascii")
    git(repository, "update-ref", "refs/heads/main", commit_oid)

    findings = public_export.scan_export(repository)

    assert any("Linux user path in reachable blob" in item for item in findings)
    assert any("private email in reachable blob" in item for item in findings)


@pytest.mark.parametrize("metadata_name", [".gitmodules", ".mailmap"])
def test_repository_metadata_is_scanned_for_personal_paths(tmp_path, metadata_name):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    private_url = "/" + "home/alice/private/dep"
    (repository / metadata_name).write_text(f"url = {private_url}\n", encoding="utf-8")
    git(repository, "add", metadata_name)
    git(repository, "commit", "-m", "add repository metadata")

    assert any(
        "Linux user path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


@pytest.mark.parametrize(
    "source_name", ["probe.cpp", "plugin.csproj", "probe.rs", "runner.jsx"]
)
def test_path_bearing_source_is_scanned_for_personal_paths(tmp_path, source_name):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    private_path = "C:/" + "Users/alice/private/AEXCompat"
    (repository / source_name).write_text(private_path, encoding="utf-8")
    git(repository, "add", source_name)
    git(repository, "commit", "-m", "add path-bearing source")

    assert any(
        "Windows user path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_project_file_is_scanned_for_arbitrary_absolute_paths(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    path = repository / "plugin.csproj"
    path.write_text("D:/" + "Projects/alice/private/AEXCompat", encoding="utf-8")
    git(repository, "add", path.name)
    git(repository, "commit", "-m", "add project path")

    assert any(
        "Windows absolute path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_tool_python_source_is_scanned_for_personal_paths(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    source = repository / "tools" / "probe.py"
    source.parent.mkdir()
    private_path = "C:/" + "Users/alice/private/AEXCompat"
    source.write_text(f"ROOT = {private_path!r}\n", encoding="utf-8")
    git(repository, "add", "tools/probe.py")
    git(repository, "commit", "-m", "add tool source")

    assert any(
        "Windows user path in reachable blob" in item
        for item in public_export.scan_export(repository)
    )


def test_scan_discards_oversized_blob_in_bounded_chunks(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    (repository / "large.bin").write_bytes(b"x" * (8 * 1024 * 1024 + 1))
    git(repository, "add", "large.bin")
    git(repository, "commit", "-m", "large fixture")

    findings = public_export.scan_export(repository)

    assert any("oversized reachable blob" in item for item in findings)


def test_scan_discards_oversized_tag_metadata_in_bounded_chunks(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "test@example.invalid")
    (repository / "README.md").write_text("public\n", encoding="utf-8")
    git(repository, "add", "README.md")
    git(repository, "commit", "-m", "fixture")
    message = repository / "large-tag-message.txt"
    message.write_bytes(b"x" * (8 * 1024 * 1024 + 1))
    git(repository, "tag", "-a", "large-tag", "-F", str(message))

    findings = public_export.scan_export(repository)

    assert any("oversized reachable tag" in item for item in findings)


def test_tag_identity_collection_discards_oversized_messages(tmp_path):
    repository = tmp_path / "repository"
    repository.mkdir()
    git(repository, "init", "-b", "main")
    git(repository, "config", "user.name", "Test")
    git(repository, "config", "user.email", "tagger@workstation.local")
    (repository / "README.md").write_text("public\n", encoding="utf-8")
    git(repository, "add", "README.md")
    git(repository, "commit", "-m", "base")
    message = tmp_path / "tag-message.txt"
    message.write_text("x" * (9 * 1024 * 1024), encoding="utf-8")
    git(repository, "tag", "-a", "large", "-F", str(message))

    assert public_export.reachable_tag_identities(repository) == [
        ("Test", "tagger@workstation.local")
    ]


def test_script_has_no_push_implementation():
    source = (ROOT / "tools/public_export.py").read_text(encoding="utf-8")
    assert '["git", "push"' not in source
