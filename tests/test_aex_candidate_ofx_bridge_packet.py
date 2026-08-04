import importlib.util
import json
import os
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_candidate_ofx_bridge_packet = load_tool("aex_candidate_ofx_bridge_packet")


SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
    "aex_file_hashed": False,
    "aex_file_copied": False,
    "aepx_file_modified": False,
    "aep_binary_modified": False,
    "ae_project_write_performed": False,
    "ofx_plugin_built": False,
    "ofx_describe_performed": False,
    "ofx_render_performed": False,
    "aex_render_performed": False,
    "render_validation_performed": False,
    "pipl_payload_parsed": False,
    "parameter_schema_emitted": False,
    "redacted_schema_emitted": False,
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
}

BLOCKED = [
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "route_through_real_ofx",
    "build_ofx_binary",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "route_through_ofx",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
]


def compat_card_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_compatibility_card",
        "compatibility_card_state": "candidate_compatibility_card_ready_no_load",
        "compatibility_card_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "unsafe_exports_present": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "fixture_approval_satisfied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "real_render_open": False,
        "real_route_open": False,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def image_mock_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_image_compat_mock",
        "mock_state": "candidate_image_compat_mock_passed_no_load",
        "mock_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "source_compatibility_card_state": "candidate_compatibility_card_ready_no_load",
        "source_native_load_gate": "closed",
        "source_real_render_open": False,
        "source_real_route_open": False,
        "source_path_acceptance_ready": False,
        "source_aex_path_acceptance_enabled": False,
        "source_fixture_approval_satisfied": False,
        "source_absolute_ppm_paths_exported": False,
        "source_absolute_aex_paths_exported": False,
        "input_ppm_relative": "target/ppm-fixtures/in.ppm",
        "output_ppm_relative": "target/candidate-image-compat-mock/out.ppm",
        "input_ppm_absolute_path_exported": False,
        "output_ppm_absolute_path_exported": False,
        "operation": "invert",
        "transform_check": {
            "width": 16,
            "height": 12,
            "bytes": 576,
            "pixel_match_expected": True,
            "dimension_match_expected": True,
            "input_dimension_match": True,
        },
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def facade_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "facade_state": "deferred_loader_not_ready",
        "ofx_route_action": "no_op",
        "primary_review_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "mapping_plan": {"state": "planning_only"},
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def route_contract_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        "route_contract": {
            "allowed_route": "no_op_identity_only",
            "mock_route_ready": True,
            "real_route_open": False,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
        },
        "blockers": ["load_gate_closed", "no_ofx_host_runtime"],
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def write_json(root_name: str, name: str, payload: dict) -> Path:
    root = LAB_ROOT / "target" / root_name
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


