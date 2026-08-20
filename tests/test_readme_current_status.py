from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
README = (ROOT / "README.md").read_text(encoding="utf-8")
README_FLAT = " ".join(README.split())
BUILD_REQUIREMENTS = (ROOT / "docs" / "BUILD_REQUIREMENTS.md").read_text(
    encoding="utf-8"
)


def test_readme_pins_current_corpus_scope_without_overstating_coverage():
    assert "27 rendered / 4 unsupported_import / 8 worker_exit" in README
    assert README.count("b99dc6bad11d8dd38fd4ed54f6ba020c431dc614") == 2
    assert README.count(
        "6581854c2d8eaa015f7c79146b424e29818c91ed4cc1db478124fb75f4b1213d"
    ) == 2
    assert "local inventory 969本" in README
    assert "39-plugin tranche is not a success rate for all 969" in README
    assert "残る930本はこのrunへ含まれておらず" in README_FLAT
    assert (
        "produced an output image; it does not imply After Effects pixel equivalence"
        in README_FLAT
    )
    assert "53221497a11227f2f5df7db6afdc245ccba08a91e3f437620107d0bd2f68253d" in README


def test_readme_platform_and_sdk_boundaries_match_public_behavior():
    platform_row = (
        "| macOS Apple Silicon | Mac-local arm64 Unicorn CLI; "
        "Rosetta native carrier is optional and opt-in |"
    )
    assert README.count(platform_row) == 1
    assert "private Cloudflare R2" in README
    assert "Public-repository and fork runs do not receive those credentials" in README_FLAT
    assert "private GitHub release asset" not in README
    assert "private repositoryにおける同一repositoryのPR" in BUILD_REQUIREMENTS
    assert "Same-repository runs while the repository is private" in BUILD_REQUIREMENTS
