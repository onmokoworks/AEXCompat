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




