import importlib.util
import hashlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "generate-third-party-licenses.py"


def load_generator():
    spec = importlib.util.spec_from_file_location("third_party_licenses", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_license_snapshot_excludes_local_crates_and_preserves_notices() -> None:
    generator = load_generator()
    raw = {
        "crates": [
            {
                "package": {
                    "name": "aexcompat-harness",
                    "version": "0.1.0",
                    "source": None,
                    "license": "MPL-2.0",
                    "repository": None,
                }
            },
            {
                "package": {
                    "name": "mit-dep",
                    "version": "1.0.0",
                    "source": "registry+index",
                    "license": "MIT",
                    "repository": "https://example.invalid/mit",
                }
            },
            {
                "package": {
                    "name": "apache-dep",
                    "version": "2.0.0",
                    "source": "registry+index",
                    "license": "Apache-2.0",
                    "repository": None,
                }
            },
        ],
        "licenses": [
            {
                "name": "MIT",
                "text": "Copyright holder\nPermission notice",
                "used_by": [
                    {
                        "crate": {
                            "name": "mit-dep",
                            "version": "1.0.0",
                            "source": "registry+index",
                        }
                    }
                ],
            },
            {
                "name": "Apache-2.0",
                "text": "Apache text\nNOTICE <keep>",
                "used_by": [
                    {
                        "crate": {
                            "name": "apache-dep",
                            "version": "2.0.0",
                            "source": "registry+index",
                        }
                    }
                ],
            },
            {
                "name": "MPL-2.0",
                "text": "project license",
                "used_by": [
                    {
                        "crate": {
                            "name": "aexcompat-harness",
                            "version": "0.1.0",
                            "source": None,
                        }
                    }
                ],
            },
        ],
    }

    snapshot = generator.compact_cargo_about(raw)
    assert [
        (package["name"], package["version"]) for package in snapshot["packages"]
    ] == [
        ("apache-dep", "2.0.0"),
        ("mit-dep", "1.0.0"),
    ]
    rendered = generator.render(snapshot)
    rendered_text = generator.render_text(snapshot)
    assert "aexcompat-harness" not in rendered
    assert "Copyright holder" in rendered
    assert "Apache text" in rendered
    assert "NOTICE &lt;keep&gt;" in rendered
    assert "project license" not in rendered
    assert "Copyright holder" in rendered_text
    assert "NOTICE <keep>" in rendered_text
    # The lockfile as text: a core.autocrlf=true checkout (the GitHub Windows
    # runner default) holds it with CRLF, and d41ca02e made the generator hash
    # it with normalized line endings for exactly that reason.
    assert (
        snapshot["cargo_lock_sha256"]
        == hashlib.sha256(
            generator.LOCKFILE.read_bytes().replace(b"\r\n", b"\n")
        ).hexdigest()
    )
