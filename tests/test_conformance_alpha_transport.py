from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_alpha_mode_is_wired_to_primary_and_all_secondary_transports():
    text = SOURCE.read_text(encoding="utf-8")
    assert text.count("apply_conformance_premultiplication(&mut rgba, mode);") == 2
    assert text.count("apply_conformance_premultiplication(&mut layer_rgba, mode);") == 1

    primary = text.index("let mut rgba = decoded.into_rgba8().into_raw();")
    ordinary = text.index("let mut layer_rgba = decoded.into_rgba8().into_raw();")
    timed = text.index(
        "let mut rgba = decoded.into_rgba8().into_raw();",
        ordinary,
    )
    transport_write = text.index(".write_all(&rgba)?;")
    assert primary < ordinary < timed < transport_write

