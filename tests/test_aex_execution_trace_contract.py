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
        "selector": "PARAMS_SETUP",
        "entry_rva": 0xA970,
        "return_value": 0,
        "truncated": False,
        "events": [
            {
                "sequence": 0,
                "depth": 0,
                "kind": "selector_enter",
                "pc_rva": 0xA970,
                "name": "PARAMS_SETUP",
            },
            {
                "sequence": 1,
                "depth": 0,
                "kind": "host_callback",
                "name": "add_param",
            },
            {
                "sequence": 2,
                "depth": 0,
                "kind": "selector_exit",
                "pc_rva": 0xA970,
                "name": "return=0x0",
            },
        ],
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
