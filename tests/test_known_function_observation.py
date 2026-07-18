import copy
import json
import shutil
import subprocess
import unittest
from pathlib import Path

from tools.known_function_observation import (
    ResolutionError,
    build_event,
    load_offset_map,
    resolve_hook,
    resolve_spec,
    session_boundary_event,
)
from tools.trace_contract_validator import validate_session


ROOT = Path(__file__).resolve().parents[1]
OBS = ROOT / "contracts" / "observation"
EXAMPLES = OBS / "examples"


def load(name):
    return json.loads((EXAMPLES / name).read_text(encoding="utf-8"))


class HookSpecSchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((OBS / "known_function_hook_set.schema.json").read_text(encoding="utf-8"))

    def test_hook_set_schema_is_draft_2020_12(self):
        self.assertEqual("https://json-schema.org/draft/2020-12/schema", self.schema["$schema"])
        self.assertEqual(1, self.schema["schema_version"])

    def test_example_hook_set_validates_against_schema(self):
        from jsonschema import Draft202012Validator

        Draft202012Validator.check_schema(self.schema)
        validator = Draft202012Validator(self.schema)
        self.assertEqual([], list(validator.iter_errors(load("example_hook_set.json"))))

    def test_schema_rejects_absolute_module_address(self):
        from jsonschema import Draft202012Validator

        validator = Draft202012Validator(self.schema)
        spec = load("example_hook_set.json")
        spec["hooks"][0]["module_rva"] = "0x7FFABC001C40"  # uppercase hex fails the lowercase pattern
        self.assertTrue(list(validator.iter_errors(spec)))


