//! Generated x86_64 Windows After Effects ABI constants.
//!
//! This crate deliberately contains no Rust layout recreation. The guest uses
//! byte buffers plus offsets from the same compiled SDK observation as the
//! native minihost.

#![forbid(unsafe_code)]

pub mod x86_64_windows {
    include!("generated.rs");
}

#[cfg(test)]
mod tests {
    use super::x86_64_windows as abi;

    #[test]
    fn gpu_smartfx_layout_is_inside_observed_structures() {
        for (offset, size, container) in [
            (
                abi::PRE_INPUT_GPU_DATA_OFFSET,
                abi::PRE_INPUT_GPU_DATA_SIZE,
                abi::PF_PRE_RENDER_INPUT_SIZE,
            ),
            (
                abi::PRE_INPUT_WHAT_GPU_OFFSET,
                abi::PRE_INPUT_WHAT_GPU_SIZE,
                abi::PF_PRE_RENDER_INPUT_SIZE,
            ),
            (
                abi::PRE_INPUT_DEVICE_INDEX_OFFSET,
                abi::PRE_INPUT_DEVICE_INDEX_SIZE,
                abi::PF_PRE_RENDER_INPUT_SIZE,
            ),
            (
                abi::PRE_OUTPUT_FLAGS_OFFSET,
                abi::PRE_OUTPUT_FLAGS_SIZE,
                abi::PF_PRE_RENDER_OUTPUT_SIZE,
            ),
            (
                abi::PRE_OUTPUT_PRE_RENDER_DATA_OFFSET,
                abi::PRE_OUTPUT_PRE_RENDER_DATA_SIZE,
                abi::PF_PRE_RENDER_OUTPUT_SIZE,
            ),
            (
                abi::SMART_INPUT_GPU_DATA_OFFSET,
                abi::SMART_INPUT_GPU_DATA_SIZE,
                abi::PF_SMART_RENDER_INPUT_SIZE,
            ),
            (
                abi::SMART_INPUT_WHAT_GPU_OFFSET,
                abi::SMART_INPUT_WHAT_GPU_SIZE,
                abi::PF_SMART_RENDER_INPUT_SIZE,
            ),
            (
                abi::SMART_INPUT_DEVICE_INDEX_OFFSET,
                abi::SMART_INPUT_DEVICE_INDEX_SIZE,
                abi::PF_SMART_RENDER_INPUT_SIZE,
            ),
        ] {
            assert!(offset + size <= container);
        }
    }

    #[test]
    fn gpu_selector_and_framework_values_match_the_observed_windows_contract() {
        assert_eq!(abi::PF_CMD_SMART_PRE_RENDER, 23);
        assert_eq!(abi::PF_CMD_SMART_RENDER, 24);
        assert_eq!(abi::PF_CMD_SMART_RENDER_GPU, 31);
        assert_eq!(abi::PF_CMD_GPU_DEVICE_SETUP, 32);
        assert_eq!(abi::PF_CMD_GPU_DEVICE_SETDOWN, 33);
        assert_eq!(abi::PF_GPU_FRAMEWORK_OPENCL, 1);
        assert_eq!(abi::PF_RENDER_OUTPUT_FLAG_GPU_RENDER_POSSIBLE, 2);
    }
}
