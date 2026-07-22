from configparser import ConfigParser
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_root_pytest_collection_is_bounded_to_checkout_tests():
    config = ConfigParser()
    config.read(ROOT / "pytest.ini", encoding="utf-8")

    assert config.get("pytest", "testpaths").split() == ["tests"]
