import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GLATOR_OPENGL_SMART_RESULT_2026-07-15.json"
PREPARE = ROOT / "tools" / "sdk-fixtures" / "prepare-glator-source.ps1"
PROPS = ROOT / "tools" / "sdk-fixtures" / "glator-safe.props"


def test_glator_safety_patch_is_hash_bound_and_does_not_modify_the_sdk():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    boundary = result["build_boundary"]
    prepare = PREPARE.read_text(encoding="utf-8")
    props = PROPS.read_text(encoding="utf-8")

    assert boundary["installed_sdk_modified"] is False
    assert boundary["reviewed_source_hash_required"] is True
    assert result["reviewed_sdk_gl_base_sha256"].upper() in prepare
    assert "new unsigned char[fileLength + 1]" in prepare
    assert "delete[] vertexShaderAssemblyP" in prepare
    assert "delete[] fragmentShaderAssemblyP" in prepare
    assert "fclose(fileP)" in prepare
    assert 'ClCompile Remove="..\\GL_base.cpp"' in props


def test_glator_platform_and_stdout_boundaries_are_enforced():
    callbacks = json.loads(RESULT.read_text(encoding="utf-8"))["host_callbacks"]

    assert callbacks["platform_data_path_is_absolute_and_bounded"] is True
    assert callbacks["get_pixel_data8_depth_checked"] is True
    assert callbacks["get_pixel_data16_depth_checked"] is True
    assert callbacks["native_stdout_isolated_from_json_protocol"] is True
    assert callbacks["worker_stdout_json_line_count"] == 1


def test_glator_opengl_lifecycle_and_all_cpu_depth_outputs_match():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    gl = result["opengl"]
    render = result["render"]

    assert result["result"] == "opengl_smart_image_io_completed"
    for error in ("global_setup_error", "smart_pre_render_error", "smart_render_error", "global_setdown_error"):
        assert gl[error] == 0
    assert render["expected_green_rgba8"] == 64
    assert render["argb8_oracle_exact"] is True
    assert render["argb16_oracle_exact"] is True
    assert render["argb32f_oracle_exact"] is True
    for invariant in (
        "suite_leases_balanced",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "parameter_checkouts_balanced",
        "guard_bytes_intact",
    ):
        assert render[invariant] is True