class ResolveTests(unittest.TestCase):
    def setUp(self):
        self.spec = load("example_hook_set.json")
        self.offset_map = load("example_offset_map.json")

    def test_load_offset_map_accepts_full_map_or_fields(self):
        full = load_offset_map(self.offset_map)
        just_fields = load_offset_map(self.offset_map["fields"])
        self.assertEqual(full, just_fields)
        self.assertEqual({"offset": 260, "size": 4}, full["in.width"])

    def test_resolve_spec_produces_read_plan(self):
        plan = resolve_spec(self.spec, self.offset_map)
        self.assertEqual("known_function_read_plan", plan["plan_kind"])
        self.assertEqual("gamma-classic", plan["module_label"])
        hook = plan["hooks"][0]
        self.assertEqual(0x1C40, hook["module_rva_int"])

        enter = {r["name"]: r for r in hook["enter_reads"]}
        self.assertEqual(
            {"name": "in.width", "source": "struct", "arg_index": 0, "offset": 260,
             "size": 4, "extent": 400, "interpret": "int"},
            enter["in.width"],
        )
        # struct prefix "out" maps to arg 1 and lands in the leave phase.
        leave = {r["name"]: r for r in hook["leave_reads"]}
        self.assertEqual(1, leave["out.width"]["arg_index"])
        self.assertEqual(200, leave["out.width"]["extent"])
        # scalar arg read straight from the integer register slot, explicit width.
        self.assertEqual("register", enter["mode"]["source"])
        self.assertEqual(2, enter["mode"]["arg_index"])
        self.assertEqual(4, enter["mode"]["width"])
        self.assertEqual({"interpret": "int"}, hook["return"])

    def test_missing_offset_field_fails_loud(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["reads"].append({"name": "in.absent", "as": "int"})
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, self.offset_map)
        self.assertIn("absent from the offset map", str(ctx.exception))

    def test_read_without_arg_struct_mapping_fails(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["reads"].append({"name": "param.flags", "as": "int"})
        offset_map = copy.deepcopy(self.offset_map)
        offset_map["fields"]["param.flags"] = {"offset": 8, "size": 4}
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, offset_map)
        self.assertIn("no arg_structs mapping", str(ctx.exception))

    def test_float_scalar_arg_is_rejected_with_xmm_hint(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["scalar_args"].append({"index": 3, "name": "gain", "as": "float"})
        with self.assertRaises(ResolutionError):
            resolve_spec(spec, self.offset_map)

    def test_interpret_size_mismatch_is_rejected(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["reads"] = [{"name": "in.quality", "as": "float"}]
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, self.offset_map)
        self.assertIn("float needs size", str(ctx.exception))

    def test_absolute_module_address_rejected(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["module_rva"] = "0X1C40"  # uppercase / not module-relative form
        with self.assertRaises(ResolutionError):
            resolve_spec(spec, self.offset_map)

    def test_lowercase_absolute_address_rejected_by_bound(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["module_rva"] = "0x7ffabc001c40"  # valid shape, absolute magnitude
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, self.offset_map)
        self.assertIn("module-relative bound", str(ctx.exception))

    def test_negative_offset_in_offset_map_is_rejected(self):
        offset_map = copy.deepcopy(self.offset_map)
        offset_map["fields"]["in.width"] = {"offset": -8, "size": 4}
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(self.spec, offset_map)
        self.assertIn("non-negative", str(ctx.exception))

    def test_read_beyond_struct_extent_is_rejected(self):
        offset_map = copy.deepcopy(self.offset_map)
        # in.width sits at 260..264 but the declared in extent is 400; push it out.
        offset_map["fields"]["in.width"] = {"offset": 398, "size": 4}
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(self.spec, offset_map)
        self.assertIn("exceeds", str(ctx.exception))

    def test_arg_struct_requires_extent(self):
        spec = copy.deepcopy(self.spec)
        del spec["hooks"][0]["arg_structs"][0]["extent"]
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, self.offset_map)
        self.assertIn("extent", str(ctx.exception))

    def test_scalar_arg_requires_width(self):
        spec = copy.deepcopy(self.spec)
        del spec["hooks"][0]["scalar_args"][0]["width"]
        with self.assertRaises(ResolutionError) as ctx:
            resolve_spec(spec, self.offset_map)
        self.assertIn("width", str(ctx.exception))

    def test_arg_slot_index_is_bounded(self):
        spec = copy.deepcopy(self.spec)
        spec["hooks"][0]["scalar_args"][0]["index"] = 999
        with self.assertRaises(ResolutionError):
            resolve_spec(spec, self.offset_map)


class FormatTests(unittest.TestCase):
    kwargs = dict(
        module_label="gamma-classic",
        plugin_label="gamma-classic",
        host_version_label="native-observation frida",
    )

    def test_enter_message_becomes_valid_event(self):
        message = {
            "type": "known_function",
            "symbol": "apply_gamma",
            "module_rva": "0x1c40",
            "phase": "enter",
            "fields": [{"name": "in.width", "value": 1920}, {"name": "in.height", "value": 1080}],
        }
        event = build_event(message, event_index=1, **self.kwargs)
        self.assertEqual("known_function_invoke", event["event_kind"])
        self.assertEqual("native_observation", event["host_kind"])
        self.assertEqual(1920, event["known_function"]["fields"][0]["value"])

    def test_leave_message_carries_return_value(self):
        message = {
            "type": "known_function",
            "symbol": "apply_gamma",
            "module_rva": "0x1c40",
            "phase": "leave",
            "return_value": 0,
            "fields": [{"name": "out.width", "value": 1920}],
        }
        event = build_event(message, event_index=2, **self.kwargs)
        self.assertEqual(0, event["known_function"]["return_value"])

    def test_raw_pointer_value_is_rejected(self):
        message = {
            "type": "known_function",
            "symbol": "apply_gamma",
            "module_rva": "0x1c40",
            "phase": "enter",
            "fields": [{"name": "in.width", "value": "0x7ffabc00"}],
        }
        with self.assertRaises(ValueError):
            build_event(message, event_index=1, **self.kwargs)

    def test_absolute_path_field_name_is_rejected(self):
        message = {
            "type": "known_function",
            "symbol": "apply_gamma",
            "module_rva": "0x1c40",
            "phase": "enter",
            "fields": [{"name": "C:\\secret\\in.width", "value": 1}],
        }
        with self.assertRaises(ValueError):
            build_event(message, event_index=1, **self.kwargs)

    def test_end_to_end_session_from_messages_validates(self):
        messages = [
            {"type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
             "phase": "enter", "fields": [{"name": "in.width", "value": 1920}]},
            {"type": "known_function", "symbol": "apply_gamma", "module_rva": "0x1c40",
             "phase": "leave", "return_value": 0, "fields": [{"name": "out.width", "value": 1920}]},
        ]
        events = [session_boundary_event("session_start", event_index=0, plugin_label="gamma-classic", host_version_label="native-observation frida")]
        events += [build_event(m, event_index=i + 1, **self.kwargs) for i, m in enumerate(messages)]
        events.append(session_boundary_event("session_end", event_index=len(events), plugin_label="gamma-classic", host_version_label="native-observation frida"))
        session = {
            "schema_version": 1,
            "session_id": "abcdef01-2345-6789-abcd-ef0123456789",
            "event_count": len(events),
            "trace_complete": True,
            "events": events,
        }
        self.assertEqual([], validate_session(session))


class FridaScriptTests(unittest.TestCase):
    SCRIPT = ROOT / "tools" / "frida" / "known_function_probe.js"

    def test_script_exists_and_is_thin(self):
        text = self.SCRIPT.read_text(encoding="utf-8")
        # Thin observer contract: consumes the plan, never writes files, forwards
        # only via send(), and locates the module base itself.
        self.assertIn("recv('plan'", text)
        self.assertIn("send(", text)
        # Identity by full path (not basename) + executable-range verification.
        self.assertIn("Process.enumerateModules", text)
        self.assertIn("Process.findRangeByAddress", text)
        self.assertNotIn("writeFile", text)
        self.assertNotIn("File(", text)

    def test_script_arms_loader_watch_before_ready(self):
        text = self.SCRIPT.read_text(encoding="utf-8")
        # Spawn-suspended workers load the plug-in later, so the script must arm a
        # loader watch and signal 'ready' (safe to resume) rather than requiring the
        # module to be present up front.
        self.assertIn("LoadLibrary", text)
        self.assertIn("type: 'ready'", text)

    def test_script_reads_are_memory_safe(self):
        text = self.SCRIPT.read_text(encoding="utf-8")
        # Null-check + extent bound + read failures reported (never thrown out).
        self.assertIn("isNull()", text)
        self.assertIn("read.extent", text)
        self.assertIn("read_error", text)

    def test_script_parses_with_node(self):
        node = shutil.which("node")
        if not node:
            self.skipTest("node not available for syntax check")
        result = subprocess.run(
            [node, "--check", str(self.SCRIPT)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(0, result.returncode, result.stderr)


if __name__ == "__main__":
    unittest.main()