class AexCandidateOfxBridgePacketTests(unittest.TestCase):
    def test_builds_bridge_packet_from_closed_no_load_evidence(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        card_path = LAB_ROOT / "target" / "candidate-compat-card" / f"ae-candidate-compat-card-{stamp}.local.json"
        image_path = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"ae-candidate-image-compat-mock-{stamp}.local.json"
        facade_path = LAB_ROOT / "target" / "ofx-facade" / f"ae-ofx-facade-{stamp}.local.json"
        route_path = LAB_ROOT / "target" / "ofx-route-contract" / f"ae-ofx-route-contract-{stamp}.local.json"
        for path in (card_path, image_path, facade_path, route_path):
            path.parent.mkdir(parents=True, exist_ok=True)
        packet = aex_candidate_ofx_bridge_packet.build_bridge_packet(
            card=compat_card_payload(),
            card_path=card_path,
            image_mock=image_mock_payload(),
            image_mock_path=image_path,
            facade=facade_payload(),
            facade_path=facade_path,
            route_contract=route_contract_payload(),
            route_contract_path=route_path,
        )

        self.assertEqual(packet["report_kind"], "aex_candidate_ofx_bridge_packet")
        self.assertEqual(packet["bridge_state"], "candidate_ofx_bridge_ready_no_load_route_closed")
        self.assertTrue(packet["bridge_ready"])
        self.assertEqual(packet["candidate_relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(packet["image_surface"]["operation"], "invert")
        self.assertEqual(packet["ofx_bridge_plan"]["allowed_route"], "no_op_identity_only")
        self.assertFalse(packet["ofx_bridge_plan"]["real_route_open"])
        self.assertFalse(packet["ofx_bridge_plan"]["ofx_runtime_invoked"])
        self.assertFalse(packet["absolute_ppm_paths_exported"])
        self.assertFalse(packet["absolute_aex_paths_exported"])
        self.assertFalse(packet["ofx_route_invoked"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertFalse(packet["pipl_payload_parsed"])

    def test_rejects_open_real_route_or_candidate_mismatch(self):
        route = route_contract_payload()
        route["real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_bridge_packet.build_bridge_packet(
                card=compat_card_payload(),
                card_path=Path("target/candidate-compat-card/card.json"),
                image_mock=image_mock_payload(),
                image_mock_path=Path("target/candidate-image-compat-mock/mock.json"),
                facade=facade_payload(),
                facade_path=Path("target/ofx-facade/facade.json"),
                route_contract=route,
                route_contract_path=Path("target/ofx-route-contract/route.json"),
            )

        image = image_mock_payload()
        image["candidate_relative_path"] = "Other.aex"
        with self.assertRaises(ValueError):
            aex_candidate_ofx_bridge_packet.build_bridge_packet(
                card=compat_card_payload(),
                card_path=Path("target/candidate-compat-card/card.json"),
                image_mock=image,
                image_mock_path=Path("target/candidate-image-compat-mock/mock.json"),
                facade=facade_payload(),
                facade_path=Path("target/ofx-facade/facade.json"),
                route_contract=route_contract_payload(),
                route_contract_path=Path("target/ofx-route-contract/route.json"),
            )

    def test_rejects_absolute_ppm_path_exports(self):
        image = image_mock_payload()
        image["output_ppm_relative"] = "D:\\secret\\out.ppm"
        with self.assertRaises(ValueError):
            aex_candidate_ofx_bridge_packet.build_bridge_packet(
                card=compat_card_payload(),
                card_path=Path("target/candidate-compat-card/card.json"),
                image_mock=image,
                image_mock_path=Path("target/candidate-image-compat-mock/mock.json"),
                facade=facade_payload(),
                facade_path=Path("target/ofx-facade/facade.json"),
                route_contract=route_contract_payload(),
                route_contract_path=Path("target/ofx-route-contract/route.json"),
            )

    def test_loads_sources_and_writes_json_create_new_under_bridge_root(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        card_path = write_json("candidate-compat-card", f"ae-candidate-compat-card-{stamp}.local.json", compat_card_payload())
        image_path = write_json(
            "candidate-image-compat-mock",
            f"ae-candidate-image-compat-mock-{stamp}.local.json",
            image_mock_payload(),
        )
        facade_path = write_json("ofx-facade", f"ae-ofx-facade-{stamp}.local.json", facade_payload())
        route_path = write_json("ofx-route-contract", f"ae-ofx-route-contract-{stamp}.local.json", route_contract_payload())
        card, resolved_card = aex_candidate_ofx_bridge_packet.load_candidate_compatibility_card(
            Path("target") / "candidate-compat-card" / card_path.name
        )
        image, resolved_image = aex_candidate_ofx_bridge_packet.load_candidate_image_mock(
            Path("target") / "candidate-image-compat-mock" / image_path.name
        )
        facade, resolved_facade = aex_candidate_ofx_bridge_packet.load_ofx_facade(
            Path("target") / "ofx-facade" / facade_path.name
        )
        route, resolved_route = aex_candidate_ofx_bridge_packet.load_ofx_route_contract(
            Path("target") / "ofx-route-contract" / route_path.name
        )
        packet = aex_candidate_ofx_bridge_packet.build_bridge_packet(
            card=card,
            card_path=resolved_card,
            image_mock=image,
            image_mock_path=resolved_image,
            facade=facade,
            facade_path=resolved_facade,
            route_contract=route,
            route_contract_path=resolved_route,
        )
        out = LAB_ROOT / "target" / "candidate-ofx-bridge" / f"ae-candidate-ofx-bridge-{stamp}.local.json"
        written = aex_candidate_ofx_bridge_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_bridge_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_bridge_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-bridge.json", packet
            )


if __name__ == "__main__":
    unittest.main()
