use std::{
    borrow::Cow,
    cell::Cell,
    num::NonZeroU64,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
        mpsc::TryRecvError,
    },
    thread,
    time::{Duration, Instant},
};

use wgpu::{
    Adapter, Backends, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BufferBindingType, BufferDescriptor, BufferUsages,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor, Device,
    DeviceDescriptor, ErrorFilter, Features, Instance, InstanceDescriptor, Limits, Maintain,
    MemoryHints, PipelineCompilationOptions, PipelineLayoutDescriptor, PowerPreference, Queue,
    RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, ShaderStages,
};

use crate::common::validate_dispatch;
use crate::{
    AdapterReport, BindingAccess, BufferOutput, DispatchDescriptor, DispatchReport, Error,
    ObjectCounts,
};

pub struct Session {
    _instance: Instance,
    _adapter: Adapter,
    device: Device,
    queue: Queue,
    adapter_report: AdapterReport,
    live_objects: Cell<ObjectCounts>,
    uncaptured_error: Arc<Mutex<Option<String>>>,
    poisoned: Arc<AtomicBool>,
}

impl Session {
    pub fn new_metal() -> Result<Self, Error> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or(Error::NoMetalAdapter)?;
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Metal {
            return Err(Error::UnexpectedBackend(format!("{:?}", info.backend)));
        }

        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                label: Some("AEXCompat bounded wgpu Metal device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: MemoryHints::MemoryUsage,
            },
            None,
        ))
        .map_err(|error| Error::RequestDevice(error.to_string()))?;

        let uncaptured_error = Arc::new(Mutex::new(None));
        let error_sink = Arc::clone(&uncaptured_error);
        device.on_uncaptured_error(Box::new(move |error| {
            if let Ok(mut stored) = error_sink.lock() {
                if stored.is_none() {
                    *stored = Some(error.to_string().chars().take(4_096).collect());
                }
            }
        }));
        let poisoned = Arc::new(AtomicBool::new(false));
        let device_poisoned = Arc::clone(&poisoned);
        let lost_error_sink = Arc::clone(&uncaptured_error);
        device.set_device_lost_callback(move |reason, message| {
            device_poisoned.store(true, Ordering::Release);
            if let Ok(mut stored) = lost_error_sink.lock() {
                if stored.is_none() {
                    *stored = Some(
                        format!("device lost ({reason:?}): {message}")
                            .chars()
                            .take(4_096)
                            .collect(),
                    );
                }
            }
        });

        Ok(Self {
            _instance: instance,
            _adapter: adapter,
            device,
            queue,
            adapter_report: AdapterReport {
                name: info.name,
                backend: format!("{:?}", info.backend),
                device_type: format!("{:?}", info.device_type),
                vendor: info.vendor,
                device: info.device,
                driver: info.driver,
                driver_info: info.driver_info,
            },
            live_objects: Cell::new(ObjectCounts::default()),
            uncaptured_error,
            poisoned,
        })
    }

    pub fn adapter_report(&self) -> &AdapterReport {
        &self.adapter_report
    }

    pub fn live_objects(&self) -> ObjectCounts {
        self.live_objects.get()
    }

    pub fn dispatch(&self, descriptor: &DispatchDescriptor<'_>) -> Result<DispatchReport, Error> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::SessionPoisoned);
        }
        validate_dispatch(descriptor)?;
        self.clear_uncaptured_errors();

        self.device.push_error_scope(ErrorFilter::OutOfMemory);
        self.device.push_error_scope(ErrorFilter::Validation);
        let result = self.dispatch_scoped(descriptor);
        let validation_error = pollster::block_on(self.device.pop_error_scope());
        let out_of_memory_error = pollster::block_on(self.device.pop_error_scope());
        let uncaptured_error = self.take_uncaptured_error();

        let final_result = if let Some(error) = out_of_memory_error {
            Err(Error::OutOfMemory(error.to_string()))
        } else if let Some(error) = validation_error {
            Err(Error::Validation(error.to_string()))
        } else if let Some(error) = uncaptured_error {
            Err(Error::Uncaptured(error))
        } else {
            result
        };
        if matches!(
            final_result,
            Err(Error::OutOfMemory(_)
                | Error::Uncaptured(_)
                | Error::DevicePoll(_)
                | Error::DispatchTimeout
                | Error::CompletionChannelDisconnected
                | Error::Map { .. }
                | Error::MapTimeout(_))
        ) {
            self.poison_and_destroy();
        }
        final_result
    }

    fn dispatch_scoped(
        &self,
        descriptor: &DispatchDescriptor<'_>,
    ) -> Result<DispatchReport, Error> {
        let writable_count = descriptor
            .bindings
            .iter()
            .filter(|binding| binding.access == BindingAccess::ReadWriteStorage)
            .count();
        let created_resources = ObjectCounts {
            buffers: descriptor.bindings.len(),
            staging_buffers: writable_count,
            shader_modules: 1,
            bind_group_layouts: 1,
            pipeline_layouts: 1,
            pipelines: 1,
            bind_groups: 1,
            command_buffers: 1,
        };
        let _live_guard = LiveObjectGuard::new(&self.live_objects, created_resources);

        let buffers = descriptor
            .bindings
            .iter()
            .map(|binding| {
                let mut usage = BufferUsages::STORAGE | BufferUsages::COPY_DST;
                if binding.access == BindingAccess::ReadWriteStorage {
                    usage |= BufferUsages::COPY_SRC;
                }
                let buffer = self.device.create_buffer(&BufferDescriptor {
                    label: descriptor.label,
                    size: binding.bytes.len() as u64,
                    usage,
                    mapped_at_creation: false,
                });
                self.queue.write_buffer(&buffer, 0, binding.bytes);
                buffer
            })
            .collect::<Vec<_>>();

        let layout_entries = descriptor
            .bindings
            .iter()
            .map(|binding| BindGroupLayoutEntry {
                binding: binding.binding,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage {
                        read_only: binding.access == BindingAccess::ReadOnlyStorage,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(binding.bytes.len() as u64),
                },
                count: None,
            })
            .collect::<Vec<_>>();
        let bind_group_layout = self
            .device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: descriptor.label,
                entries: &layout_entries,
            });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: descriptor.label,
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let shader = self.device.create_shader_module(ShaderModuleDescriptor {
            label: descriptor.label,
            source: ShaderSource::Wgsl(Cow::Borrowed(descriptor.wgsl)),
        });
        let pipeline = self
            .device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: descriptor.label,
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(descriptor.entry_point),
                compilation_options: PipelineCompilationOptions::default(),
                cache: None,
            });
        let bind_group_entries = descriptor
            .bindings
            .iter()
            .zip(&buffers)
            .map(|(binding, buffer)| BindGroupEntry {
                binding: binding.binding,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: descriptor.label,
            layout: &bind_group_layout,
            entries: &bind_group_entries,
        });

        let staging = descriptor
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| binding.access == BindingAccess::ReadWriteStorage)
            .map(|(index, binding)| {
                let buffer = self.device.create_buffer(&BufferDescriptor {
                    label: descriptor.label,
                    size: binding.bytes.len() as u64,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                (index, binding.binding, buffer)
            })
            .collect::<Vec<_>>();

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: descriptor.label,
            });
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: descriptor.label,
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                descriptor.workgroups[0],
                descriptor.workgroups[1],
                descriptor.workgroups[2],
            );
        }
        for (source_index, _, readback) in &staging {
            encoder.copy_buffer_to_buffer(
                &buffers[*source_index],
                0,
                readback,
                0,
                descriptor.bindings[*source_index].bytes.len() as u64,
            );
        }
        self.queue.submit(Some(encoder.finish()));

        let mut receivers = Vec::with_capacity(staging.len());
        for (_, binding, readback) in &staging {
            let (sender, receiver) = mpsc::sync_channel(1);
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result.map_err(|error| error.to_string()));
                });
            receivers.push((*binding, receiver));
        }
        let (completion_sender, completion_receiver) = mpsc::sync_channel(1);
        self.queue.on_submitted_work_done(move || {
            let _ = completion_sender.send(());
        });
        let mut map_results = (0..receivers.len())
            .map(|_| None)
            .collect::<Vec<Option<Result<(), String>>>>();
        let mut submission_complete = false;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match catch_unwind(AssertUnwindSafe(|| self.device.poll(Maintain::Poll))) {
                Ok(_) => {}
                Err(panic) => return Err(Error::DevicePoll(panic_message(panic))),
            }

            for ((binding, receiver), result) in receivers.iter().zip(&mut map_results) {
                if result.is_some() {
                    continue;
                }
                match receiver.try_recv() {
                    Ok(value) => *result = Some(value),
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        *result = Some(Err(format!(
                            "map callback channel disconnected for binding {binding}"
                        )));
                    }
                }
            }
            if !submission_complete {
                match completion_receiver.try_recv() {
                    Ok(()) => submission_complete = true,
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        return Err(Error::CompletionChannelDisconnected);
                    }
                }
            }
            if submission_complete && map_results.iter().all(Option::is_some) {
                break;
            }
            if Instant::now() >= deadline {
                if !submission_complete {
                    return Err(Error::DispatchTimeout);
                }
                if let Some(pending_binding) = receivers
                    .iter()
                    .zip(&map_results)
                    .find_map(|((binding, _), result)| result.is_none().then_some(*binding))
                {
                    return Err(Error::MapTimeout(pending_binding));
                }
            }
            thread::sleep(Duration::from_millis(1));
        }

        let mut outputs = Vec::with_capacity(staging.len());
        for ((_, binding, readback), map_result) in staging.iter().zip(map_results) {
            let map_result = map_result.ok_or(Error::MapTimeout(*binding))?;
            map_result.map_err(|message| Error::Map {
                binding: *binding,
                message,
            })?;
            let bytes = readback.slice(..).get_mapped_range().to_vec();
            readback.unmap();
            outputs.push(BufferOutput {
                binding: *binding,
                bytes,
            });
        }

        Ok(DispatchReport {
            adapter: self.adapter_report.clone(),
            workgroups: descriptor.workgroups,
            outputs,
            created_resources,
            live_resources: ObjectCounts::default(),
        })
    }

    fn clear_uncaptured_errors(&self) {
        if let Ok(mut error) = self.uncaptured_error.lock() {
            *error = None;
        }
    }

    fn take_uncaptured_error(&self) -> Option<String> {
        self.uncaptured_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
    }

    fn poison_and_destroy(&self) {
        self.poisoned.store(true, Ordering::Release);
        let _ = catch_unwind(AssertUnwindSafe(|| self.device.destroy()));
    }
}

struct LiveObjectGuard<'a> {
    counts: &'a Cell<ObjectCounts>,
}

impl<'a> LiveObjectGuard<'a> {
    fn new(counts: &'a Cell<ObjectCounts>, live: ObjectCounts) -> Self {
        counts.set(live);
        Self { counts }
    }
}

impl Drop for LiveObjectGuard<'_> {
    fn drop(&mut self) {
        self.counts.set(ObjectCounts::default());
    }
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "wgpu device poll panicked without a string payload".to_owned()
    }
}
