#![cfg(target_os = "macos")]

use aex_wgpu_compute::{
    BindingAccess, BufferBinding, DispatchDescriptor, Error, NagaDispatchDescriptor, ObjectCounts,
    PipelineConstant, Session, ValidatedNagaModule, naga,
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

const NAGA_UNIFORM_SHADER: &str = r#"
struct Params {
    multiplier: u32,
    addend: u32,
    padding: vec2<u32>,
};

@id(7) override extra: u32 = 0u;

@group(0) @binding(0)
var<uniform> params: Params;

@group(0) @binding(1)
var<storage, read> input_values: array<u32>;

@group(0) @binding(2)
var<storage, read_write> output_values: array<u32>;

@compute @workgroup_size(4)
fn transform(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < arrayLength(&input_values)) {
        output_values[id.x] =
            input_values[id.x] * params.multiplier + params.addend + extra;
    }
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

#[test]
fn validated_naga_dispatch_uses_uniform_and_pipeline_constant() {
    let adapters = Session::enumerate_metal_adapters().expect("Metal adapters should enumerate");
    assert!(!adapters.is_empty());
    assert!(adapters.iter().enumerate().all(|(index, report)| {
        report.index == index && report.backend == "Metal" && !report.name.is_empty()
    }));
    assert!(matches!(
        Session::select_metal(adapters.len()),
        Err(Error::MetalAdapterIndexOutOfRange {
            requested,
            available
        }) if requested == adapters.len() && available == adapters.len()
    ));

    let session = Session::select_metal(0).expect("the first stable Metal adapter should open");
    let module = naga::front::wgsl::parse_str(NAGA_UNIFORM_SHADER)
        .expect("test WGSL should parse into Naga IR");
    let module = ValidatedNagaModule::new(module).expect("Naga IR should fully validate");
    assert_eq!(
        module.compute_entry_points().collect::<Vec<_>>(),
        ["transform"]
    );

    let uniform = encode_u32(&[3, 7, 0, 0]);
    let input = encode_u32(&[0, 1, 2, 5, 13, 21, 34, 55]);
    let output = vec![0u8; input.len()];
    let bindings = [
        BufferBinding {
            binding: 0,
            access: BindingAccess::Uniform,
            bytes: &uniform,
        },
        BufferBinding {
            binding: 1,
            access: BindingAccess::ReadOnlyStorage,
            bytes: &input,
        },
        BufferBinding {
            binding: 2,
            access: BindingAccess::ReadWriteStorage,
            bytes: &output,
        },
    ];
    let constants = [PipelineConstant {
        key: "7",
        value: 5.0,
    }];
    let report = session
        .dispatch_naga(
            &module,
            &NagaDispatchDescriptor {
                label: Some("validated Naga uniform proof"),
                entry_point: "transform",
                bindings: &bindings,
                constants: &constants,
                workgroups: [2, 1, 1],
            },
        )
        .expect("validated Naga IR should execute through Metal");

    assert_eq!(report.outputs.len(), 1);
    assert_eq!(report.outputs[0].binding, 2);
    assert_eq!(
        decode_u32(&report.outputs[0].bytes),
        [12, 15, 18, 27, 51, 75, 114, 177]
    );
    assert!(report.live_resources.is_zero());
    assert!(session.live_objects().is_zero());
}
