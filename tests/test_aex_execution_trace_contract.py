import json
from pathlib import Path

from jsonschema import Draft202012Validator


ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads(
    (ROOT / "contracts/aex/aex_execution_trace.schema.json").read_text(encoding="utf-8")
)


def test_execution_trace_contract_accepts_ordered_selector_timeline():
    report = {
        "schema": "aexcompat.aex-execution-trace",
        "schema_version": 1,
        "execution_backend": "unicorn-x86_64",
        "image_sha256": "0" * 64,
        "preferred_image_base": 0x180000000,
        "entry_export": "EffectMain",
        "selector": "PARAMS_SETUP",
        "entry_rva": 0xA970,
        "return_value": 0,
        "truncated": False,
        "events": [
            {
                "sequence": 0,
                "observed_count": 1,
                "depth": 0,
                "kind": "selector_enter",
                "function_rva": 0xA970,
                "pc_rva": 0xA970,
                "name": "PARAMS_SETUP",
            },
            {
                "sequence": 1,
                "observed_count": 1,
                "depth": 0,
                "kind": "host_callback",
                "function_rva": 0xA970,
                "name": "add_param",
                "arguments": [
                    {
                        "register": "rcx",
                        "raw": 0x40001000,
                        "classification": "guest_data",
                        "offset": 0x1000,
                    }
                ],
            },
            {
                "sequence": 2,
                "observed_count": 1,
                "depth": 0,
                "kind": "selector_exit",
                "function_rva": 0xA970,
                "pc_rva": 0xA970,
                "name": "return=0x0",
            },
        ],
        "functions": [
            {
                "entry_rva": 0xA970,
                "observed_calls": 1,
                "observed_returns": 1,
                "callees": [],
                "imports": [],
                "host_callbacks": ["add_param"],
                "entry_bytes": "48895c2408",
            }
        ],
        "state_changes": [],
        "timeline": [
            "00000 selector_enter rva=0xa970 PARAMS_SETUP",
            "00001 host_callback add_param",
            "00002 selector_exit rva=0xa970 return=0x0",
        ],
    }

    Draft202012Validator(SCHEMA).validate(report)
    assert [event["sequence"] for event in report["events"]] == list(
        range(len(report["events"]))
    )
    assert len(report["timeline"]) == len(report["events"])
