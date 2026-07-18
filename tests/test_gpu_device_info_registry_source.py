from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "gpu_device_info_registry.hpp"
SOURCE = ROOT / "minihost" / "src" / "gpu_device_info_registry.cpp"
MAIN = ROOT / "minihost" / "src" / "l2_main.cpp"


def test_gpu_device_info_registry_preserves_bounded_abi_contract():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")

    assert "kMaxGpuDevices = 16" in header
    assert "uint32_t device_count_{1}" in header
    assert "int32_t framework_{3}" in header
    for marker in (
        "sizeof(DeviceInfoAbi) == 56",
        "offsetof(DeviceInfoAbi, framework) == 0",
        "offsetof(DeviceInfoAbi, compatible) == 4",
        "offsetof(DeviceInfoAbi, platform) == 8",
        "offsetof(DeviceInfoAbi, device) == 16",
        "offsetof(DeviceInfoAbi, context) == 24",
        "offsetof(DeviceInfoAbi, queue) == 32",
        "count == 0 || count > kMaxGpuDevices",
        "if (index >= kMaxGpuDevices) return false",
    ):
        assert marker in source


def test_gpu_device_info_callbacks_and_reset_keep_existing_protocol():
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")

    assert '"stage:gpu_device_info_begin\\n"' in source
    assert '"stage:gpu_device_info_end error=0\\n"' in source
    assert source.index("if (error != 0) return error") < source.index(
        '"stage:gpu_device_info_end error=0\\n"'
    )
    reset = source[source.index("void DeviceInfoRegistry::reset_devices()") :]
    reset = reset[: reset.index("\n}")]
    assert "device_count_ = 1" in reset
    assert "device = {}" in reset
    assert "framework_" not in reset
    assert "int32_t __cdecl gpu_get_device_count" not in main
    assert "int32_t __cdecl gpu_get_device_info" not in main
    assert "device_info_registry().reset_devices()" in main
