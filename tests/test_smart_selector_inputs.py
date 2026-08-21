"""SmartRender must be handed the same request and bitdepth PreRender got.

The dispatch built `PF_SmartRenderInput` with its whole leading
`PF_RenderRequest` and its `bitdepth` zeroed while `PF_PreRenderInput` carried
the real values, so a SmartFX plug-in that selects its pixel loop from the
bitdepth had no case to take (issue #699).

Two halves, because either alone proves little:

* the native self-test builds both selector inputs through the one builder the
  dispatch uses and compares them byte for byte over the shared prefix, so
  dropping the SmartRender half fails it;
* the offsets that self-test reports are compared here against the frozen ABI
  observation taken from the real SDK headers, so the constants cannot merely
  agree with themselves.
"""

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"
ABI_OBSERVATION = ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json"
WORKER = BUILD / "aex_worker.exe"
KINDS = ("discovery", "classic", "smart")


def _self_test(kind: str) -> dict:
    completed = subprocess.run(
        [str(WORKER), "--kind", kind, "--self-test-smart-selector-inputs"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    return json.loads(completed.stdout)


def test_native_selector_input_self_test_passes_all_three_workers() -> None:
    assert WORKER.exists(), "build the worker with tools\\build-native.ps1 first"
    for kind in KINDS:
        report = _self_test(kind)
        assert report["smart_selector_inputs"] == "passed"
        assert report["pr_gpu_pf_first_all_depths"] is True


def test_reported_offsets_match_the_frozen_sdk_abi_observation() -> None:
    fields = json.loads(ABI_OBSERVATION.read_text(encoding="utf-8"))["fields"]
    request = fields["pre_input.output_request"]
    bitdepth = fields["smart_input.bitdepth"]
    pre_render_data = fields["smart_input.pre_render_data"]
    # PF_RenderRequest leads PF_PreRenderInput, so its recorded size is the
    # prefix both selector inputs share, and PF_SmartRenderInput's bitdepth sits
    # immediately after it.
    assert request["offset"] == 0
    assert bitdepth["offset"] == request["size"]

    assert WORKER.exists(), "build the worker with tools\\build-native.ps1 first"
    for kind in KINDS:
        reported = _self_test(kind)
        assert reported["render_request_bytes"] == request["size"]
        assert reported["bitdepth_offset"] == bitdepth["offset"]
        assert reported["pre_render_data_offset"] == pre_render_data["offset"]
        # PF_Field then PF_ChannelMask follow the 16-byte PF_LRect.
        assert reported["field_offset"] == 16
        assert reported["channel_mask_offset"] == 20
        assert reported["channel_mask_offset"] + 4 <= reported["render_request_bytes"]
