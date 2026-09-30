#![cfg(windows)]

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::{c_char, c_int};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;

const WGPU_VERSION: &str = "0.19.4";
const ELEMENT_COUNT: usize = 64;
const WGSL: &str = r#"
@group(0) @binding(0)
var<storage, read> input_values: array<u32>;

@group(0) @binding(1)
var<storage, read_write> output_values: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < 64u) {
        output_values[id.x] = input_values[id.x] * 3u + 7u;
    }
}
"#;

struct RuntimeState {
    _instance: wgpu::Instance,
    _device: wgpu::Device,
    _queue: wgpu::Queue,
    _input: wgpu::Buffer,
    _output: wgpu::Buffer,
    _readback: wgpu::Buffer,
}

static STATE: OnceLock<Mutex<Option<RuntimeState>>> = OnceLock::new();

#[derive(Serialize)]
struct AdapterObservation {
    name: String,
    adapter_luid: String,
    vendor_id: u32,
    device_id: u32,
    device_type: String,
    driver: String,
    driver_info: String,
    backend: &'static str,
}

#[derive(Serialize)]
struct SetupObservation {
    schema_version: u32,
    stage: &'static str,
    backend: &'static str,
    wgpu_version: &'static str,
    wgsl_sha256: String,
    element_count: usize,
    expected_values: Vec<u32>,
    actual_values: Vec<u32>,
    expected_sha256: String,
    actual_sha256: String,
    adapter: Option<AdapterObservation>,
    wgpu_compute_ready: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct SetdownObservation {
    schema_version: u32,
    stage: &'static str,
    cleanup_complete: bool,
    live_state_after_setdown: bool,
    error: Option<String>,
}

fn state() -> &'static Mutex<Option<RuntimeState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn values_sha256(values: &[u32]) -> String {
    sha256(bytemuck::cast_slice(values))
}

fn bounded_error(message: impl ToString) -> String {
    let mut value = message.to_string();
    if value.len() > 512 {
        value.truncate(512);
    }
    value
}

fn dx12_adapter_luid(adapter: &wgpu::Adapter) -> Result<u64, String> {
    unsafe {
        adapter.as_hal::<wgpu::hal::api::Dx12, _, _>(|hal_adapter| {
            let hal_adapter =
                hal_adapter.ok_or_else(|| "wgpu did not expose its DX12 adapter".to_owned())?;
            let mut description: winapi::shared::dxgi1_2::DXGI_ADAPTER_DESC2 = std::mem::zeroed();
            let status = hal_adapter
                .raw_adapter()
                .unwrap_adapter2()
                .GetDesc2(&mut description);
            if status < 0 {
                return Err(format!(
                    "IDXGIAdapter2::GetDesc2 failed with HRESULT 0x{:08x}",
                    status as u32
                ));
            }
            let low = u64::from(description.AdapterLuid.LowPart);
            let high = u64::from(description.AdapterLuid.HighPart as u32) << 32;
            let luid = high | low;
            if luid == 0 {
                return Err("wgpu DX12 adapter reported a zero LUID".to_owned());
            }
            Ok(luid)
        })
    }
}

fn failed_setup(message: impl ToString) -> SetupObservation {
    let expected_values = (0..ELEMENT_COUNT as u32)
        .map(|value| value * 3 + 7)
        .collect::<Vec<_>>();
    SetupObservation {
        schema_version: 1,
        stage: "failed",
        backend: "dx12",
        wgpu_version: WGPU_VERSION,
        wgsl_sha256: sha256(WGSL.as_bytes()),
        element_count: ELEMENT_COUNT,
        expected_sha256: values_sha256(&expected_values),
        actual_sha256: values_sha256(&[]),
        expected_values,
        actual_values: Vec::new(),
        adapter: None,
        wgpu_compute_ready: false,
        error: Some(bounded_error(message)),
    }
}

