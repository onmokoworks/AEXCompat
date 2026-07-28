from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_alpha_mode_is_wired_to_primary_and_all_secondary_transports():
    text = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    assert text.count("apply_conformance_premultiplication(&mut rgba, mode);") == 2
    assert text.count("apply_conformance_premultiplication(&mut layer_rgba, mode);") == 1

    primary = text.index("let mut rgba = decoded.into_rgba8().into_raw();")
    ordinary = text.index("let mut layer_rgba = decoded.into_rgba8().into_raw();")
    timed = text.index(
        "let mut rgba = decoded.into_rgba8().into_raw();",
        ordinary,
    )
    # The pre-transform has to happen before the pixels leave for the worker.
    # That used to be the one-shot's `.write_all(&rgba)?` into a raw sidecar;
    # #365 deleted it, and the session hands the same buffer over by reference
    # in the wrapper request instead.
    transport_handoff = text.index("rgba: &rgba,")
    assert primary < ordinary < timed < transport_handoff

