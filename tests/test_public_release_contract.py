from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_public_documents_warn_against_private_and_proprietary_submissions():
    paths = [
        ROOT / "README.md",
        ROOT / "SECURITY.md",
        ROOT / "CONTRIBUTING.md",
        ROOT / ".github/ISSUE_TEMPLATE/bug_report.yml",
    ]
    for path in paths:
        text = path.read_text(encoding="utf-8").lower()
        assert "aex" in text, path
        assert "sdk" in text, path
        assert "dump" in text, path
        assert "path" in text, path


def test_worker_cwd_is_not_the_repository_root_and_transport_pin_follows_it():
    launch = (ROOT / "broker/crates/broker/src/secure_launch.rs").read_text(encoding="utf-8")
    transport = (ROOT / "minihost/src/parameter_animation_transport.cpp").read_text(
        encoding="utf-8"
    )
    assert 'let worker_cwd = repository.join("target")' in launch
    assert 'let worker_cwd = request.repository.join("target")' in launch
    assert 'std::filesystem::current_path() / "image-transport"' in transport
    assert 'std::filesystem::current_path() / "target" / "image-transport"' not in transport


def test_existing_worker_bounds_and_timeout_tree_kill_remain_explicit():
    process = (ROOT / "broker/crates/broker/src/windows_process.rs").read_text(encoding="utf-8")
    secure_test = (ROOT / "broker/crates/broker/tests/secure_launch.rs").read_text(
        encoding="utf-8"
    )
    for contract in (
        "JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE",
        "JOB_OBJECT_LIMIT_PROCESS_MEMORY",
        "TerminateJobObject",
        "stdout_truncated",
        "stderr_truncated",
    ):
        assert contract in process
    assert "timeout_kills_worker_and_cleans_sealed_and_staged_trees" in secure_test
    assert "timeout_kills_worker_descendant_process_too" in secure_test