fn compute() -> Result<(SetupObservation, RuntimeState), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        dx12_shader_compiler: wgpu::Dx12Compiler::Fxc,
        flags: wgpu::InstanceFlags::VALIDATION,
        gles_minor_version: wgpu::Gles3MinorVersion::Automatic,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok_or_else(|| "wgpu found no hardware DX12 adapter".to_owned())?;
    let info = adapter.get_info();
    if info.backend != wgpu::Backend::Dx12 {
        return Err("wgpu selected a non-DX12 backend".to_owned());
    }
    let adapter_luid = dx12_adapter_luid(&adapter)?;

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("AEXCompat wgpu DX12 probe device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .map_err(|error| bounded_error(format!("request_device failed: {error}")))?;

    let input_values = (0..ELEMENT_COUNT as u32).collect::<Vec<_>>();
    let expected_values = input_values
        .iter()
        .map(|value| value * 3 + 7)
        .collect::<Vec<_>>();
    let byte_len = (ELEMENT_COUNT * std::mem::size_of::<u32>()) as u64;

    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("AEXCompat wgpu DX12 input"),
        contents: bytemuck::cast_slice(&input_values),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("AEXCompat wgpu DX12 output"),
        size: byte_len,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("AEXCompat wgpu DX12 readback"),
        size: byte_len,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("AEXCompat inline affine WGSL"),
        source: wgpu::ShaderSource::Wgsl(WGSL.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("AEXCompat wgpu DX12 bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("AEXCompat wgpu DX12 pipeline layout"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("AEXCompat wgpu DX12 compute pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: "main",
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("AEXCompat wgpu DX12 bind group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("AEXCompat wgpu DX12 command encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("AEXCompat wgpu DX12 compute pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, byte_len);
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| "wgpu readback callback was dropped".to_owned())?
        .map_err(|error| bounded_error(format!("wgpu readback mapping failed: {error}")))?;
    let mapped = slice.get_mapped_range();
    let actual_values = bytemuck::cast_slice::<u8, u32>(&mapped).to_vec();
    drop(mapped);
    readback.unmap();

    let ready = actual_values == expected_values;
    let observation = SetupObservation {
        schema_version: 1,
        stage: if ready {
            "compute_readback_complete"
        } else {
            "readback_mismatch"
        },
        backend: "dx12",
        wgpu_version: WGPU_VERSION,
        wgsl_sha256: sha256(WGSL.as_bytes()),
        element_count: ELEMENT_COUNT,
        expected_sha256: values_sha256(&expected_values),
        actual_sha256: values_sha256(&actual_values),
        expected_values,
        actual_values,
        adapter: Some(AdapterObservation {
            name: info.name,
            adapter_luid: format!("{adapter_luid:016x}"),
            vendor_id: info.vendor,
            device_id: info.device,
            device_type: format!("{:?}", info.device_type).to_ascii_lowercase(),
            driver: info.driver,
            driver_info: info.driver_info,
            backend: "dx12",
        }),
        wgpu_compute_ready: ready,
        error: (!ready).then(|| "wgpu readback did not match the fixed affine result".to_owned()),
    };
    Ok((
        observation,
        RuntimeState {
            _instance: instance,
            _device: device,
            _queue: queue,
            _input: input,
            _output: output,
            _readback: readback,
        },
    ))
}

fn write_json<T: Serialize>(value: &T, output: *mut c_char, capacity: usize) -> c_int {
    if output.is_null() || capacity == 0 {
        return 2;
    }
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(_) => return 3,
    };
    if bytes.len() + 1 > capacity {
        return 4;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast::<u8>(), bytes.len());
        output.add(bytes.len()).write(0);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn aexcompat_wgpu_dx12_global_setup(output: *mut c_char, capacity: usize) -> c_int {
    let mut guard = match state().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return write_json(
                &failed_setup("runtime state lock is poisoned"),
                output,
                capacity,
            );
        }
    };
    if guard.is_some() {
        return write_json(&failed_setup("global setup called twice"), output, capacity);
    }
    match compute() {
        Ok((observation, runtime)) => {
            *guard = Some(runtime);
            write_json(&observation, output, capacity)
        }
        Err(error) => write_json(&failed_setup(error), output, capacity),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aexcompat_wgpu_dx12_global_setdown(
    output: *mut c_char,
    capacity: usize,
) -> c_int {
    let mut guard = match state().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return write_json(
                &SetdownObservation {
                    schema_version: 1,
                    stage: "global_setdown",
                    cleanup_complete: false,
                    live_state_after_setdown: true,
                    error: Some("runtime state lock is poisoned".to_owned()),
                },
                output,
                capacity,
            );
        }
    };
    let had_state = guard.take().is_some();
    let live_state_after_setdown = guard.is_some();
    write_json(
        &SetdownObservation {
            schema_version: 1,
            stage: "global_setdown",
            cleanup_complete: had_state && !live_state_after_setdown,
            live_state_after_setdown,
            error: (!had_state).then(|| "global setdown had no live runtime state".to_owned()),
        },
        output,
        capacity,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_observation_never_claims_readiness() {
        let report = failed_setup("fixture");
        assert!(!report.wgpu_compute_ready);
        assert_eq!(report.stage, "failed");
        assert!(report.actual_values.is_empty());
    }
}
