from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
COMPONENT = (ROOT / "minihost" / "src" / "runtime_module_audit.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")


def test_module_audit_has_one_common_worker_component():
    assert CMAKE.count("src/runtime_module_audit.cpp") == 1
    assert '#include "runtime_module_audit.hpp"' in MAIN
    assert "EnumProcessModulesEx" not in MAIN
    assert "struct AuthorizedRuntimeModule" not in MAIN
    assert "'A','E','X','R','M','A','1',0" not in MAIN
    assert "EnumProcessModulesEx" in COMPONENT
    assert "struct AuthorizedRuntimeModule" in COMPONENT


def test_component_keeps_sensitive_paths_internal_to_nonserialized_keys():
    serializer = COMPONENT[COMPONENT.index("std::string module_audit_snapshot_json"):
                           COMPONENT.index("}  // namespace", COMPONENT.index(
                               "std::string module_audit_snapshot_json"))]
    assert "unknown_keys" not in serializer
    assert "module_path.wstring()" not in serializer
    assert "plugin_root.wstring()" not in serializer
