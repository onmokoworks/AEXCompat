from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
README = (ROOT / "README.md").read_text(encoding="utf-8")
README_FLAT = " ".join(README.split())
README_EN = (ROOT / "README.en.md").read_text(encoding="utf-8")
README_EN_FLAT = " ".join(README_EN.split())
BUILD_REQUIREMENTS = (ROOT / "docs" / "BUILD_REQUIREMENTS.md").read_text(
    encoding="utf-8"
)


def test_readme_pins_current_corpus_scope_without_overstating_coverage():
    assert "27 rendered / 4 unsupported_import / 8 worker_exit" in README
    assert "27 rendered / 4 unsupported_import / 8 worker_exit" in README_EN
    for document in (README, README_EN):
        assert document.count("b99dc6bad11d8dd38fd4ed54f6ba020c431dc614") == 1
        assert document.count(
            "6581854c2d8eaa015f7c79146b424e29818c91ed4cc1db478124fb75f4b1213d"
        ) == 1
        assert (
            document.count(
                "53221497a11227f2f5df7db6afdc245ccba08a91e3f437620107d0bd2f68253d"
            )
            == 1
        )
    assert "local inventory 969本全体の成功率ではありません" in README_FLAT
    assert (
        "This is not a success rate for the full local inventory of 969"
        in README_EN_FLAT
    )
    assert "残る930本はこのrunに含めておらず" in README_FLAT
    assert (
        "rendered` does not imply After Effects pixel equivalence"
        in README_EN_FLAT
    )


def test_readme_platform_and_sdk_boundaries_match_public_behavior():
    assert "Apple Silicon Macではarm64 Unicorn worker" in README
    assert "arm64 Unicorn worker for Windows x64 guest execution" in README_EN
    assert "Windows native workerの集計ではありません" in README
    assert "not the Windows native worker" in README_EN
    assert "desktop harness + native C++/MSVC worker" in README
    assert "Desktop harness + native C++/MSVC worker" in README_EN
    assert "公開repositoryとforkのCIはsource-only" in README_FLAT
    assert (
        "Public-repository and fork CI runs validate the source-only scope"
        in README_EN_FLAT
    )
    assert "private GitHub release asset" not in README + README_EN
    assert "private repositoryにおける同一repositoryのPR" in BUILD_REQUIREMENTS
    assert "Same-repository runs while the repository is private" in BUILD_REQUIREMENTS


def test_readme_language_entry_points_are_separate_and_linked():
    assert "[English](README.en.md)" in README
    assert "[日本語](README.md)" in README_EN
    assert "## English" not in README
