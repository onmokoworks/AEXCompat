#![cfg(target_os = "macos")]

use aex_wgpu_compute::{
    BindingAccess, BufferBinding, DispatchDescriptor, Error, ObjectCounts, Session,
};

const ADD_SHADER: &str = r#"
@group(0) @binding(0)
var<storage, read> input_values: array<u32>;

@group(0) @binding(1)
var<storage, read_write> output_values: array<u32>;

@compute @workgroup_size(4)
fn add_and_scale(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < arrayLength(&input_values)) {
        output_values[id.x] = input_values[id.x] * 3u + 7u;
    }
}
"#;

const READ_ONLY_SHADER: &str = r#"
@group(0) @binding(0)
var<storage, read> input_values: array<u32>;

@compute @workgroup_size(1)
fn inspect(@builtin(global_invocation_id) id: vec3<u32>) {
    _ = input_values[id.x];
}
"#;

fn encode_u32(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_u32(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four byte chunk")))
        .collect()
}

#[test]
fn metal_dispatch_uploads_computes_reads_back_and_cleans_up() {
    let session = Session::new_metal().expect("Apple Silicon should expose a Metal adapter");
    assert_eq!(session.adapter_report().backend, "Metal");
    assert!(!session.adapter_report().name.is_empty());
    assert_eq!(session.live_objects(), ObjectCounts::default());

    let input = encode_u32(&[0, 1, 2, 5, 13, 21, 34, 55]);
    let output = vec![0u8; input.len()];
    let bindings = [
        BufferBinding {
            binding: 0,
            access: BindingAccess::ReadOnlyStorage,
            bytes: &input,
        },
        BufferBinding {
            binding: 1,
            access: BindingAccess::ReadWriteStorage,
            bytes: &output,
        },
    ];
    let report = session
        .dispatch(&DispatchDescriptor {
            label: Some("bounded Metal compute proof"),
            wgsl: ADD_SHADER,
            entry_point: "add_and_scale",
            bindings: &bindings,
            workgroups: [2, 1, 1],
        })
        .expect("bounded WGSL dispatch should complete");

    assert_eq!(report.adapter.backend, "Metal");
    assert_eq!(report.workgroups, [2, 1, 1]);
    assert_eq!(report.outputs.len(), 1);
    assert_eq!(report.outputs[0].binding, 1);
    assert_eq!(
        decode_u32(&report.outputs[0].bytes),
        [7, 10, 13, 22, 46, 70, 109, 172]
    );
    assert_eq!(report.created_resources.buffers, 2);
    assert_eq!(report.created_resources.staging_buffers, 1);
    assert!(report.live_resources.is_zero());
    assert!(session.live_objects().is_zero());
}

#[test]
fn invalid_wgsl_fails_closed_and_releases_dispatch_resources() {
    let session = Session::new_metal().expect("Apple Silicon should expose a Metal adapter");
    let output = [0u8; 4];
    let bindings = [BufferBinding {
        binding: 0,
        access: BindingAccess::ReadWriteStorage,
        bytes: &output,
    }];
    let error = session
        .dispatch(&DispatchDescriptor {
            label: Some("invalid bounded Metal compute proof"),
            wgsl: "this is not WGSL",
            entry_point: "main",
            bindings: &bindings,
            workgroups: [1, 1, 1],
        })
        .expect_err("invalid WGSL must not be treated as a successful dispatch");

    assert!(matches!(error, Error::Validation(_)));
    assert!(session.live_objects().is_zero());

    let read_only_bindings = [BufferBinding {
        binding: 0,
        access: BindingAccess::ReadOnlyStorage,
        bytes: &output,
    }];
    let report = session
        .dispatch(&DispatchDescriptor {
            label: Some("read-only completion proof"),
            wgsl: READ_ONLY_SHADER,
            entry_point: "inspect",
            bindings: &read_only_bindings,
            workgroups: [1, 1, 1],
        })
        .expect("validation errors must not poison later bounded dispatches");
    assert!(report.outputs.is_empty());
    assert!(report.live_resources.is_zero());
    assert!(session.live_objects().is_zero());
}
